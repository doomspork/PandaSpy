### PandaSpy — application shell strings.
###
### This file is consumed by BOTH sides of the app: fluent-rs builds the tray
### menu and OS notifications from it, and @fluent/bundle builds the window UI
### from the very same file. There is no second copy to keep in sync.
###
### en-US is the reference locale. `cargo xtask locale-check` fails if any other
### locale is missing a key defined here (or defines one that is not).

## Branding

# The product name. A proper noun — left as "PandaSpy" in every locale. It is a
# term rather than a message so that translations can inflect the words around
# it without the name itself ever being retyped.
-brand-name = PandaSpy

## Application shell

window-title = { -brand-name }

## Tray menu

tray-show = Show { -brand-name }
tray-quit = Quit

## Window navigation

nav-add-printer = Add printer
nav-settings = Settings
nav-back = Back

## Common

common-cancel = Cancel

## Printer list

printer-list-empty-title = No printers yet
printer-list-empty-body = Add a printer to start watching it from here.
printer-list-empty-cta = Add printer

## Connection status
##
## `status`/`reason` come straight from the client's session state machine
## (`pandaspy-client`); `*[unknown]` covers any value this build does not
## recognise yet, the same "never reject, just show what happened" rule the
## wire parser itself follows.

connection-status = { $status ->
    [disconnected] Disconnected
    [connecting] Connecting…
    [handshaking] Handshaking…
    [connected] Connected
    [failed] Connection failed
   *[unknown] Unknown
}

connection-reason = { $reason ->
    [wrong-access-code] Wrong access code
    [unreachable] Printer unreachable
    [tls] TLS handshake failed
    [certificate-changed] Certificate changed
    [protocol] Unexpected response from printer
    [connection-closed] Connection closed
   *[unknown] Unknown error
}

## Printer card

print-status = { $status ->
    [idle] Idle
    [preparing] Preparing
    [printing] Printing
    [paused] Paused
    [finished] Finished
    [failed] Failed
   *[unknown] Unknown
}

card-remaining = { $time } left
card-progress-percent = { $percent }%
card-layer = Layer { $layer } / { $total }

card-nozzle-temp = { $hasTarget ->
    [yes] Nozzle { $current }° / { $target }°
   *[no] Nozzle { $current }°
}

card-bed-temp = { $hasTarget ->
    [yes] Bed { $current }° / { $target }°
   *[no] Bed { $current }°
}

card-chamber-temp = Chamber { $current }°
card-task-name = Job: { $name }
card-print-error = Print error: { $message }

card-expand = Show details
card-collapse = Hide details
card-remove-label = Remove printer
card-remove-confirm-title = Remove { $name }?
card-remove-confirm-body = PandaSpy will stop watching this printer and forget its stored access code.
card-remove-confirm-confirm = Remove

## AMS

ams-kind = { $kind ->
    [standard] AMS
    [lite] AMS Lite
    [pro2] AMS 2 Pro
    [ht] AMS HT
   *[unknown] AMS
}

ams-humidity = Humidity { $percent }%
ams-temperature = { $temp }°
ams-tray-empty = Empty
ams-tray-remaining = { $percent }% left
ams-active-badge = Feeding

## HMS / health errors

hms-severity = { $severity ->
    [fatal] Fatal
    [serious] Serious
    [common] Warning
    [info] Info
   *[unknown] Notice
}

hms-code-only = Code { $code }
hms-learn-more = Learn more

## Add printer

add-printer-title = Add printer
add-printer-tab-discovered = Discovered
add-printer-tab-manual = Manual
add-printer-tab-studio = Bambu Studio

add-printer-scanning = Scanning…
add-printer-rescan = Scan again
add-printer-already-added = Already added
add-printer-use = Use

# Plural of how many printers a network scan turned up, shown above the
# results list.
discover-found-count = { $count ->
    [0] No printers found
    [one] Found { $count } printer
   *[other] Found { $count } printers
}

# Shown instead of the results list when a scan comes back empty; the text
# depends on why (see `DiscoveryVerdict` in `ipc.ts`). `Found` cannot reach
# this message in practice (it implies at least one result) so it is left to
# fall through to the default text along with anything unrecognised.
discover-verdict-empty = { $verdict ->
    [NoUsableInterface] No usable network interface was found. Check your network connection and try again.
    [PermissionDenied] PandaSpy needs permission to see devices on your local network. Check your system's privacy settings and try again.
    [NoResponse] No printers responded. Make sure the printer is powered on and connected to the same network.
   *[unknown] No printers were found.
}

add-printer-manual-serial = Serial number
add-printer-manual-address = IP address
add-printer-manual-access-code = Access code
add-printer-manual-nickname = Nickname (optional)
# `hasKeyring` is "no" for the brief window before `getSettings()` resolves,
# when the Add screen is reachable but the keyring's platform name isn't
# known yet — falls back to a generic claim instead of an empty `in .`.
add-printer-manual-access-code-hint = { $hasKeyring ->
    [yes] Find this on the printer's screen under Settings → WLAN. PandaSpy stores it in { $keyring }, never in the printer list file.
   *[no] Find this on the printer's screen under Settings → WLAN. PandaSpy stores it securely, never in the printer list file.
}
add-printer-manual-submit = Add printer
add-printer-manual-error-required = Enter a serial number and an access code.
add-printer-manual-error = Couldn't add this printer: { $message }

add-printer-studio-import = Import from Bambu Studio
add-printer-studio-importing = Looking for Bambu Studio printers…
add-printer-studio-empty = No printers found in Bambu Studio yet.
add-printer-studio-use = Use

## Certificate trust prompt

trust-title = This printer's certificate changed
trust-body = The certificate PandaSpy pinned for this printer no longer matches what it just presented. A reflash and an attacker on your network look identical from here — compare the fingerprint below against the one shown on the printer's screen before deciding.
trust-pinned-label = Previously pinned
trust-presented-label = Presented now
trust-accept = It matches — trust it
trust-reject = Keep blocked
trust-error = Couldn't record your decision: { $message }

## Settings

settings-title = Settings
settings-language = Language
settings-language-system = Follow system
settings-launch-at-login = Launch at login

settings-secrets = { $backend ->
    [os-keyring] Access codes are stored in your system's { $keyring }.
    [encrypted-file] No system keyring was available, so access codes are stored in an encrypted file protected by your login.
   *[unknown] Access codes are stored in { $keyring }.
}

settings-save-error = Couldn't save settings: { $message }

## Update banner

update-available = { -brand-name } { $version } is available.
update-install = Install & restart
update-installing = Installing…
update-install-error = Update failed: { $message }
update-dismiss = Dismiss
