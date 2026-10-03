# FuAshiAto

Records the footprints (*ashiato*, 足跡) of your own work on Windows: which window was in front, and when there was no input. A single EXE that runs in the background and writes plain-text logs to a local folder.

![Windows](https://img.shields.io/badge/platform-Windows-blue)
![Rust](https://img.shields.io/badge/Rust-2021-orange)
![License](https://img.shields.io/badge/license-Apache%202.0-green)

It exists to answer one question, for yourself: *where did my working hours actually go?* Nothing has to be typed or tagged. You start it once and read the log later.

This is a 0.x release. The log format and the commands may still change.

## Read this first

FuAshiAto runs without a window and keeps a record of what you do on the PC. That is the same shape as monitoring software, so here is exactly what it is and is not.

**It is for recording your own activity, under your own Windows account.** Do not install it on a PC or an account that someone else uses in order to watch them. Doing that without their knowledge is unethical and, in many places, illegal.

**On a work PC, check your employer's rules before you run it.** Window titles can contain the names of documents, customers and e-mail subjects, and the log is written to disk as plain text. Managed PCs may also stop it for technical reasons: endpoint protection (EDR) can flag an unsigned program that watches the foreground window, and AppLocker or WDAC policies often block unsigned EXEs in user folders.

It makes no attempt to hide. The process appears in Task Manager as `fuashiato.exe`, automatic start-up is an ordinary shortcut in your Startup folder, and it runs without administrator rights.

### What is recorded

| Item | Detail |
|---|---|
| Foreground window | Process name (e.g. `Code.exe`) and window title, with start and end time |
| No input | Periods with no keyboard or mouse input for 60 seconds or more. Only the time of the last input is read, never the input itself |
| Lock / remote disconnect | When the session was locked or an RDP session was disconnected |
| Sleep | When the PC was asleep |
| Microphone in use | Which app had the microphone open, and when, checked every 10 seconds. This is read from the registry key Windows keeps for its own privacy indicator. Store apps appear as a package name (`MSTeams_8wekyb3d8bbwe`); other apps appear as the full path of their EXE with `\` replaced by `#`, which can include your user name |
| File header | Host name, Windows user name, UTC offset |

### What is not recorded

- Keystrokes, mouse clicks or mouse position
- Screenshots or anything shown on the screen other than the window title
- Audio. The microphone is never opened; only "app X started / stopped using it" is logged
- Browser URLs, file contents, clipboard
- Anything over the network. FuAshiAto makes no network connection and listens on no port. Logs stay in the local folder until you move them

### What a window title can give away

A title is often more than an app name: `Client X contract draft.docx - Word`, the subject of the e-mail you are reading, the title of the web page in a private browser window. All of it lands in the log. Use `fuashiato pause` before doing something you do not want recorded, and treat the log folder as you would any private document.

Two ways the log can leave the PC without FuAshiAto sending anything:

- **Synced folders.** If the log folder is inside OneDrive, Dropbox or a redirected Documents / Desktop folder, the logs are uploaded. Keep them outside such folders.
- **Summarising with an online AI service.** Pasting a log into one hands over every window title in it.

## Getting started

Tested on Windows 11 (x64). Windows 10 and ARM64 are untested.

### Scoop (recommended)

```powershell
scoop bucket add fu-foo https://github.com/fu-foo/scoop-bucket
scoop install fuashiato
```

- Logs are kept in `~\scoop\persist\fuashiato\logs\`, so they survive updates.
- `scoop update fuashiato` stops the running recorder first. Run `fuashiato start` again afterwards (or sign out and in, if you registered it at start-up). The start-up shortcut points at Scoop's `current` folder, so it keeps working after an update.
- `scoop uninstall fuashiato` leaves the logs in place; add `-p` to delete them too. Run `fuashiato startup off` before uninstalling, or the shortcut is left behind.

### Manual download

1. Download `fuashiato-x64.exe` from [Releases](https://github.com/fu-foo/FuAshiAto/releases) and rename it to `fuashiato.exe`.
2. Put it in a folder that only you can write to, for example `%LOCALAPPDATA%\FuAshiAto`. `C:\Program Files` does not work, because the logs are written next to the EXE.
3. Run `fuashiato start` from a terminal in that folder.

The EXE is not digitally signed, so Windows SmartScreen may warn the first time you run a manually downloaded copy. Scoop verifies the download hash; for a manual download, compare it with `SHA256SUMS.txt` in the release.

## Usage

```
fuashiato start            Start recording in the background (no window)
fuashiato stop             Stop recording
fuashiato status           State, today's active time, pause state
fuashiato status --brief   One line, e.g. for a PowerShell prompt
fuashiato pause 30m        Pause for 30m / 2h / 1h30m (no argument: until resumed)
fuashiato resume           Resume
fuashiato today            Today's active time per process
fuashiato startup on|off   Add / remove the shortcut in your Startup folder
fuashiato open             Open the log folder in Explorer
fuashiato run              Record in the foreground and echo records to stdout (Ctrl+C to stop)
fuashiato version
```

Add `--dir <folder>` to any command to use a different log folder, e.g. `fuashiato start --dir "%LOCALAPPDATA%\FuAshiAto\logs"`. Choose a folder under your own profile: on a shared PC, a folder at the root of a data drive (`D:\...`) is normally readable by the other users. While the recorder is running, the other commands find its folder by themselves.

Only one recorder runs per session; a second `start` reports the running one and exits with code 2. Exit codes: `0` success, `1` not running, `2` already running, `3` bad arguments, `10` other error.

`status` and `today` read today's log file each time they are called, and add the interval that is still open (from `current.json`) up to the present moment. If the recorder is not running but `current.json` is still there, the open interval is counted up to its last update.

### "Active" does not mean "working", and "idle" does not mean "away"

`idle` only says that no key or mouse input arrived for 60 seconds. Reading a long document, watching a shared screen in a meeting and talking on the phone all count as idle.

- The active time shown by `status` and `today` is the sum of the `active` intervals and nothing else. On a day of meetings and reading it will be well below the hours you worked.
- The 60-second threshold is fixed, and deliberately short. Because every idle interval is in the log with its real start and end, whoever reads the log can apply a longer threshold afterwards, for example by counting idle intervals under five minutes as work.
- `mic_on` / `mic_off` events mark when an app had the microphone, which is the usual way to recognise a call inside idle time.

## The data

Everything is in one folder: `logs\` next to the EXE, unless you pass `--dir`.

| File | Content |
|---|---|
| `{HOSTNAME}_{YYYY-MM-DD}.jsonl` | The log. One file per day (local time), one JSON record per line, UTF-8 |
| `current.json` | The interval that is still open. Used to detect a crash or power loss on the next start; removed on a clean stop |
| `fuashiato-error.log` | Errors from the background process |

Logs are never rotated or deleted. Delete old files yourself when you no longer need them.

```json
{"type":"header","schema":1,"app":"0.2.0","host":"HOGE-PC","user":"hoge","tz":"+09:00"}
{"type":"seg","s":"2026-09-28T09:12:03.412+09:00","e":"2026-09-28T09:14:47.090+09:00","st":"active","proc":"hoge.exe","title":"fuga.txt - Hoge Editor"}
{"type":"seg","s":"2026-09-28T09:14:47.090+09:00","e":"2026-09-28T09:31:10.004+09:00","st":"idle","proc":"","title":""}
{"type":"event","t":"2026-09-28T10:00:12.003+09:00","ev":"mic_on","app":"HogeMeet_abcdefgh12345"}
```

### Intervals (`seg`)

- `st` is `active`, `idle`, `locked`, `sleep` or `paused`. If several apply, the first of `sleep`, `locked`, `paused`, `idle` wins. Process and title are empty unless it is `active`; a paused interval records only that you paused.
- An interval is written when it ends. A new one starts whenever the foreground window changes **or its title changes**, so an app that puts a counter or the current tab in its title produces many short intervals. None are dropped or merged; do that when you read the log.
- `proc` is empty if the process name could not be read, which happens for windows of protected system processes. The title is still recorded. For Store apps, the real process is recorded rather than `ApplicationFrameHost.exe` when it can be found.
- An idle interval starts at the time of the last input, not when the 60 seconds ran out, so the threshold does not leak into active time. It ends the same way: at the time of the input that broke it, not when the 5-second check noticed.
- An interval never crosses midnight. At 00:00 local time it is closed at `23:59:59.999` and continued in the next day's file from `00:00:00.000`, so the intervals of each daily file can be summed on their own. The 1 ms gap is intentional: it keeps both ends of every interval on the date of its file.
- Every timestamp carries its own UTC offset and is the value to trust. `tz` in the header is only the offset at the moment the file was created, and a day is not always 24 hours where daylight saving time applies.

### Events (`event`)

| `ev` | Meaning | Extra fields |
|---|---|---|
| `start` / `stop` | The recorder started / stopped | `mode`, `reason` (`user`, `logoff`, `shutdown`) |
| `mic_on` / `mic_off` | An app started / stopped using the microphone | `app` |
| `abnormal_exit` | The previous run ended without a clean stop | `from`, `to`: the span with no record |
| `clock_backward` | The system clock was set back | `from`, `to` |
| `sleep_detected` | Diagnostic only, see below | `method`, `ms` |

- **Microphone.** The time of `mic_on` / `mic_off` is when the 10-second check noticed the change, so it can be up to 10 seconds late. Apps already using the microphone when the recorder starts get a `mic_on` right after `start`. No `mic_off` is written when the recorder stops, and nothing is written at midnight, so a call that runs past midnight has its `mic_on` in one file and its `mic_off` in the next: pair them across files, and treat `stop` as the end of any call still open.
- **Sleep.** The `sleep` interval is the record to use. Sleep is detected in three ways (a power notification, a jump in the wall clock, and a comparison of two system timers) because none is reliable on every machine. Each method that fires also writes a `sleep_detected` event with its own measurement; these are there to compare the methods and can be ignored when summing time.
- **Crash or power loss.** `current.json` is rewritten every minute and every time an interval changes. On the next start, the interval that was open is written with its end set to the last rewrite and `"end_unknown":true`, followed by an `abnormal_exit` event. At most about a minute of the open interval is lost.

### Compatibility

`schema` in the header is the format version. New fields and new event types may appear without a change of `schema`; it changes when the meaning of an existing field changes. Readers should ignore what they do not recognise.

### Reading the log

The records are kept raw on purpose. Classifying and summing them up is left to whatever you read them with. Active hours per process over all days, in PowerShell. Run it in the folder that contains `logs\`, or replace `.\logs` with the folder that `fuashiato open` shows:

```powershell
Get-Content .\logs\*.jsonl -Encoding UTF8 | ForEach-Object { $_ | ConvertFrom-Json } |
  Where-Object { $_.type -eq 'seg' -and $_.st -eq 'active' } |
  Group-Object proc |
  ForEach-Object {
    $h = ($_.Group | ForEach-Object { ([datetimeoffset]$_.e - [datetimeoffset]$_.s).TotalHours } |
          Measure-Object -Sum).Sum
    [pscustomobject]@{ proc = $_.Name; hours = [math]::Round($h, 2) }
  } | Sort-Object hours -Descending
```

### Working over Remote Desktop

On the PC you sit at, a remote session is one window: `mstsc.exe`, with no view of what happens inside. Run FuAshiAto on the remote machine as well to record that side. The host name in each file name keeps the two sets of logs apart, and they can be matched by time. Input is counted per session, so the remote side goes idle when you leave the Remote Desktop window, and disconnecting is recorded there as `locked`.

### Stopping and deleting

- Stop now: `fuashiato stop`. Stop it from starting at sign-in: `fuashiato startup off`.
- Delete the record: delete the files in the log folder. There is no other copy, and nothing is stored in the registry.

## How it works

Win32 API only, through [`windows-sys`](https://crates.io/crates/windows-sys); no runtime to install. One hidden window, one message loop and four timers. The EXE is about 440 KB and the running process holds about 1.5 MB of private memory.

- Foreground changes come from `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` with `WINEVENT_OUTOFCONTEXT`, so no code is injected into other processes. Title changes are picked up by polling once a second.
- Idle time comes from `GetLastInputInfo`, polled every 5 seconds. There are no keyboard or mouse hooks.
- It does not use UI Automation and never asks another process to draw or describe its window.
- `status` and `today` read the log files directly, so they work even if the recorder is stuck.

## Building

```
cargo build --release
cargo test
```

Rust 1.86 or later. Cross-compiling from macOS or Linux works with [`cargo-xwin`](https://github.com/rust-cross/cargo-xwin):

```
rustup target add x86_64-pc-windows-msvc
cargo xwin build --release --target x86_64-pc-windows-msvc
```

## Support

If you find this project useful, consider supporting it:

[![GitHub Sponsors](https://img.shields.io/badge/Sponsor-GitHub-ea4aaa?logo=github)](https://github.com/sponsors/fu-foo)
[![Ko-fi](https://img.shields.io/badge/Support-Ko--fi-FF5E5B?logo=kofi)](https://ko-fi.com/fufoo)

## License

Apache License 2.0 - See [LICENSE](LICENSE) for details.
