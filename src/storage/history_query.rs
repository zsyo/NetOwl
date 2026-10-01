//! 历史查询与页面状态(conn_events 只读侧;写入见 history.rs)。
//! 归属地不落库,渲染时实时反查 geoip;全部查询走 rusqlite 参数绑定。

use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::System::Time::{
    FileTimeToSystemTime, SystemTimeToFileTime, SystemTimeToTzSpecificLocalTime,
};

use crate::model::Protocol;
use crate::storage::history::{Db, Writer, db_size, unix_now};

/// 历史库超容提醒阈值(1 GiB)
pub const REMIND_SIZE: u64 = 1024 * 1024 * 1024;
/// 明细/聚合单次查询行数上限
pub const QUERY_LIMIT: usize = 500;
/// 筛选输入防抖时长:连续按键合并为最后一次重查
const FILTER_DEBOUNCE: Duration = Duration::from_millis(300);
/// FILETIME(1601 起 100ns)与 unix 秒的基准差(秒)
const EPOCH_DELTA: u64 = 11_644_473_600;

/// 历史筛选条件;时间窗为 [start, +inf):连接存活期与窗口相交即命中
pub struct Filter {
    pub start: u64,
    /// 进程名模糊匹配(空 = 不过滤)
    pub process: String,
    /// 远端 IP 网段范围 [min, max](None = 不过滤)
    pub remote: Option<(u32, u32)>,
    pub proto: Option<Protocol>,
    /// 隐藏回环远端(127.0.0.0/8)
    pub hide_local: bool,
    /// 隐藏私网远端(RFC1918:10/8、172.16/12、192.168/16)
    pub hide_lan: bool,
}

impl Filter {
    /// WHERE 条件公共段(时间窗 + 筛选项,参数绑定)。
    /// 私网判定用位运算:10/8 = 前 8 位 10;172.16/12 = 前 12 位 0xAC1;
    /// 192.168/16 = 前 16 位 0xC0A8
    fn where_clause() -> &'static str {
        "WHERE last_seen >= ?1
             AND (?2 = '' OR process LIKE '%' || ?2 || '%')
             AND (?3 IS NULL OR (remote_ip >= ?3 AND remote_ip <= ?4))
             AND (?5 = '' OR proto = ?5)
             AND (?6 = 0 OR NOT ((remote_ip >> 24) = 127))
             AND (?7 = 0 OR NOT ((remote_ip >> 24) = 10
                      OR (remote_ip >> 20) = 2753
                      OR (remote_ip >> 16) = 49320))"
    }

    /// 与 where_clause 参数位一一对应(全部 owned);
    /// proto 为 None 时绑空串(SQL 侧以 ?5 = '' 判定不过滤,NULL 比较恒假)
    fn bind(&self) -> [Box<dyn rusqlite::ToSql>; 7] {
        [
            Box::new(self.start as i64),
            Box::new(self.process.clone()),
            Box::new(self.remote.map(|(lo, _)| lo as i64)),
            Box::new(self.remote.map(|(_, hi)| hi as i64)),
            Box::new(
                self.proto
                    .map(|p| p.as_str().to_owned())
                    .unwrap_or_default(),
            ),
            Box::new(self.hide_local as i64),
            Box::new(self.hide_lan as i64),
        ]
    }
}

/// 明细行(一条已完结连接)
pub struct DetailRow {
    pub event_id: u64,
    pub first_seen: u64,
    pub last_seen: u64,
    pub pid: u32,
    pub process: String,
    pub proc_path: Option<String>,
    pub proto: Protocol,
    pub remote_ip: Ipv4Addr,
    pub remote_port: u16,
    pub bytes_in: u64,
    pub bytes_out: u64,
}

/// 聚合行(进程 x 协议 x 远端)
pub struct AggregateRow {
    pub process: String,
    pub proto: Protocol,
    pub remote_ip: Ipv4Addr,
    pub count: u64,
    pub total_secs: u64,
    pub last_active: u64,
    pub bytes_out: u64,
    pub bytes_in: u64,
}

/// 聚合视图排序键
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AggregateSort {
    LastActive,
    BytesOut,
    BytesIn,
    Count,
    TotalSecs,
}

impl AggregateSort {
    /// SQL ORDER BY 列名映射(白名单常量,非外部输入)
    fn order_col(self) -> &'static str {
        match self {
            AggregateSort::LastActive => "last_active",
            AggregateSort::BytesOut => "out_total",
            AggregateSort::BytesIn => "in_total",
            AggregateSort::Count => "n",
            AggregateSort::TotalSecs => "total",
        }
    }
}

