# NetOwl 项目记忆文件

## 项目概述
- 项目名称: NetOwl
- 项目类型: 网络连接监控工具(Windows 平台的 Little Snitch 复刻,首期定位只读监控+流量地图)
- 开发语言: Rust
- GUI 框架: egui/eframe 0.36(glow 后端),即时模式,纯 Rust 自绘渲染,不依赖 webview
- 支持平台: 前期仅 Windows 10+;macOS/Linux 为后续扩展目标,架构需为此预留边界
- 核心功能: 流量地图(本机节点为中心,连接动画可视化)、连接列表;拦截与规则引擎为后续里程碑
- 许可: GPLv3(仓库根 LICENSE)

## 选型记录(勿轻易重议)
- egui/eframe 0.36:即时模式 painter 可直接绘制节点/连线/动画,同一库覆盖表单式 UI(连接、规则)与画布式 UI(地图);MIT/Apache 双许可
- 否决项: iced 0.14(Elm 架构清晰但 canvas 自定义绘制样板多)、slint 1.16(地图仍需嵌 canvas,社区较小)、xilem(实验性)、floem(生态小)、Tauri(webview,违背"非 web"硬性要求)、native-windows-gui(Win32 控件做不出流量地图)
- 渲染后端用 glow 而非默认 wgpu:编译更快、依赖更少,Windows 上 OpenGL 足够

## 项目结构
src 为 lib crate(main.rs 仅入口,lib.rs 为 crate 根,bin 经 netowl:: 引用),
按功能域分目录;子目录统一用 mod.rs 风格。拆分组织模式(参照 wallwarp):
单一文件不超过 300 行;大结构体单点定义在目录 mod.rs,方法 impl 块按
功能域分散到子模块文件;目录 mod.rs 尽量纯声明,跨目录引用的符号经
`pub use` re-export 保持 `crate::xxx::符号` 原路径(外部调用零改动);
拆出方法可见性用 pub(super),不放宽。

