//! 内存日志层:日志浏览窗口的数据源(参照 wallwarp 同名实现)。
//!
//! 独立于文件层与控制台的第三条 tracing 通道:把应用自身日志结构化写入
//! 全局环形缓冲,窗口经增量游标拉取。与另两层完全隔离:
//! - 展示档位由本模块独立开关控制,不影响两层输出
//! - 窗口未打开时为 off,on_event 快速短路,正常零开销
//! - 仅收录 target 以 netowl 开头的事件(第三方库噪声不进窗口)
//! - 按等级/时间戳/消息体结构化存储,UI 按等级着色,不做二次解析
//! - 本层为旁挂观察者,刻意不实现 enabled() 与 register_callsite():
//!   否则窗口未打开时经 callsite 兴趣缓存把事件点永久禁用,连带文件层
//!   与控制台层失效(max_level_hint 恒 TRACE 保证事件始终分发到本层)

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

use tracing::{Event, Level, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

use super::{LogLevel, local_timestamp};

/// 环形缓冲容量上限(超出丢最旧)
const BUFFER_CAPACITY: usize = 5000;
/// off 档位(窗口未打开)
const LEVEL_OFF: u8 = 0;

/// 一条结构化日志行(等级/时间戳与消息体分开存放,UI 按等级着色)
#[derive(Debug, Clone)]
pub struct LogLine {
    /// 单调递增序号(游标增量拉取的依据)
    pub seq: u64,
    /// 事件等级
    pub level: Level,
    /// 本地时间戳
    pub time: String,
    /// 消息体(不含时间戳与等级前缀)
    pub message: String,
}

struct LogBuffer {
    lines: VecDeque<LogLine>,
    next_seq: u64,
}

/// 当前展示档位(LEVEL_OFF = 关闭收集)
static SHOWN_LEVEL: AtomicU8 = AtomicU8::new(LEVEL_OFF);
static LOG_BUFFER: OnceLock<Mutex<LogBuffer>> = OnceLock::new();

fn buffer_cell() -> &'static Mutex<LogBuffer> {
    LOG_BUFFER.get_or_init(|| {
        Mutex::new(LogBuffer {
            lines: VecDeque::with_capacity(BUFFER_CAPACITY.min(256)),
            next_seq: 1,
        })
    })
}

/// LogLevel 到内部档位(数值越大越宽松;off 单独表示关闭)
fn level_flag(level: LogLevel) -> u8 {
    match level {
        LogLevel::Off => LEVEL_OFF,
        LogLevel::Error => 1,
        LogLevel::Warn => 2,
        LogLevel::Info => 3,
        LogLevel::Debug => 4,
        LogLevel::Trace => 5,
    }
}

fn event_flag(level: &Level) -> u8 {
    match *level {
        Level::ERROR => 1,
        Level::WARN => 2,
        Level::INFO => 3,
        Level::DEBUG => 4,
        Level::TRACE => 5,
    }
}

/// 设置窗口展示档位;None 关闭收集并清空缓冲。
/// 只作用于本层:文件层与控制台层的档位与开关均不受影响
pub fn set_shown_level(level: Option<LogLevel>) {
    let flag = level.map_or(LEVEL_OFF, level_flag);
    SHOWN_LEVEL.store(flag, Ordering::Release);
    if flag == LEVEL_OFF {
        clear();
    }
}

/// 读取序号大于 `seq` 的增量日志行(不重复、不跳过)。
/// 缓冲写满后最旧行被淘汰,游标过于滞后时返回的仍是存活的最新行
pub fn read_since(seq: u64) -> Vec<LogLine> {
    let buffer = buffer_cell().lock().unwrap_or_else(|e| e.into_inner());
    buffer
        .lines
        .iter()
        .filter(|line| line.seq > seq)
        .cloned()
        .collect()
}

/// 清空环形缓冲(序号继续递增,旧游标自动失效)
pub fn clear() {
    let mut buffer = buffer_cell().lock().unwrap_or_else(|e| e.into_inner());
    buffer.lines.clear();
}

fn push_line(level: Level, time: String, message: String) {
    let mut buffer = buffer_cell().lock().unwrap_or_else(|e| e.into_inner());
    let seq = buffer.next_seq;
    buffer.next_seq += 1;
    if buffer.lines.len() >= BUFFER_CAPACITY {
        buffer.lines.pop_front();
    }
    buffer.lines.push_back(LogLine {
        seq,
        level,
        time,
        message,
    });
}

/// 从事件中提取 message 字段(其余字段忽略,消息体自带 [模块] 前缀)
struct MessageVisitor(String);

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" && self.0.is_empty() {
            self.0 = format!("{value:?}");
        }
    }
}

/// 内存日志层:事件经预过滤后格式化入环形缓冲(旁挂观察者)
pub struct MemoryLayer;

impl<S: Subscriber> Layer<S> for MemoryLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        // 旁挂过滤:开关非零 + 仅应用自身 + 事件等级不高于展示档位
        let shown = SHOWN_LEVEL.load(Ordering::Acquire);
        if shown == LEVEL_OFF {
            return;
        }
        if !event.metadata().target().starts_with("netowl") {
            return;
        }
        if event_flag(event.metadata().level()) > shown {
            return;
        }

        let mut visitor = MessageVisitor(String::new());
        event.record(&mut visitor);
        if visitor.0.is_empty() {
            return;
        }

        push_line(*event.metadata().level(), local_timestamp(), visitor.0);
    }

    /// 等级提示恒为 TRACE:保证任何档位的事件都分发到本层,再由
    /// on_event 过滤;各层独立过滤,其他层输出不受影响
    fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
        Some(tracing::level_filters::LevelFilter::TRACE)
    }
}
