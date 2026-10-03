//! 新连接询问(Little Snitch 式):未命中任何规则的公网新连接弹窗询问
//! 允许/拒绝与生效范围,倒计时超时自动执行默认动作(拒绝·仅本次)。
//!
//! 触发粒度 = 进程 + 目标 IP(端口/协议不参与去重,同目标多端口只问一次);
//! 回环/局域网/保留段目标、系统进程、UDP 无远端行不询问(静默放行),
//! 待询问队列超上限时同样静默放行。询问等待期间该身份被临时阻断
//! (最高 weight 的 pending 过滤器,安全默认);仅本次决策只作用于
//! 当前连接(拒绝时生成含本地端口的临时规则,连接结束即清理),
//! 永久选项落库,WFP 拦截与列表标注随之生效。
//! 询问条目与判定在 item,询问编排状态机在 asker。

mod asker;
mod item;

use std::time::Duration;

pub use asker::Asker;
pub use item::{AskItem, Decision, Scope, temp_rule_holds};

/// 倒计时:超时自动执行默认动作(拒绝·仅本次)
pub const ASK_TIMEOUT: Duration = Duration::from_secs(30);
