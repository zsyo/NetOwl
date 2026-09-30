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
map-panel-toggle-list = Connection list panel
map-panel-toggle-inspector = Inspector panel
map-panel-search = Search processes or targets
map-panel-filter = Showing only: {$place}
map-panel-empty = No matching connections
map-block-process = Block all connections of this process
map-unblock-process = Unblock: remove the process-level block rule
map-block-target = Block this process to this target
map-unblock-target = Unblock: remove the target-level block rule
map-blocked-by-process = Already blocked by a process-level rule
map-inspector-summary = Summary
map-inspector-place = Endpoint details
map-inspector-process = Process details
map-inspector-clear = Clear selection
map-inspector-procs = Related processes
map-inspector-conns = Connection details
map-inspector-path = Path
map-inspector-top-proc = Top Processes
map-inspector-top-domain = Top Domains
map-inspector-processes = { $count } processes, { $remotes } remotes
map-inspector-empty = Click a map endpoint or a process on the left to see details

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
conn-total-bytes = Total bytes
conns-subtitle = Currently active network connections
col-process = Process
col-proto = Protocol
col-remote = Remote address
col-location = Location
col-down = Down
col-up = Up
col-down-total = Total Down
col-up-total = Total Up
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

settings-tray-pin = Pin tray icon
settings-tray-pin-on = Keep the tray icon on the taskbar instead of the hidden overflow
settings-tray-pin-hint = Writes the system tray setting; takes effect next session, falls back to system default on failure

settings-log = Logging
settings-log-level = Log level
settings-log-file-on = Write to log file
settings-log-view = View logs
settings-log-hint = Level applies immediately; file logs go to the logs directory next to the exe with rotation; "View logs" opens the log browser

settings-section-appearance = Appearance
settings-section-monitoring = Monitoring
settings-section-logging = Logging

log-level-off = Off
log-level-error = Error
log-level-warn = Warning
log-level-info = Info
log-level-debug = Debug
log-level-trace = Trace

log-window-level = Shown level
log-window-autoscroll = Auto scroll
log-window-filter = Filter
log-window-filter-placeholder = Keyword filter (case-insensitive)
log-window-clear = Clear
log-window-empty = No logs yet
log-window-no-match = No matching logs

history-title = Connection History
history-subtitle = Records of completed connections (written on close)
history-view-detail = Detail
history-view-aggregate = Aggregate
history-view-summary = Summary
history-summary-note = Completed connections only; active ones count when closed
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
rules-export = Export
rules-import = Import
rules-temp-badge = temp
rules-export-done = Exported {$n} rules
rules-import-done = { $skipped ->
    [0] Imported {$n} rules
   *[other] Imported {$n} rules, skipped {$skipped} duplicates
}
rules-export-failed = Export failed: {$err}
rules-import-failed = Import failed: {$err}
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
