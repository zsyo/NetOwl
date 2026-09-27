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

nav-rate-down = 下载 {$rate}
nav-rate-up = 上传 {$rate}

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
conns-subtitle = 当前活跃的网络连接
col-process = 进程
col-proto = 协议
col-remote = 远端地址
col-location = 位置
col-down = 下载
col-up = 上传
conns-empty = 暂无活跃连接
conn-loc-unknown = 未知归属
conn-proc-unknown = 未知进程

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
rules-placeholder = 规则引擎将在后续里程碑接入:允许/拒绝策略、进程与网段匹配、持久化。

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
