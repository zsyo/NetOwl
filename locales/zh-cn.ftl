# NetOwl 简体中文词条
lang-name = 简体中文

app-name = NetOwl
app-subtitle = Network Monitor

nav-map = 流量地图
nav-connections = 连接
nav-history = 历史
nav-rules = 规则
nav-settings = 设置

status-monitoring = 监控中
status-mock = 模拟数据
status-conn-count = { $count } 条连接

nav-rate-down = 下载
nav-rate-up = 上传

map-title = 流量地图
map-subtitle = 实时网络连接可视化
map-legend-out = 出站
map-legend-in = 入站
map-local = 本机
map-info-more = +{ $n } 个连接

city-local = 本机
city-shanghai = 上海
city-tokyo = 东京
city-seoul = 首尔
city-hongkong = 香港
city-macau = 澳门
city-taipei = 台北
city-singapore = 新加坡
city-mumbai = 孟买
city-moscow = 莫斯科
city-frankfurt = 法兰克福
city-amsterdam = 阿姆斯特丹
city-london = 伦敦
city-newyork = 纽约
city-sanjose = 圣何塞
city-sydney = 悉尼
city-saopaulo = 圣保罗

conns-title = 连接
conn-total-bytes = 累计字节
conns-subtitle = 当前活跃的网络连接
col-process = 进程
col-proto = 协议
col-remote = 远端地址
col-location = 位置
col-down = 下载
col-down-total = 下载总量
col-up-total = 上传总量
col-up = 上传
col-port = 端口
col-action = 动作
conns-empty = 暂无活跃连接
conn-loc-unknown = 未知归属
conn-proc-unknown = 未知进程
conn-action-allow = 允许
conn-action-block = 阻断

proc-signed = 已签名
proc-unsigned = 未签名
proc-sign-invalid = 签名未通过
proc-sign-unknown = 签名未知
proc-path-unknown = 无法读取路径

filter-hide-local = 隐藏本地
filter-hide-lan = 隐藏局域网

settings-history-days = 历史自动清理
settings-history-days-unit = 天
settings-history-days-hint = 自动删除超过该天数的历史数据,0 表示不自动清理

settings-ask = 新连接询问
settings-ask-on = 为未命中规则的新连接弹窗询问
settings-ask-hint = 仅询问公网目标,本地与局域网连接不弹;永久选项写入规则页,可随时修改

settings-tray-pin = 托盘图标常驻
settings-tray-pin-on = 将托盘图标固定在任务栏,不折叠进隐藏区域
settings-tray-pin-hint = 写入系统托盘设置,新会话生效;失败时保持系统默认行为

ask-title = 新连接
ask-question = {$process} 要连接到
ask-timeout-hint = {$n} 秒后自动拒绝
ask-always-hint = 永久选项写入规则页,可随时修改;询问期间该连接保持阻断
ask-allow = 允许
ask-deny = 拒绝
ask-scope = 生效范围
ask-scope-once = 仅本次
ask-scope-target = 永久·仅此目标
ask-scope-process = 永久·整个程序

history-title = 连接历史
history-subtitle = 已完结连接的记录(连接关闭时落盘)
history-view-detail = 明细
history-view-aggregate = 聚合
history-range-1h = 最近 1 小时
history-range-6h = 最近 6 小时
history-range-24h = 最近 24 小时
history-range-7d = 最近 7 天
history-range-month = 本月
history-filter-process = 进程名…
history-filter-remote = 远端 IP…
history-filter-proto-all = 全部协议
history-refresh = 刷新
history-db-size = 历史库 {$size}
history-purge = 清理
history-purge-all = 清空全部
history-purge-days = 清理 {$n} 天前数据
history-purge-month = 清理一月前数据
history-remind-text = 历史数据库已达 {$size},建议清理不用的历史数据
history-remind-dismiss = 不再提醒
history-col-process = 进程
history-col-first = 出现
history-col-duration = 持续
history-col-count = 次数
history-col-total = 累计时长
history-col-last = 最近活动
history-empty = 所选范围内没有历史记录
history-truncated = 已达单次查询上限 {$n} 条,请缩小时间范围或增加筛选

rules-title = 规则
rules-subtitle = 规则按优先级自上而下评估,首个命中的启用规则决定动作,未命中默认放行
wfp-status-active = 拦截引擎已生效,当前 {$n} 条过滤器
wfp-status-noadmin = 未以管理员身份运行:规则仅标注连接,不实际拦截;以管理员运行后自动生效
wfp-status-failed = 拦截引擎启动失败:{$err}
wfp-status-off = 拦截引擎未就绪
wfp-active-hint = 进程规则在目标进程出现连接后自动生效;域名规则仅标注不拦截
rules-new = 新建规则
rules-col-enabled = 启用
rules-col-name = 名称
rules-col-action = 动作
rules-col-direction = 方向
rules-col-remote = 远端
rules-col-ops = 操作
rules-empty = 暂无规则,点击"新建规则"添加
rules-new-title = 新建规则
rules-edit-title = 编辑规则
rules-move-up = 上移
rules-move-down = 下移
rules-edit = 编辑
rules-delete = 删除
rules-save = 保存
rules-cancel = 取消
rules-export = 导出
rules-import = 导入
rules-temp-badge = 临时
rules-export-done = 已导出 {$n} 条规则
rules-import-done = 已导入 {$n} 条规则
rules-export-failed = 导出失败: {$err}
rules-import-failed = 导入失败: {$err}
rule-action-allow = 允许
rule-action-block = 阻断
rule-direction-any = 任意方向
rule-direction-out = 出站
rule-direction-in = 入站
rule-proto-any = 任意协议
rule-remote-any = 任意远端
rule-remote-ip = IP 网段
rule-remote-domain = 域名
rules-process-hint = 映像名或路径结尾,如 chrome.exe;留空匹配任意进程
rules-direction-hint = 当前按远端端口近似判定,>=49152 视为入站
rules-remote-ip-hint = 如 10.0.0.0/8、142.250. 或 1.2.3.4
rules-remote-domain-hint = 域名后缀,如 example.com
rules-port-any-hint = 0 = 任意端口
rules-err-name = 名称不能为空
rules-err-remote = 网段格式无法解析
rules-err-remote-empty = 域名不能为空
rules-err-save = 保存失败,请查看日志

settings-title = 设置
settings-language = 语言
settings-language-hint = 界面语言立即生效;将 .ftl 词条文件放入 locales 目录即可新增语言
settings-theme = 界面主题
settings-theme-hint = 深色与浅色立即生效,并同步保存到配置文件
theme-dark = 深色
theme-light = 浅色
settings-datasource = 数据源
datasource-real = 真实采集
datasource-mock = 模拟演示
settings-datasource-hint = 模拟数据仅供演示与测试,切换立即生效并保存到配置文件
