# NetOwl English translations
lang-name = English

app-name = NetOwl
app-subtitle = Network Monitor

nav-map = Traffic Map
nav-connections = Connections
nav-lan = LAN
nav-history = History
nav-rules = Rules
nav-settings = Settings

status-monitoring = Monitoring
status-mock = Mock data
status-conn-count = { $count } connections

nav-rate-down = Down
nav-rate-up = Up
nav-session-total = Session

map-title = Traffic Map
map-subtitle = Live network connections visualization
map-legend-help = Node color guide
map-legend-dir = Flow direction
map-legend-latency = Node latency
map-legend-out = Outbound
map-legend-in = Inbound
map-legend-good = Good <100ms
map-legend-warn = Slow 100-300ms
map-legend-bad = Poor >300ms
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
map-unblock-silent = Silent-deny fallback active; toggling off creates an allow rule
map-unblock-target = Unblock: remove the target-level block rule
map-blocked-by-process = Already blocked by a process-level rule
map-inspector-summary = Summary
map-inspector-place = Endpoint details
map-inspector-process = Process details
map-inspector-clear = Clear selection
map-inspector-procs = Related processes
map-inspector-isp = ISP: {$isp}
map-inspector-latency = Latency {$rtt}ms · loss {$loss}%
map-inspector-conns = Connection details
map-inspector-path = Path
map-inspector-top-proc = Top Processes
map-inspector-top-domain = Top Domains
map-rank-total = Total
map-rank-up = Up
map-rank-down = Down
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
conns-no-match = No matching connections; adjust the search or display filters
conns-search = Search process / remote / domain
conn-loc-unknown = Unknown
conn-proc-unknown = Unknown process
conn-action-allow = Allow
conn-action-block = Block
menu-kill = Close connection
menu-kill-need-admin = Run as administrator to close connections
menu-kill-v6 = Killing IPv6 connections is not supported
menu-locate = Reveal program
menu-copy-remote = Copy remote address
menu-copy-listen = Copy listening address
menu-copy-selected = Copy selected text
menu-copy-path = Copy process path
menu-copy-cell = Copy cell content
history-menu-delete-row = Delete this record
history-menu-delete-group = Delete this group
history-menu-delete-process = Delete all records of this process
history-delete-confirm-title = Confirm Deletion
history-delete-process-text = All history records of {$process} will be deleted. This cannot be undone.
history-delete-group-text = All history records of {$process} to {$remote} will be deleted. This cannot be undone.
history-delete-confirm = Delete
history-delete-cancel = Cancel

proc-signed = Signed
proc-unsigned = Unsigned
proc-sign-invalid = Signature invalid
proc-sign-unknown = Signature unknown
proc-path-unknown = Path unavailable

filter-include-local = Include local traffic
filter-include-lan = Include LAN traffic

settings-history-days = Auto-clean history
settings-history-days-unit = days
settings-history-days-hint = Delete history older than this many days; "-" disables auto-clean

settings-ask = New connection prompts
settings-silent = Global silent mode
settings-silent-hint = Silently allow or deny connections that match no rule without prompting; turning it off restores the "ask for new connections" behavior
settings-silent-off = Off
settings-silent-allow = Silent allow
settings-silent-deny = Silent deny
settings-ask-on = Ask for new connections that match no rule
settings-ask-hint = Public targets only; local and LAN connections never prompt. "Always" options are saved to the Rules page

settings-tray-pin = Pin tray icon
settings-tray-pin-on = Keep the tray icon on the taskbar instead of the hidden overflow
settings-tray-pin-hint = Writes the system tray setting; takes effect next session, falls back to system default on failure

settings-autostart = Launch at login
settings-autostart-hint = Run in the background after you sign in without showing the main window
settings-hotkey = Show main window
settings-hotkey-hint = Summons the main window to the traffic map. Click the field to record; Esc cancels
settings-hotkey-off = Off
settings-hotkey-none = Not set
settings-hotkey-capturing = Press a combo…
hotkey-fail-toast = Failed to register hotkey {$combo} ({$reason})
hotkey-reason-invalid = unrecognized combo
hotkey-reason-taken = combo already taken by another program
hotkey-reason-other = error code {$code}

settings-floating-ball = Floating widget
settings-floating-ball-hint = Dock to the screen edge with live rates; hover for per-process traffic