/// 汇总行(按进程聚合;同一进程名的多路径/PID 合并,路径取代表用于图标)。
/// Clone 供汇总视图克隆 SQL 结果后叠加活跃连接实时字节(不写回查询缓存)
#[derive(Clone)]
pub struct SummaryRow {
    pub process: String,
    pub proc_path: Option<String>,
    pub bytes_out: u64,
    pub bytes_in: u64,
    pub count: u64,
    pub total_secs: u64,
}

/// 汇总视图排序键
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SummarySort {
    BytesOut,
    BytesIn,
    Count,
    TotalSecs,
}

impl SummarySort {
    /// SQL ORDER BY 列名映射(白名单常量,非外部输入)
    fn order_col(self) -> &'static str {
        match self {
            SummarySort::BytesOut => "bytes_out",
            SummarySort::BytesIn => "bytes_in",
            SummarySort::Count => "n",
            SummarySort::TotalSecs => "total",
        }
    }
}

/// 明细查询:按最后活动倒序,最多 QUERY_LIMIT 行
pub fn query_detail(db: &Db, f: &Filter) -> rusqlite::Result<Vec<DetailRow>> {
    let sql = format!(
        "SELECT event_id, first_seen, last_seen, pid, process, proc_path, proto, remote_ip, remote_port,
                bytes_in, bytes_out
         FROM conn_events {} ORDER BY last_seen DESC LIMIT {QUERY_LIMIT}",
        Filter::where_clause()
    );
    let mut stmt = db.prepare_cached(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(f.bind()), |row| {
        Ok(DetailRow {
            event_id: row.get::<_, i64>(0)? as u64,
            first_seen: row.get::<_, i64>(1)? as u64,
            last_seen: row.get::<_, i64>(2)? as u64,
            pid: row.get::<_, i64>(3)? as u32,
            process: row.get(4)?,
            proc_path: row.get(5)?,
            proto: parse_proto(&row.get::<_, String>(6)?),
            remote_ip: Ipv4Addr::from(row.get::<_, i64>(7)? as u32),
            remote_port: row.get::<_, i64>(8)? as u16,
            bytes_in: row.get::<_, i64>(9)?.max(0) as u64,
            bytes_out: row.get::<_, i64>(10)?.max(0) as u64,
        })
    })?;
    rows.collect()
}

/// 聚合查询:按进程 x 协议 x 远端汇总(次数/累计时长/最近活动/字节总量)。
/// ORDER BY 列与方向来自排序枚举的白名单映射,不含外部输入
pub fn query_aggregate(
    db: &Db,
    f: &Filter,
    sort: AggregateSort,
    ascending: bool,
) -> rusqlite::Result<Vec<AggregateRow>> {
    let dir = if ascending { "ASC" } else { "DESC" };
    let sql = format!(
        "SELECT process, proto, remote_ip, COUNT(*) AS n,
                SUM(last_seen - first_seen) AS total, MAX(last_seen) AS last_active,
                SUM(bytes_out) AS out_total, SUM(bytes_in) AS in_total
         FROM conn_events {} GROUP BY process, proto, remote_ip
         ORDER BY {} {dir} LIMIT {QUERY_LIMIT}",
        Filter::where_clause(),
        sort.order_col()
    );
    let mut stmt = db.prepare_cached(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(f.bind()), |row| {
        Ok(AggregateRow {
            process: row.get(0)?,
            proto: parse_proto(&row.get::<_, String>(1)?),
            remote_ip: Ipv4Addr::from(row.get::<_, i64>(2)? as u32),
            count: row.get::<_, i64>(3)? as u64,
            total_secs: row.get::<_, i64>(4)?.max(0) as u64,
            last_active: row.get::<_, i64>(5)? as u64,
            bytes_out: row.get::<_, i64>(6)?.max(0) as u64,
            bytes_in: row.get::<_, i64>(7)?.max(0) as u64,
        })
    })?;
    rows.collect()
}

/// 汇总查询:按进程聚合(上/下行总量、次数、累计时长)。
/// ORDER BY 列与方向来自排序枚举的白名单映射,不含外部输入
pub fn query_summary(
    db: &Db,
    f: &Filter,
    sort: SummarySort,
    ascending: bool,
) -> rusqlite::Result<Vec<SummaryRow>> {
    let dir = if ascending { "ASC" } else { "DESC" };
    let sql = format!(
        "SELECT process, MAX(proc_path) AS proc_path,
                SUM(bytes_out) AS out_total, SUM(bytes_in) AS in_total,
                COUNT(*) AS n, SUM(last_seen - first_seen) AS total
         FROM conn_events {} GROUP BY process
         ORDER BY {} {dir} LIMIT {QUERY_LIMIT}",
        Filter::where_clause(),
        sort.order_col()
    );
    let mut stmt = db.prepare_cached(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(f.bind()), |row| {
        Ok(SummaryRow {
            process: row.get(0)?,
            proc_path: row.get(1)?,
            bytes_out: row.get::<_, i64>(2)?.max(0) as u64,
            bytes_in: row.get::<_, i64>(3)?.max(0) as u64,
            count: row.get::<_, i64>(4)? as u64,
            total_secs: row.get::<_, i64>(5)?.max(0) as u64,
        })
    })?;
    rows.collect()
}

