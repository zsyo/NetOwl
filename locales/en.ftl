# NetOwl English translations
lang-name = English

app-name = NetOwl
app-subtitle = Network Monitor

nav-map = Traffic Map
nav-connections = Connections
nav-history = History
nav-rules = Rules
nav-settings = Settings

status-monitoring = Monitoring
status-mock = Mock data
status-conn-count = { $count } connections

nav-rate-down = Down {$rate}
nav-rate-up = Up {$rate}

map-title = Traffic Map
map-subtitle = Live network connections visualization
map-legend-out = Outbound
map-legend-in = Inbound
map-local = Local
map-info-more = +{ $n } more

city-local = Local
city-shanghai = Shanghai
city-tokyo = Tokyo
city-seoul = Seoul
city-hongkong = Hong Kong
city-macau = Macau
city-taipei = Taipei
city-singapore = Singapore
city-mumbai = Mumbai
city-moscow = Moscow
city-frankfurt = Frankfurt
city-amsterdam = Amsterdam
city-london = London
city-newyork = New York
city-sanjose = San Jose
city-sydney = Sydney
city-saopaulo = São Paulo

conns-title = Connections
conns-subtitle = Currently active network connections
col-process = Process
col-proto = Protocol
col-remote = Remote address
col-location = Location
col-down = Down
col-up = Up
conns-empty = No active connections
conn-loc-unknown = Unknown
conn-proc-unknown = Unknown process

proc-signed = Signed
proc-unsigned = Unsigned
proc-sign-invalid = Signature invalid
proc-sign-unknown = Signature unknown
proc-path-unknown = Path unavailable

filter-hide-local = Hide local
filter-hide-lan = Hide LAN

settings-history-days = Auto-clean history
settings-history-days-unit = days
settings-history-days-hint = Delete history older than this many days; 0 disables auto-clean

history-title = Connection History
history-subtitle = Records of completed connections (written on close)
history-view-detail = Detail
history-view-aggregate = Aggregate
history-range-1h = Last hour
history-range-6h = Last 6 hours
history-range-24h = Last 24 hours
history-range-7d = Last 7 days
history-range-month = This month
history-filter-process = Process…
history-filter-remote = Remote IP…
history-filter-proto-all = All protocols
history-refresh = Refresh
history-db-size = Database {$size}
history-purge = Purge
history-purge-all = Purge all
history-purge-days = Purge data older than {$n} days
history-purge-month = Purge data older than 1 month
history-remind-text = History database has reached {$size}; consider cleaning up old data
history-remind-dismiss = Don't remind again
history-col-process = Process
history-col-first = First seen
history-col-duration = Duration
history-col-count = Connections
history-col-total = Total time
history-col-last = Last active
history-empty = No history records in the selected range
history-truncated = Query limit of {$n} rows reached; narrow the time range or add filters

rules-title = Rules
rules-placeholder = The rule engine arrives in a later milestone: allow/deny policies, process and network matching, persistence.

settings-title = Settings
settings-language = Language
settings-language-hint = Applies immediately; drop .ftl files into the locales directory to add languages
settings-theme = Interface theme
settings-theme-hint = Dark and light apply immediately and are saved to the config file
theme-dark = Dark
theme-light = Light
settings-datasource = Data source
datasource-real = Live collection
datasource-mock = Mock
settings-datasource-hint = Mock data is for demo and testing only; switching applies immediately and is saved to the config file