ball-realtime-up = Live upload
ball-realtime-down = Live download
ball-total-today = Today
ball-detail = View details
ball-no-etw = Running without elevation, per-process rates unavailable
ball-empty = No active traffic
ball-menu-show = Show Window
ball-menu-topmost = Keep on Top
ball-menu-auto-hide = Auto-hide at Edge
ball-menu-log = Open Live Log
ball-menu-settings = Settings
ball-menu-close = Close Floating Widget

settings-log = Logging
settings-log-level = Log level
settings-log-retain = Retain log files
settings-log-retain-hint = Auto-delete rotated log archives older than this many days (latest.log is kept); "-" disables cleanup
settings-log-retain-unit = days
settings-log-level-hint = Applies immediately to console and file; the log window has its own shown level
settings-log-file-on = Write to log file
settings-log-file-hint = Write to the logs directory next to the exe, old files rotate by timestamp
settings-log-view = View logs
settings-log-view-hint = Locate the log file in Explorer, or open the browser window for live logs
settings-log-locate = Locate log file
settings-log-open = Open window

settings-section-general = General
settings-section-monitoring = Monitoring
settings-section-logging = Logging
settings-section-about = About
about-version = Current version
about-version-hint = The version of NetOwl currently running
about-channel = Update channel
about-channel-hint = Stable includes releases only; Preview includes pre-releases for early access
channel-stable = Stable
channel-preview = Preview
about-check = Check for updates
about-check-hint = Fetch the latest release info from GitHub Releases
about-check-btn = Check for updates
about-checking = Checking…
about-latest = You are up to date
about-new-version = New version available: {$ver}
about-no-release = No releases published yet
about-failed = Check failed: {$err}
about-download = Download
about-github = Project homepage
about-github-hint = github.com/zsyo/NetOwl
about-github-btn = Homepage
about-feedback-btn = Feedback

log-level-off = Off
log-level-error = Error
log-level-warn = Warning
log-level-info = Info
log-level-debug = Debug
log-level-trace = Trace

log-window-level = Shown level
log-window-autoscroll = Auto scroll
log-window-pin = Always on top
log-window-filter = Filter
log-window-filter-placeholder = Keyword filter (case-insensitive)
log-window-clear = Clear
log-window-empty = No logs yet
log-window-no-match = No matching logs

history-title = Connection History
history-subtitle = Records of completed connections (written on close)
history-view-detail = Detail
history-view-aggregate = Aggregate
lan-title = LAN Devices
lan-subtitle = Neighbors discovered via ARP on this network (no admin required)
lan-col-ip = IP Address
lan-col-mac = MAC Address
lan-col-last = Last Seen
lan-col-status = Status
lan-online = Online
lan-offline = Offline
lan-new = New
lan-empty = No LAN devices discovered yet; they appear after traffic
lan-count = {$total} devices, {$online} online
lan-copy-ip = Copy IP address
lan-copy-mac = Copy MAC address
history-view-usage = Usage
history-view-summary = Summary
history-summary-note = Completed connections + live bytes of currently active ones
history-summary-drill = Click to show this process's connections in Detail view
history-usage-daily = Daily
history-usage-hourly = Hourly
history-usage-no-etw = Not running as administrator: byte totals are unavailable (connection records are unaffected)
history-col-bucket = Time
history-range-1h = Last hour
history-range-6h = Last 6 hours
history-range-24h = Last 24 hours
history-range-7d = Last 7 days
history-range-month = This month
history-filter-process = Process…
filter-noise = Filter
history-filter-remote = Remote IP…
filter-proto-all = All protocols
history-refresh = Refresh
history-export = Export CSV
history-loading = Loading…
history-export-ok = Exported to {$path}
history-export-failed = Export failed: {$error}
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

rules-manage = Manage
rules-profile-manage = Manage Profiles
rules-profile-name-hint = Profile name
rules-profile-new = Create
rules-profile-save = Save
rules-profile-cancel = Cancel
rules-profile-current = current
rules-profile-rule-count = {$n} rules
rules-profile-copy-name = {$name} - copy
rules-profile-delete-text = Delete profile "{$name}" and all of its {$n} rules; this cannot be undone
rules-profile-delete-confirm = Delete
rules-profile-op-done = Operation succeeded
rules-profile-op-failed = Operation failed: {$detail}
rules-profile-default = Default
rules-title = Rules
rules-subtitle = Rules are evaluated top-down by priority; the first enabled match decides the action, unmatched connections are allowed
ask-title = New Connection
ask-question = A program wants to connect to
ask-timeout-hint = Auto-deny in {$n} s
ask-rule-save-failed = Failed to save the rule, this decision has no effect: {$process}
ask-always-hint = "Always" options are saved to the Rules page; the connection stays blocked while asking
ask-allow = Allow
ask-deny = Deny
ask-scope = Scope
ask-scope-once = This time only
ask-scope-target = Always · this target
ask-scope-process = Always · whole program