- src/main.rs - 程序入口(NativeOptions、窗口最小/默认尺寸、窗口图标、run_native)
- src/lib.rs - crate 根:模块声明
- src/app/ - 应用编排(结构体 NetOwlApp 单点定义于 mod.rs(全部字段)与共享
  常量/WindowRect;frame.rs:eframe::App trait 实现——logic 每帧逻辑编排
  [托盘命令→可见性校准→注册表同步→采集轮询→页面切换检测→几何→关闭拦截→
  配置防抖落盘] 与 ui 每帧绘制编排[标题栏/导航/地图面板/中央面板/重绘节奏/
  边缘缩放/浮层]+ 语言主题→config 同步 + mark_config_dirty;trait impl
  不可跨文件,logic+ui 必须同块;etw_merge.rs:ETW 合并——TCP 精确键字节
  回填、UDP 按 (pid,本地端口) 归并回填远端+字节、udp_last_remote 缓存回退、
  每连接实时速率、短命连接收割落盘;window.rs:窗口几何恢复/捕获、可见性
  跟踪与单实例唤出校准;tray.rs:托盘命令分发[显示/设置/日志/询问开关/
  静默三态/退出收尾]、托盘常驻与自启动写入(管理员令牌经计划任务、标准用户经 Run 键;失败定时重试)、tooltip
  速率刷新、双入口勾选态校准;poll.rs:采集编排——连接快照 1s 轮询
  [UDP 无远端过滤/去重→rDNS→图标→历史→询问→临时规则]、公网 IP 重探
  [启动定位窗口 15s 内首个结果把地图瞬跳到本机中心最大缩放,窗口后不再
  自动定位]、
  总速率采样、图标纹理缓存(键=映像路径,ColorImage::from_rgba_unmultiplied
  建纹理)、WFP 目标集合同步[规则 specs+询问 pending 阻断+静默 deny
  兜底/自身放行]、悬浮球数据快照;ask_flow.rs:询问编排——入队检测、
  超时默认拒绝、决策落地[永久落库/仅本次临时规则]、临时规则生命周期
  (连接消失即删);ball.rs:悬浮球交互结果落地(位置记忆/唤出主窗口/
  开关闭合);init.rs:NetOwlApp::new 构造(托盘创建、历史库/规则档、
  ETW 启动、logo 纹理、全字段初始化);
  ask/:新连接询问引擎(mod.rs:ASK_TIMEOUT + re-export;item.rs:AskItem
  身份快照[remote_display/pending_block_spec/to_rule]、Scope/Decision、
  可询问目标判定 is_askable、身份键 identity_key、temp_rule_holds;
  asker.rs:Asker 状态机——触发身份=进程+目标IP,静默放行自身/系统/
  未知进程/回环/局域网/保留段/UDP 无远端/队列超限,启动基线仅首个非空
  快照做一次(休眠唤醒快照短暂为空不得重新基线化),决策=动作x范围,
  仅本次只作用于当前连接(拒绝=含本地端口的临时规则,连接消失即清理,
  决策后解除身份去重重连重问;允许=不产生规则),超时自动拒绝;
  与静默模式互斥,allow/deny 下队列与弹窗一并清空;
  lan.rs:局域网设备发现编排——10s 轮询 ARP(net/lan.rs),条目合并
  lan_devices 表(新 MAC 插入留痕,已知项刷新 last_seen),视图行带
  online/is_new 合并口径)
- src/model/ - 共享数据模型(mod.rs:Connection/Protocol/Place 等数据结构;
  city 为 Option<Place>:内网/保留段/未收录 IP 归属未知,地图不绘制,列表
  显示占位;UDP 表行远端以 *:* 占位,ETW 合并后仍无远端的行由 app 层
  retain 排除:列表/地图不显示,Tracker 不落库)
- src/collector/ - 连接采集(mod.rs:Collector trait 与 real/mock 工厂 +
  CollectorKind,icon_image 默认返回 Pending;mock.rs:模拟数据供演示/测试;
  query.rs:Win32 查询原语,GetExtendedTcpTable/GetExtendedUdpTable owner-PID
  快照(TCP 仅活动状态,两段式缓冲重试)与 OpenProcess+QueryFullProcessImageNameW
  全路径反查;反查被目标 DACL 拒绝(非提权/受保护服务)时以
  NtQuerySystemInformation 全量进程名快照兜底(任务管理器同源,无需句柄,
  拿名字无路径,签名/图标维持未知);windows_table.rs:真实采集器,1s 节流,
  TCP 过滤 SYN_SENT..LAST_ACK,
  连接身份四元组+PID hash 派生稳定 id,进程元数据(名/路径/签名)按 PID 缓存、
  行消失即剔除(仅成功项,无名行不缓存每轮重查防瞬时失败固化;PID 4 特判
  System),图标按路径常驻缓存(连接关闭后进程再现
  即取即用);signature.rs:WinVerifyTrust Authenticode 校验(UI_NONE+REVOKE_NONE
  不弹窗不联网,须在工作线程跑),每轮限流派发(在途 4/每轮 2 个)结果回填,
  回填前 Unknown;icon.rs:SHGetFileInfoW 取 32x32 关联图标 + GetIconInfo/
  GetDIBits 转 RGBA(工作线程执行,失败缓存 None);表快照无字节语义,
  字节由 app 层从 ETW 流合并填充;均为只读 API,无需管理员权限)
- src/rules/ - 规则与拦截(结构体 RuleSet 单点定义于 mod.rs:规则模型
  (动作/方向/协议/进程/远端[网段或域名]/端口/优先级)、MatchReq 求值输入、
  conn_direction 方向判定(ETW initiated_out 真实方向优先,未知时 >=49152 入站近似兜底)、parse_net、
  SILENT_FALLBACK_ID/WEIGHT_RESERVED_HIGH/WEIGHT_FALLBACK;profile.rs:
  RuleSet impl 配置档管理——db v7 profiles 表 + rules.profile_id,规则按
  配置档隔离,load/insert 作用于当前档,switch_profile 重载并保留会话
  临时规则,ensure_default_profile 启动兜底,数据库行反序列化 parse_*;
  eval.rs:RuleSet impl 求值——priority 升序首个命中,未命中返回静默兜底
  (若开启)否则放行;静默拒绝兜底 = 全通配 Block 内存规则(不入 rules
  列表,evaluate 自动联动连接标注与地图开关);blocking_rule/
  process_block_rule 供地图面板标注;store.rs:RuleSet impl CRUD 与会话
  临时规则(负数 id 不落库);specs.rs:RuleSet impl wfp_specs 翻译启用规则
  为 WFP 过滤器目标集——进程条件按映像名展开为完整路径集合(粘滞缓存防
  拦截窗口抖动),网段 RANGE、端口/协议等值,Any 方向拆 CONNECT/
  RECV_ACCEPT 两层;weight 布局:15=询问 pending/静默自身放行(互斥),
  14..1=用户规则(超 14 条钳制到 1),0=静默兜底/子层基线;域名规则
  不参与翻译,仅求值标注;wfp/:WFP 拦截引擎(mod.rs:管理线程持动态会话
  [进程退出/崩溃内核对象自毁],单子层 weight 0 低于系统防火墙,非提权
  回落 NoAdmin 只读;静默拒绝下 app 层追加自身放行(当前 exe,weight 15)
  与通配阻断(weight 0);filter.rs:Spec→FWPM_FILTER0 的 FFI 构造
  [app id blob/网段 RANGE/协议端口等值]))
- src/storage/ - 持久化(config.rs:应用配置(config.toml,serde TOML + 防抖
  写盘);db.rs:SQLite 连接与结构迁移(v5 = conn_events 补 bytes_in/out 列;
  v6 = lan_devices 局域网设备表;v7 = profiles 配置档表 + rules.profile_id);
  history/:历史落盘(mod.rs:ClosedConn、Tracker 对比前后快照生成事件
  [mock 不入库]、Writer 写线程句柄与 run 主循环、write_batch 批量事务
  [conn_events 事件表,每条已完结连接一行整行 INSERT,含收发字节——
  Tracker 每轮随快照刷新、短命连接取 FlowAgg;托盘退出 flush 并 join;
  归属地不落库];maintenance.rs:自动清理小时级节流[天数 0 = 不清理]、
  purge_before/reclaim_space[DELETE 后归还空闲页+截断 WAL,
  incremental_vacuum 须消费全部结果行]、db_size);
  history_query/:历史查询与页面状态(mod.rs:REMIND_SIZE/QUERY_LIMIT +
  re-export;filter.rs:Filter[WHERE 公共段+参数绑定]、明细/聚合/汇总/
  用量行类型与排序枚举[order_col 白名单列名映射拼 ORDER BY];query.rs:
  明细/聚合/汇总/用量 SQL 全参数绑定,LIMIT 500,远端前缀解析为网段
  BETWEEN;proto None 绑空串而非 NULL——SQL 侧 ?5 = '' 判定,NULL 比较
  恒假会过滤全部行;本地/私网过滤在 SQL 内位运算判定;PendingDelete 与
  明细行/聚合组/整进程删除;fmt.rs:local_tz_offset_secs/month_start/
  fmt_local[Win32 SystemTime]、parse_ip_prefix、fmt_duration;page.rs:
  ViewMode/Range/Rows/PageState[视图/筛选/排序/结果/清空,筛选防抖
  300ms,汇总活跃合并缓存]))
- src/net/ - 网络信息(geoip.rs:GeoIP 归属定位(assets/geoip.bin 编译期内嵌,
  首用解析一次;IPv4 区间表 LEB128 delta 编码二分查询;城市级粒度:中国含港澳台
  到地级市、外国按城市名匹配 GeoNames,未命中回退省/国家级;place_pos/place_label
  为地图与列表的统一归属坐标/显示名入口,显示名双语内嵌不走词条);
  local_ip.rs:本机公网 IP 探测(6 个知名公共回显接口并发,手写 HTTP/1.1 GET
  不走系统代理、不引 TLS 依赖,最先返回的合法 IPv4 胜出;app 层每 10 分钟重探,
  经 geoip 得到本机地图点位,失败回退 map::world::LOCAL;NETOWL_IP 环境变量
  可指定固定本机 IP 隐藏真实位置[截图场景],生效时不发起任何真实探测);rdns.rs:rDNS 域名
  解析(异步 PTR:getnameinfo NI_NAMEREQD 于独立线程执行,app 每帧 update 收割;
  并发上限 8 + 每 250ms 派发 2 个限流,成功/失败分别 10min/2min TTL 缓存,失效
  仅对仍活跃连接重查;回环/私网/保留段不查;lookup 供列表与地图信息卡域名优先
  显示,rdns::display 超长截断);traffic.rs:总上传/下载速率(GetIfTable2 各接口
  In/OutOctets 采样差值;排除回环/隧道;**必须排除
  InterfaceAndOperStatusFlags.FilterInterface(bit1) 接口——WFP 轻量过滤/QoS
  过滤接口会镜像底层物理网卡计数,不过滤速率成倍虚高**;可见/隐藏均 1s 采样
  (托盘悬停提示跟随秒级刷新),表查询失败沿用旧速率);etw/:ETW 流量事件
  采集(mod.rs:Microsoft-Windows-Kernel-Network 实时会话,命名实例启动清理
  残留/退出停止,需管理员权限,未提权不启动;手写 windows crate 消费者
  StartTraceW→EnableTraceEx2→OpenTraceW 回调→ProcessTrace;FlowKey/FlowAgg/
  Etw 句柄[snapshot 空闲流收割/流表 4096 上限/take_finished];decode.rs:
  payload 前 20 字节同构硬编码解析:PID/size/daddr/saddr/dport/sport,
  端口网络序、IPv4 BE;事件视角归一本地/远端:发送/发起/接受/关闭
  saddr 为本机侧、接收 daddr 为本机侧;消费 10/11 TCP 收发、42/43 UDP 收发、
  12/15/13 连接建立/接受/关闭,18 与 11 双计、IPv6 系列忽略;TCP close 完结,
  UDP 10s/TCP 60s 空闲兜底;app 层按 PID+协议+本地端口+远端
  合并填充连接字节,完结流未被表快照覆盖即短命连接落盘历史,回环不入库;
  lan.rs:局域网设备发现(GetIpNetTable IPv4 ARP 缓存只读查询,两段式
  缓冲;排除无效表项与组播/广播 MAC[首字节 I/G 位],dwAddr 网络序还原;
  DeviceRow 视图行 = 库记录 + 本轮在线状态合并,免管理员权限))
- src/map/ - 流量地图(mod.rs:画布(painter 自绘:节点聚合与脉冲、悬停
  信息卡、视图交互 handle_input[滚轮锚点缩放/拖拽/双击复位默认视图]与
  clamp_view[纬度钳制视口不脱出底图数据窗口 FIT_MIN/MAX_LAT -58..84],
  home_view 默认视图 = 本机中心+最大缩放(与双击复位共用,app 层启动
  定位复用;定位用 View::at 瞬跳——走 target 动画会被途中小 zoom 的
  大纬度半窗钳坏 target_lat),视图状态存 NetOwlApp;经度方向无缝循环:
  中心经度归一化 [-180,180),节点
  按可见副本平移绘制,悬停按模周期距离);flights.rs:贝塞尔连线与粒子尾迹
  (远端归属+主导方向聚合 LOD 一城一线,线宽随聚合数增长;连线取最短方向
  走短弧,按可见世界副本平移铺开);basemap/:地图底图(mod.rs:绘制入口
  [世界层/中国层/河流/十段线层序]、经纬网格、陆地与洞环填充、海岸线/国界
  quad 线段[每帧视口剔除即时渲染,无缓存无状态];projection.rs:View 视图
  状态[动画目标趋近]与等距圆柱投影[周期副本平移/可见副本区间];
  labels.rs:名称标签按缩放分级显隐,英文国家名大写逐字符字距,省名弱化色);
  triangulate.rs:简单多边形耳剪三角化(输入量化整数坐标,精确
  几何判定;f32 坐标会破坏共线性导致耳被误判,勿改回浮点);world/:城市坐标
  与矢量底图数据(mod.rs:Ring/MapLevel/MapData 模型与 map_data 懒初始化;
  cities.rs:City/LOCAL/CITIES 静态表;decode.rs:mapdata.bin 解码[Natural
  Earth 世界 + DataV 中国混合,两档 LOD:110m 全局/50m 放大,zoom>=3 切换;
  海岸线/国界按邻国共享边分类;labels 三种 kind + 十段线段节 + 河流折线节
  两档]))
- src/ui/ - 界面(mod.rs:主窗口布局与页面(UiCtx、TOOLBAR_ROW_H 工具栏行高
  共用常量、text_width、Page/ConnSort;conn_visible 为连接列表与地图页左右
  面板共用的本地/局域网远端过滤口径[hide_local/hide_lan];central_ui 页面
  分发与 map_header/panel_toggle/legend);
  nav.rs:左侧导航栏(品牌区、页面切换[选中指示条动画]、底部速率卡
  [一分钟双色走势 + 当前速率 + 会话累计]与监控状态行);
  titlebar.rs:自绘无边框标题栏(拖动 StartDrag/双击最大化/窗控三钮,关闭走
  隐藏到托盘同路径,app 层处理 TitleAction);
  connections/:连接页(mod.rs:过滤/搜索编排与表头可排序[header_sort_cell
  整格可点,常驻置灰上下双三角标识可排序,激活列点亮当前方向,占位与状态
  无关防列宽抖动];rows.rs:行渲染[行悬停高亮 Order::Background 垫底、进程
  两行列/协议徽章/rDNS 域名两行列/速率与累计数字列右对齐/规则求值动作
  徽章];menu.rs:行右键菜单[结束连接=SetTcpEntry DELETE_TCB,仅 TCP 行且
  提权可用,非提权禁用带提示/定位程序/复制远端地址/复制进程路径];
  sort.rs:表头排序);
  settings/:设置页(mod.rs:ScrollArea 编排 + section_card/setting_row 行式
  布局[左标签右控件] + locate_log_file;appearance.rs:语言/主题分段切换/
  悬浮球开关;monitor.rs:数据源/保留期/询问/静默模式三态分段
  [off/allow/deny]/托盘常驻/自启动;logging.rs:日志文件开关/级别/浏览入口);
  widgets/:公共组件库(header 页头/badge 胶囊徽章/
  segmented 分段选择/toggle 滑动开关/table 统一表头与行底色[列贴列布局
  (Grid spacing.x=0),内容与列缘间距由 CELL_PAD_X 提供,定宽列宽须含
  2×CELL_PAD_X;num_cell 数字右对齐单元格,Grid 末列须空占位防右对齐
  横跨;Grid 格内禁用 add_space(egui 断言 panic),缩进须用定宽占位 +
  max_rect shrink 子区域]/process 进程
  图标占位/card 卡片/sparkline 多序列迷你走势图/bar_chart 双序列时间桶
  柱状图[mod.rs 三段式桶宽/粘性跟随/Y 归一/X 标签稀疏化,tooltip.rs 悬停
  桶标签+精确字节数据卡]/menu 菜单项按钮,新页面禁止重复实现);
  ask.rs:新连接询问弹窗(右下角无标题栏 toast,盾形图标标题,范围下拉 +
  允许[accent 填充 on_accent 字]/拒绝[danger 描边],进度条内嵌剩余秒数);
  history/:历史页(mod.rs:show 入口与四视图分发 + 超容提醒卡[warn 低透明
  底 + 警示图标,勾选不再提醒=一票否决持久化,手动清空还原];toolbar.rs:
  四视图 segmented 切换/时间范围/进程远端协议筛选[300ms 防抖]/导出按钮/
  隐藏开关/库大小与清理菜单;detail.rs:明细 8 列[行右键删该条];
  aggregate.rs:聚合 9 列表头可排序[行右键删该组];summary.rs:进程汇总
  [按进程聚合字节总量/次数/时长,默认上传降序,数字含活跃连接实时字节——
  活跃合并缓存按秒失效不写回查询缓存;汇总行整行点击下钻明细并按进程名
  过滤,空进程名行禁用,行右键删该进程全部];usage.rs:用量[按本地时区
  日界或小时界分桶柱状图,query_usage GROUP BY 桶求和,未提权全零时提示
  不渲染空图];export.rs:CSV 导出覆盖全部四视图[UTF-8 BOM];cells.rs:
  手动斑马行底色/进程/位置[实时反查 geoip]/字节单元格;menu.rs:右键菜单;
  confirm.rs:批量删除 egui::Modal 确认[删除 SQL 全参数绑定;定位与复制
  同连接页];表格口径与连接页一致);
  rules/:规则页(mod.rs:show 入口/PageState/Feedback/WFP 拦截状态行
  [盾形图标]/工具栏档位下拉切换与管理按钮/primary_btn;draft.rs:编辑草稿
  与 Rule 互转;table.rs:规则表格[toggle 启停、动作/协议徽章、图标操作钮
  (删除悬停警示),删除后立即结束本帧表格防索引越界];edit.rs:编辑弹窗
  带校验与落库;labels.rs:方向/协议/动作/远端显示文案;import_export.rs:
  当前档规则 JSON 导入导出[rfd 模态];profile_manager.rs 弹窗:档列表带
  规则数/新建/行内重命名/复制档/删除(当前档置灰,确认后连同规则删除),
  show 返回配置变更标志);lan.rs:局域网设备页(ARP 设备表
  IP/MAC/最近在线/首见/在线状态点/新徽章[首见 24h 内],行右键复制 IP 与
  MAC,统计行显示在线数;设备量小不虚拟化,表头固定滚动区外);
  log_window.rs:日志浏览窗口(样式与主窗口刻度对齐);
  map_panel/:地图左面板(mod.rs:MapPanelState/RankSort/ProcGroup/
  collect_groups 分组聚合与 list_panel[搜索/端点过滤条/WFP 状态提示];
  group_row.rs:进程组头行[名称+会话累计上/下行副行+连接数徽章+进程级
  阻断开关]);map_inspector/:右 Inspector(mod.rs:三态切换与 title_row;
  summary.rs:概览[流量卡+进程/域名排行,各自独立的总量/上传/下载排序
  切换,上传占优行警示色];place.rs:端点详情;process.rs:进程详情[路径/
  签名/阻断开关/连接明细];行内变长文本统一"固定宽度容器 + Truncate"防
  面板宽度记忆膨胀);map_widgets.rs/map_conn.rs:面板共享小件与连接明细行;
  icons.rs:Bootstrap Icons 码点常量表(glyph 名注释即契约);theme.rs:主题
  (theme/palette.rs:深/浅两套 Palette 调色板纯数据;AtomicUsize 主题索引,
  theme::c() 统一取色;设计刻度:font 字号/sp 间距/圆角三档+PILL/
  window_shadow+popup_shadow;Visuals 双主题定制含窗口描边与投影;字体加载;
  设置页切换即时生效,持久化于 config [general] theme))
- src/i18n/ - 多语言模块(mod.rs:locales 扫描/加载/语言列表;translate.rs:查找/插值/回退/告警)
- locales/ - fluent 词条文件(zh-cn.ftl / en.ftl;目录缺失时使用编译期内嵌兜底)
- src/platform/ - 平台集成(paths.rs:数据根目录 = exe 同级(Windows 便携式),
  open_in_explorer/select_in_explorer 资源管理器定位原语[/select, 后须
  raw_arg 防路径含空格被拆参];
  icon.rs:应用图标加载(assets 资源编译期内嵌,PNG 解码为 RGBA);tray.rs:
  托盘与菜单(tray-icon + muda;静默模式子菜单三态 CheckMenuItem[句柄
  保活,app 侧 sync_silent 以 config 为唯一来源校准勾选态],tooltip 跟随
  实时速率 1s 刷新);tray_pin.rs:托盘常驻注册表写入(NotifyIconSettings
  IsPromoted,按 ExecutablePath 匹配本进程定位);shutdown_hook.rs:关机
  落库钩子(子类化主窗口拦 WM_ENDSESSION,执行 tracker.flush+
  writer.shutdown——winit 不处理 ENDSESSION,关机时进程被强杀,该钩子是
  托盘退出外唯一落库路径))
- tools/build_mapdata.py - 底图数据生成脚本(混合数据源 ->
    assets/mapdata.bin;原始 GeoJSON 放 tools/cache/,该目录不入库)
- tools/build_geoip.py - GeoIP 归属数据生成脚本(tools/cache/ip2region_v4.xdb
  [Apache-2.0,持续更新] + DataV 100000_full.json 与各省 {adcode}_full.json
  [市级政府驻地坐标] + NE 50m admin_0 + GeoNames cities15000[公有领域,外国
  城市坐标与英文名] -> assets/geoip.bin,NWGI v1 格式;城市级:中国含港澳台
  到地级市[台湾 DataV 无 full 数据回退省级]、外国 (ISO,城市名) 匹配 GeoNames
  同名取人口最多者,未命中回退省/国家级;55 万条 -> 合并 49.6 万条/约 2.7MB;
  重生成需将数据源文件放入 tools/cache/,缺失的会自动下载)

## 架构规范(egui 即时模式)
- 数据与绘制分离:collector 产出模型状态,ui/map 只读绘制,绘制逻辑不修改数据
- 状态集中:全部可变应用状态收敛在 NetOwlApp;绘制函数只接收不可变引用
- 系统调用隔离:平台/采集逻辑不得散落在 UI 层;Collector trait 是唯一数据入口,
  后续替换 MockCollector 为 ETW 实现时不改 UI 代码
- 按需重绘:动画存在时 request_repaint_after(约 30fps),静态页面用长间隔(约 500ms),
  禁止盲目持续 60fps 空转耗电

## 平台规范(Windows 首期)
- 字体:运行时加载 C:\Windows\Fonts\msyh.ttc(微软雅黑)注册为中文 fallback,
  Consolas 注册到 monospace;文件缺失时直接报错,不做静默降级
- 自绘标题栏:主窗口 with_decorations(false),标题栏由 ui::titlebar 自绘
  (空白区 StartDrag 拖动/双击切换最大化/最小化/最大化/关闭三钮);关闭按钮
  与系统关闭事件(Alt+F4 兜底)同走"关闭到托盘"路径;无边框窗口边缘缩放
  依赖 winit Windows 端 hit-test,勿重复实现
- 托盘:tray-icon 在 eframe 主线程创建(eframe setup 时),由 winit 消息循环代泵;
  TrayIcon 必须保活(存于 App 内),drop 即移除图标
- 托盘事件:muda/tray-icon 的 set_event_handler 把命令推入 std::sync::mpsc 通道,
  并调用 ctx.request_repaint() 唤醒隐藏状态下的主循环
- 关闭到托盘:实现 App::on_close_event,非退出态时 Visible(false) 隐藏窗口并返回 false;
  仅退出标志置位时放行关闭
- 扩展备忘(跨平台时启用,源自同类项目实战):
  - tray-icon 0.25 的 default features 在 Linux 引入 libappindicator/muda-gtk3/libxdo,
    Windows 无影响;扩展 Linux 时评估 default-features = false + 显式选择后端
    (ksni 或 libappindicator),libxdo 仅服务于未使用的菜单 accelerator
  - Linux 托盘无双击事件(appindicator 限制);Wayland 下托盘与全局坐标能力受限
  - macOS 侧 tray-icon 正迁移 objc2,扩展时核对版本
  - 届时平台专属调用必须收敛到独立 platform 模块,依赖放 [target.'cfg(...)'.dependencies],
    UI 层禁止直接引用平台 crate

## 开发规范

### 1. 代码风格
- 使用 Rust 标准代码风格 (rustfmt),4 空格缩进
- 遵循 Rust 命名约定 (snake_case 函数/变量,PascalCase 类型/trait)
- 公共 API 包含文档注释;注释说明约束与意图,不复述代码,不使用 emoji

### 2. 文件组织
- 代码文件尽量不超过 300 行;超过且逻辑易拆时拆分,承担多职责时必须拆分
- 每个文件职责单一,便于后续用 AI 工具读取和迭代

### 3. 样式常量
- 颜色统一在 theme.rs 的 Palette 调色板字段定义(深浅两套),绘制代码经
  theme::c() 取色,禁止散落硬编码色值,新增色前先查重
- 唯一强调色: 深色主题 #5C9DFF(hover/active 派生),禁止散落蓝色硬编码;
  语义色(入站/出站等)在调色板内按主题分别定义;accent 上的文字用
  on_accent,悬停/展开底色用 hover_bg/open_bg(全部走调色板)
- 圆角刻度: RADIUS_SM=4 / RADIUS_MD=8 / RADIUS_LG=12 / RADIUS_PILL=胶囊
  (u8::MAX 渲染 clamp 为半圆),不出现圆角魔法数字
- 字号刻度 theme::font(H1/H2/H3/BODY/SM/XS/MICRO)与间距刻度 theme::sp
  (XS/SM/MD/LG/XL):页面内禁止散布字号/间距魔法数字
- 阴影: theme::window_shadow()/popup_shadow() 按主题取用,仅用于浮起层
  (窗口/弹层/悬浮卡),平面内容不加投影
- 可交互控件样式必须区分 hovered / active / disabled 状态;按钮选中态
  优先用 egui Button::selected(走 Visuals selection),自绘态经 painter
  分状态绘制
- 公共组件优先用 ui/widgets/(页头/徽章/分段选择/开关/表格件/卡片),
  新页面禁止重复实现同类小件

### 4. 多语言(i18n)
- 基于 fluent-bundle:全部 UI 文本经 I18n::t()/t_with_args() 获取,
  禁止在绘制代码里散落字符串字面量(品牌名等与语言无关的常量除外)
- 词条在 locales/<lang>.ftl,每个文件必须含 lang-name;新增文案必须同步补全
  zh-cn 与 en 两种语言
- FTL 变量用 {$name} 插值,代码用 t_with_args(key, &[("name", value)]),
  禁止 .replace() 手动替换;{name} 是消息引用而非变量
- 词条查找顺序:当前语言 → 默认语言(zh-cn)→ 返回键名并告警(相同键只告警一次)
- 数据类显示名(如城市)以数据键派生词条键(city-<key>);新增数据项必须补词条
- 新增语言:按现有模板翻译后放入 locales/ 目录即被自动发现(设置页进入时重扫);
  NETOWL_LANG 环境变量可覆盖初始语言

### 5. 错误处理
- 使用 Result 与有意义的错误信息;文件/系统 API 操作必须处理错误
- 不做静默 fallback:失败直接暴露,不用默认值掩盖(如字体加载失败应显式报错)

### 6. 代码复用
- 相同/相似逻辑必须提取公共函数;仅单文件使用的私有化在本文件,
  跨文件复用的上提到对应公共模块
- 功能相同仅参数不同的函数合并为参数化通用函数
- update/绘制主流程保持简洁,复杂细节委托辅助函数,提交前自查重复代码

### 7. 日志(引入日志库后生效)
- 统一前缀格式: [模块名] [标识] 消息内容,必须包含操作对象,
  禁止"请求成功"式无信息日志
- 第三方库日志收敛到 warn;应用自身档位由 RUST_LOG 控制
- 级别语义(排查时按 debug → trace 逐级展开,默认 info 保持少量):
  - error:最终失败,功能不可用且无恢复
  - warn:可恢复失败的事实一句话(细节不进 warn,避免双记)
  - info:状态迁移与用户可见动作的结果
  - debug:操作流转与失败细节(机制选择、外部命令行与退出码、注册表
    操作结果、分支决策、配置写盘、设置项变更)
  - trace:高频细节与帧事件(页面切换、重绘节奏变化、每 5s 帧耗时统计)
- 渲染路径禁止每帧逐条日志(egui 30~60fps 会造成日志爆炸);帧级信息
  一律走事件型(节奏变化)或周期汇总(帧统计)

### 8. 依赖管理
- 新依赖一律最新稳定版;与依赖树冲突时保持生态一致版本并在 Cargo.toml 注释说明
- 每次改动 Cargo.toml 后核对 Cargo.lock 关键包版本未被降级
- 重要依赖在 Cargo.toml 注释中写明选型理由
- 最小化依赖: 标准库可覆盖的能力(std mpsc、简单 PRNG 等)不引入第三方

### 9. 图标
- 应用图标源文件位于 assets/:窗口图标 app_icon.png(256x256)、托盘图标
  tray_icon.png(64x64),经 include_bytes! 编译期内嵌,发布程序不携带 assets 目录
- app_icon.ico 保留供打包阶段编译进 exe 资源段(文件管理器/快捷方式/安装程序),
  届时以多尺寸层(16/32/48/256)替换现有单层版本
- 图标必须为 8-bit RGBA PNG,解码失败/格式不符时显式报错,不做静默降级
- UI 内禁止 emoji 与 unicode 符号充当图标;assets/icons.ttf(Bootstrap Icons)已就位,
  接入 UI 时码点必须对照字体 cmap 或官方码点表验证,
  注释写真实 glyph 名(注释即契约)
- 底图数据 assets/mapdata.bin 由 tools/build_mapdata.py 生成后入库,
  混合数据源:v5 格式(magic "NWLD" + version=5)
  - 世界国界/海洋:Natural Earth admin_0_countries + geography_marine_polys
    (公有领域);中国(CHN/HKG/MAC/TWN)NE feature 几何与标签剔除
  - 中国行政区划:阿里云 DataV GeoAtlas areas_v3/bound/100000_full.json
    (免 key 静态 GeoJSON,基于天地图,GCJ-02),含 34 省级 geometry、
    100000_JD 十段线;分层渲染(世界层在前、中国层在后),NE 中越界
    的邻国国界由中国层覆盖;邻国界顶点向中国边界温和吸合(<=0.2 度)
    消细缝,远离边界者不动
  - 湖泊:110m 档取 ne_50m_lakes 面积 >=0.2 平方度大湖;50m 档取
    ne_10m_lakes 面积 >=0.05 平方度(补入鄱阳湖、洞庭湖、纳木错、
    色林错、巢湖等)并固定容差 DP 预抽稀,不参与世界层预算二分;
    境内湖洞按 pip 判定归入中国层(否则被中国层陆地填充盖住,
    境内湖泊不可见),境外湖(贝加尔、五大湖等)留世界层;咸海一律
    取历史层整体轮廓
  - 河流:ne_10m_rivers_lake_centerlines 仅 River 类(排除 Lake
    Centerline 避免画进湖面),scalerank<=3(全局档)/<=4(精细档)
    过滤全球主要河流,固定容差 DP 预抽稀;独立折线节存储与渲染
    (世界/中国层之上),不参与环三角化与共享边分类;NE 长江口
    江阴以东整体缺段,由脚本内手工补线(YANGTZE_MOUTH_PATCH,沿
    南岸主槽衔接 NE 端点至吴淞口外入海)
  量化 0.001 度 + varint delta,几何两档;50m 档经共享边感知的
  Douglas-Peucker 抽稀(相邻国家共享段顶点序列一致且 DP 方向对称,
  边界无缝);南极洲几何与标签剔除,底图窗口纬度 [-58, 84];
  港澳台为省级行政区正常显示省名(kind=2,样式弱于国家名),台湾
  NAME_ZH 问题随 NE 中国数据剔除从根本上消除;重生成需下载
  ne_110m/50m_admin_0_countries、ne_50m_geography_marine_polys、
  ne_50m_lakes、ne_10m_lakes、ne_50m_lakes_historic、
  ne_10m_rivers_lake_centerlines 与 100000_full.json 到 tools/cache/
- 地名标签(国家 238 + 海洋 111 + 省 34,共 383 条)内嵌于 mapdata.bin
  标签段,不走 fluent 词条(数量庞大);南海十段线独立线段节;
  河流折线两档独立节;城市(含港澳台市)仍走 city-<key> 词条

### 10. 持久化
- 数据根 = exe 同级目录(Windows 便携式),启动时切换工作目录;
  config.toml 与 data/ 均为相对路径(paths.rs)
- 配置:serde TOML;变更只更新内存,由 App 层 500ms 防抖合并写盘,
  写盘走 临时文件 -> .bak 备份 -> rename;托盘"退出"时立即落盘
- 窗口几何存物理像素(i32::MIN 表示未设置居中);最小化污染坐标(Windows 移窗到
  -32000)不落盘;恢复在首帧按当前 pixels_per_point 换算下发 ViewportCommand
  (egui 坐标全为逻辑 points,物理除以当前 ppp 后精确还原)
- SQLite:data/netowl.db(rusqlite bundled 免系统依赖,WAL);结构迁移以
  schema_version 表记录版本;所有含外部输入的查询必须参数绑定(rusqlite `params!`),
  禁止字符串拼接/format 组装 SQL

## 开发流程规范

### 功能点与 git 提交
- 每完成一个单独的功能点,创建一个 git commit 提交
- commit 前置条件(三者都必须满足):
  - `cargo fmt` 已执行,项目代码符合 rustfmt 标准格式(每次提交都保持
    格式化到位,避免后续触发 fmt 时产生大量与功能无关的重排)
  - `cargo build` 正常编译通过
  - `cargo clippy` 无任何警告
- commit message 首行 = 提交分类 + 中文简述,如 "feat: xxx" / "fix: xxx" /
  "chore: xxx"(feat 新功能、fix 修复、chore 构建/工具/杂务,其他场景按需
  使用 refactor/docs/style/test);禁止 emoji
- 提交内容仅包含该功能点相关文件;临时验证产物(截图、日志等)不入库,
  收尾删除

### TODO 管理
- 用户提出多功能开发需求时,先将功能点清单写入 TODO 文件(TODO.md),
  每项含简要说明与验收要点
- 每完成一个功能点:在 TODO 文件中勾掉对应项,并为该功能点做一次 git commit
- TODO.md 命中用户全局 gitignore,为本地工作文件,不提交入库,README 也不引用

### 打包
- cargo packager --release(配置在 Cargo.toml [package.metadata.packager]):
  Windows 产出 NSIS 安装程序 target/release/netowl_<version>_x64-setup.exe
- NSIS 用自定义模板 packaging/nsis/installer.nsi(拷自 cargo-packager 0.11.8
  默认模板):向导新增 nsDialogs"附加选项"页(开始菜单页后),勾选"开机自
  启动"则完成后先杀残留实例再以 --autostart-on 启动应用(单实例守卫会吞掉
  参数),由 NetOwlApp 置 config.general.autostart 落注册表(安装器不直接写
  Run 键);启动必须走 ExecShell(Exec=CreateProcess 对 highestAvailable
  manifest 报 740 静默失败);卸载时清 Run 键值;自绘 Finish 页复选框
  (创建成功但不显示)与 Reinstall 页 PRE-Abort 均实测不可行,Reinstall 检测
  页经 !if 0 禁用;升 cargo-packager 需与上游模板 diff 同步
- category 合法值为 LSApplicationCategory 风格枚举(GraphicsAndDesign/
  Utilities 等,无 Network);identifier com.zephyr.netowl 为占位域名,可再改
- 发布产物不携带 assets(图标/底图编译期内嵌),locales 词条外置,
  目录缺失时运行时用内嵌兜底

## 验证规范
- 每次改动 cargo build 通过;界面改动 cargo run 冒烟确认无 panic
- GUI 无法自动断言的视觉项,说明验证方式并交用户确认