/// 待确认的批量删除(右键菜单发起,确认弹窗执行)。
/// proto/remote_ip 均为 Some 时删除聚合组,否则删除整个进程
#[derive(Clone)]
pub struct PendingDelete {
    pub process: String,
    pub proto: Option<Protocol>,
    pub remote_ip: Option<Ipv4Addr>,
}

/// 明细行删除:事件身份 + 首末时刻定位唯一行(端口复用产生同 event_id
/// 的多次连接,由 first_seen 区分)
pub fn delete_detail(db: &Db, r: &DetailRow) -> rusqlite::Result<usize> {
    db.execute(
        "DELETE FROM conn_events WHERE event_id = ?1 AND first_seen = ?2 AND last_seen = ?3",
        rusqlite::params![r.event_id as i64, r.first_seen as i64, r.last_seen as i64],
    )
}

/// 聚合组删除:同进程 + 协议 + 远端的全部记录
pub fn delete_aggregate_group(
    db: &Db,
    process: &str,
    proto: Protocol,
    remote_ip: Ipv4Addr,
) -> rusqlite::Result<usize> {
    db.execute(
        "DELETE FROM conn_events WHERE process = ?1 AND proto = ?2 AND remote_ip = ?3",
        rusqlite::params![process, proto.as_str(), u32::from(remote_ip) as i64],
    )
}

/// 进程全部记录删除
pub fn delete_process(db: &Db, process: &str) -> rusqlite::Result<usize> {
    db.execute(
        "DELETE FROM conn_events WHERE process = ?1",
        rusqlite::params![process],
    )
}

fn parse_proto(s: &str) -> Protocol {
    if s == Protocol::Udp.as_str() {
        Protocol::Udp
    } else {
        Protocol::Tcp
    }
}

/// 远端前缀解析:"142.250." / "142.250.73.78" -> 网段范围(前缀补零);
/// 空串或非法输入返回 None(视为不过滤)
pub fn parse_ip_prefix(input: &str) -> Option<(u32, u32)> {
    let s = input.trim().trim_end_matches('.');
    if s.is_empty() {
        return None;
    }
    let mut octets = [0u8; 4];
    let mut filled = 0;
    for part in s.split('.') {
        let v: u8 = part.trim().parse().ok()?;
        // 先判段数再写入:5 段输入(如 1.2.3.4.5)直接拒绝,不能先下标
        if filled >= 4 {
            return None;
        }
        octets[filled] = v;
        filled += 1;
    }
    let min = u32::from(Ipv4Addr::new(octets[0], octets[1], octets[2], octets[3]));
    let max = if filled == 4 {
        min
    } else {
        min | ((1u32 << ((4 - filled) * 8)) - 1)
    };
    Some((min, max))
}

/// unix 秒时长 -> "H:MM:SS" / "M:SS"
pub fn fmt_duration(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// 本地时区本月 1 日 0 时起的 unix 秒
pub fn month_start() -> u64 {
    unsafe {
        let mut st = GetLocalTime();
        st.wDay = 1;
        st.wHour = 0;
        st.wMinute = 0;
        st.wSecond = 0;
        st.wMilliseconds = 0;
        let mut ft = FILETIME::default();
        SystemTimeToFileTime(&st, &mut ft).expect("[History] 本月起点换算失败");
        let ticks = ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64;
        (ticks.saturating_sub(EPOCH_DELTA * 10_000_000)) / 10_000_000
    }
}

/// unix 秒 -> 本地时间 "MM-DD HH:MM:SS"
pub fn fmt_local(unix: u64) -> String {
    unsafe {
        let ticks = unix.saturating_add(EPOCH_DELTA) * 10_000_000;
        let ft = FILETIME {
            dwLowDateTime: ticks as u32,
            dwHighDateTime: (ticks >> 32) as u32,
        };
        let mut utc = SYSTEMTIME::default();
        FileTimeToSystemTime(&ft, &mut utc).expect("[History] 时间换算失败");
        let mut local = SYSTEMTIME::default();
        SystemTimeToTzSpecificLocalTime(None, &utc, &mut local)
            .expect("[History] 本地时间换算失败");
        format!(
            "{:02}-{:02} {:02}:{:02}:{:02}",
            local.wMonth, local.wDay, local.wHour, local.wMinute, local.wSecond
        )
    }
}

/// 历史页视图形态
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Detail,
    Aggregate,
    Summary,
}

/// 时间范围档位
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Range {
    LastHour,
    Last6Hours,
    Last24Hours,
    Last7Days,
    ThisMonth,
}

