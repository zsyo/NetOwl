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

nav-rate-down = Down
nav-rate-up = Up

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
col-port = Port
col-action = Action
conns-empty = No active connections
conn-loc-unknown = Unknown
conn-proc-unknown = Unknown process
conn-action-allow = Allow
conn-action-block = Block

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

settings-ask = New connection prompts
settings-ask-on = Ask for new connections that match no rule
settings-ask-hint = Public targets only; local and LAN connections never prompt. "Always" options are saved to the Rules page

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
rules-subtitle = Rules are evaluated top-down by priority; the first enabled match decides the action, unmatched connections are allowed
ask-title = New Connection
ask-question = {$process} wants to connect to
ask-timeout-hint = Auto-deny in {$n} s
ask-always-hint = "Always" options are saved to the Rules page; the connection stays blocked while asking
ask-allow = Allow
ask-deny = Deny
ask-scope = Scope
ask-scope-once = This time only
ask-scope-target = Always · this target
ask-scope-process = Always · whole program
wfp-status-active = Enforcement active: {$n} WFP filters
wfp-status-noadmin = Not running as administrator: rules only annotate connections and do not block; restart as admin to enforce
wfp-status-failed = Enforcement engine failed to start: {$err}
wfp-status-off = Enforcement engine not ready
wfp-active-hint = Process rules take effect once the target process shows up; domain rules annotate only
rules-new = New Rule
rules-col-enabled = On
rules-col-name = Name
rules-col-action = Action
rules-col-direction = Direction
rules-col-remote = Remote
rules-col-ops = Ops
rules-empty = No rules yet; click "New Rule" to add one
rules-new-title = New Rule
rules-edit-title = Edit Rule
rules-move-up = Up
rules-move-down = Down
rules-edit = Edit
rules-delete = Delete
rules-save = Save
rules-cancel = Cancel
rule-action-allow = Allow
rule-action-block = Block
rule-direction-any = Any direction
rule-direction-out = Outbound
rule-direction-in = Inbound
rule-proto-any = Any protocol
rule-remote-any = Any remote
rule-remote-ip = IP range
rule-remote-domain = Domain
rules-process-hint = Image name or path suffix, e.g. chrome.exe; empty matches any process
rules-direction-hint = Approximated by remote port for now; >=49152 counts as inbound
rules-remote-ip-hint = e.g. 10.0.0.0/8, 142.250. or 1.2.3.4
rules-remote-domain-hint = Domain suffix, e.g. example.com
rules-port-any-hint = 0 = any port
rules-err-name = Name must not be empty
rules-err-remote = Cannot parse the IP range
rules-err-remote-empty = Domain must not be empty
rules-err-save = Save failed; check the log

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