tray-show = Show Main Window
tray-quit = Quit
tray-silent = Silent Mode
tray-log = Open Live Log
tray-tooltip = NetOwl Network Monitor
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
rules-col-hits = Hits
rules-col-hits-tip = Cumulative matched connections (counted once per connection, survives restarts)
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
rules-remote-ip-hint = e.g. 10.0.0.0/8, 142.250., 1.2.3.4 or 2001:db8::/32
rules-remote-domain-hint = Domain suffix, e.g. example.com
rules-remote-domain-desc = Enforced in-kernel via resolved IPs after save; resolution refreshes about every 10 minutes
rules-port-any-hint = - = any port
rules-err-name = Name must not be empty
rules-err-remote = Cannot parse the IP range
rules-err-remote-domain = Invalid domain format: letters, digits, hyphens and dots only, e.g. example.com
rules-err-save = Save failed; check the log

settings-title = Settings
settings-language = Language
settings-language-hint = Applies immediately; drop .ftl files into the locales directory to add languages
settings-font = Interface font
settings-font-hint = One font for both Latin and Chinese text; Chinese falls back to Microsoft YaHei when the chosen font has no Han glyphs. Fonts installed while running appear after a restart
font-default = Default (Microsoft YaHei)
font-search = Search fonts...
settings-count-format = Number format
settings-count-format-hint = Units for large numbers; Auto follows the UI language
count-format-auto = Auto
count-format-wan = 万·亿
count-format-kmb = K·M·B
settings-theme = Interface theme
settings-theme-hint = Dark and light apply immediately and are saved to the config file
theme-dark = Dark
theme-light = Light
settings-datasource = Data source
datasource-real = Live collection
datasource-mock = Mock
settings-datasource-hint = Mock data is for demo and testing only; switching applies immediately and is saved to the config file

# Common format suffixes
pid-suffix = (PID {$pid})
conns-count-suffix = ({$count})

# New device notification
lan-notify-toast = New device joined: {$ip}
settings-lan-notify = New device alert
settings-lan-notify-hint = Show a toast when a new device joins the LAN
listen-notify-toast = New listening port: {$process} ({$endpoint})
toast-copied = Copied to clipboard
toast-font-missing = Font {$name} is not in the system font list; kept the current font
toast-font-failed = Failed to switch the interface font: {$error}
settings-listen-notify = New listening port alert
settings-listen-notify-hint = Show a toast when a program opens a new listening port
settings-quality = Network quality probing
settings-quality-hint = ICMP-probe public remotes of active connections; map nodes are colored by latency

# Listening ports view
conns-view-conns = Connections
conns-view-listen = Listening
listen-col-local = Local address:port
listen-col-pid = PID
listen-empty = No listening ports found

# Connections grouping
conns-group = Group by process
conns-group-count = {$count} conns

# Usage quota alerts
quota-warn-toast = Monthly usage reached 80% ({$used})
quota-exceed-toast = Monthly usage exceeded the quota ({$used})
settings-quality-targets = Custom probe targets
settings-quality-targets-hint = Add addresses to monitor continuously (e.g. game servers), probed even without active connections, results shown live on each row; active-connection remotes are covered automatically. Port - = no TCP fallback when ping is blocked; label shows in alerts
settings-quality-label-hint = Label
settings-quality-add = Add target
settings-quality-remove = Remove
settings-quality-invalid-ip = Invalid IP format
settings-quality-sample = {$rtt}ms · {$loss}% loss
settings-quality-wait = Pending
settings-quality-noreply = No reply
quality-alert-toast = Custom target "{$name}" unresponsive for {$n} rounds
settings-quota = Monthly data quota
settings-quota-hint = Alert at 80% and 100% of the quota; "-" = disabled
settings-quota-unit = GB

listen-search = Search process or port

menu-view-history = View history
inspector-open-conns = View in connections
inspector-open-history = View history