impl Range {
    /// 档位对应的时间窗起点(unix 秒);本月取本地时区月初零点
    pub fn start(&self, now: u64) -> u64 {
        match self {
            Range::LastHour => now.saturating_sub(3600),
            Range::Last6Hours => now.saturating_sub(6 * 3600),
            Range::Last24Hours => now.saturating_sub(24 * 3600),
            Range::Last7Days => now.saturating_sub(7 * 86400),
            Range::ThisMonth => month_start(),
        }
    }
}

/// 查询结果(与视图形态对应)
pub enum Rows {
    Detail(Vec<DetailRow>),
    Aggregate(Vec<AggregateRow>),
    Summary(Vec<SummaryRow>),
}

/// 历史页状态:视图、筛选、结果与维护操作
pub struct PageState {
    pub view: ViewMode,
    pub range: Range,
    /// 进程名筛选(模糊)
    pub process: String,
    /// 远端 IP 前缀筛选文本
    pub remote: String,
    pub proto: Option<Protocol>,
    /// 聚合视图排序键与方向(默认最近活动降序)
    pub aggregate_sort: (AggregateSort, bool),
    /// 汇总视图排序键与方向(默认上传总量降序)
    pub summary_sort: (SummarySort, bool),
    pub rows: Rows,
    /// 结果或库大小需要重新加载
    pub dirty: bool,
    pub db_size: u64,
    /// 右键发起的待确认批量删除(确认弹窗,同帧或次帧内决出)
    pub pending_delete: Option<PendingDelete>,
    /// 已发起清理,延迟数帧后刷新(等写线程完成)
    purge_pending: Option<Instant>,
    /// 筛选输入防抖:输入每键一次全量 SQL 查询在大库上卡顿,记下
    /// 最后按键时刻,300ms 无后续输入才真正重查
    filter_debounce: Option<Instant>,
    /// 汇总视图"活跃合并"缓存:(生成时刻, 并入活跃连接后的行)。
    /// SQL 结果克隆+活跃聚合+重排每帧执行在交互帧重复,按秒失效复用;
    /// 重查后由 refresh_if_needed 置 None
    pub summary_merged: Option<(Instant, Vec<SummaryRow>)>,
}

impl PageState {
    pub fn new() -> Self {
        PageState {
            view: ViewMode::Detail,
            range: Range::Last24Hours,
            process: String::new(),
            remote: String::new(),
            proto: None,
            aggregate_sort: (AggregateSort::LastActive, false),
            summary_sort: (SummarySort::BytesOut, false),
            rows: Rows::Detail(Vec::new()),
            dirty: true,
            db_size: 0,
            pending_delete: None,
            purge_pending: None,
            filter_debounce: None,
            summary_merged: None,
        }
    }

    /// 筛选文本输入中:延迟 FILTER_DEBOUNCE 再重查(合并连续按键)
    pub fn defer_refresh(&mut self) {
        self.filter_debounce = Some(Instant::now());
    }

    /// 发起清理:删除 days 天前的数据(0 = 清空全部);
    /// 还原超容提醒由调用方处理(config)
    pub fn request_purge(&mut self, writer: &Writer, days: u32) {
        writer.purge(days);
        self.purge_pending = Some(Instant::now());
    }

    /// 按需重新加载:dirty(进入页面/筛选变化)或清空完成后;
    /// 本地/局域网过滤取自 config(两页共享,唯一来源)
    pub fn refresh_if_needed(&mut self, db: &Db, hide_local: bool, hide_lan: bool) {
        if let Some(at) = self.purge_pending
            && at.elapsed() >= Duration::from_millis(300)
        {
            self.dirty = true;
            self.purge_pending = None;
        }
        // 筛选输入防抖:静默期到达后合并为一次重查
        if let Some(at) = self.filter_debounce {
            if at.elapsed() >= FILTER_DEBOUNCE {
                self.dirty = true;
                self.filter_debounce = None;
            } else {
                return;
            }
        }
        if !self.dirty {
            return;
        }
        let filter = Filter {
            start: self.range.start(unix_now()),
            process: self.process.trim().to_owned(),
            remote: parse_ip_prefix(&self.remote),
            proto: self.proto,
            hide_local,
            hide_lan,
        };
        self.rows = match self.view {
            ViewMode::Detail => Rows::Detail(query_detail(db, &filter).unwrap_or_default()),
            ViewMode::Aggregate => {
                let (sort, asc) = self.aggregate_sort;
                Rows::Aggregate(query_aggregate(db, &filter, sort, asc).unwrap_or_default())
            }
            ViewMode::Summary => {
                let (sort, asc) = self.summary_sort;
                Rows::Summary(query_summary(db, &filter, sort, asc).unwrap_or_default())
            }
        };
        self.db_size = db_size();
        self.summary_merged = None;
        self.dirty = false;
    }
}
