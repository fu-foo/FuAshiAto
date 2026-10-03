//! fuashiato：Windows上の作業（前面ウィンドウ、アイドル、ロック、スリープ、マイク使用）を記録する常駐ツール

mod common;
mod daemon;

#[cfg(windows)]
mod cli;

use std::path::{Path, PathBuf};

use common::console;

const USAGE: &str = "\
FuAshiAto - records the footprints of your own work

Usage: fuashiato <command> [--dir <log folder>]

  run               Record in the foreground and echo records to stdout (for testing)
  start             Start recording in the background
  stop              Stop recording
  status [--brief]  Show the state and today's active time
  pause [duration]  Pause (e.g. 30m, 2h; no argument: until resumed)
  resume            Resume
  today             Today's active time per process
  startup on|off    Add / remove the shortcut in the Startup folder
  open              Open the log folder
  version           Show the version

Logs go to the logs folder next to the EXE unless --dir is given.
While the recorder is running, its folder is used even without --dir.";

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let Some((dir, rest)) = split_dir(&raw) else {
        std::process::exit(usage_error());
    };
    let args: Vec<&str> = rest.iter().map(String::as_str).collect();
    resolve_dir(dir.as_deref(), &args);
    std::process::exit(dispatch(&args, dir.as_deref()));
}

/// `--dir <パス>` / `--dir=<パス>` を取り出す。値がなければ `None`（引数エラー）
fn split_dir(raw: &[String]) -> Option<(Option<PathBuf>, Vec<String>)> {
    let mut dir = None;
    let mut rest = Vec::new();
    let mut it = raw.iter();
    while let Some(a) = it.next() {
        if a == "--dir" {
            dir = Some(PathBuf::from(it.next()?));
        } else if let Some(v) = a.strip_prefix("--dir=") {
            if v.is_empty() {
                return None;
            }
            dir = Some(PathBuf::from(v));
        } else {
            rest.push(a.clone());
        }
    }
    Some((dir, rest))
}

/// 出力先を決める：`--dir` の指定 ＞ 常駐中のプロセスの出力先 ＞ exe の隣の `logs` フォルダ
fn resolve_dir(explicit: Option<&Path>, args: &[&str]) {
    let dir = match explicit {
        Some(d) => common::paths::absolute(d),
        None => {
            let launches = matches!(args.first(), Some(&"run") | Some(&"start"));
            #[cfg(windows)]
            let running = if launches { None } else { cli::daemon_dir() };
            #[cfg(not(windows))]
            let running: Option<PathBuf> = { let _ = launches; None };
            running.unwrap_or_else(common::paths::default_dir)
        }
    };
    common::paths::init(dir);
}

fn usage_error() -> i32 {
    console::err(USAGE);
    3
}

#[cfg(windows)]
fn dispatch(args: &[&str], explicit_dir: Option<&Path>) -> i32 {
    match args {
        ["run"] => daemon::run(false),
        ["run", "--detached"] => daemon::run(true),
        ["start"] => cli::start::start(),
        ["stop"] => cli::control::stop(),
        ["status"] => cli::status::status(false),
        ["status", "--brief"] => cli::status::status(true),
        ["pause"] => cli::control::pause(None),
        ["pause", t] => cli::control::pause(Some(t)),
        ["resume"] => cli::control::resume(),
        ["today"] => cli::status::today(),
        ["startup", arg] => cli::startup::startup(Some(arg), explicit_dir),
        ["open"] => open_dir(),
        ["version"] | ["--version"] | ["-V"] => {
            console::out(&format!("fuashiato {}", env!("CARGO_PKG_VERSION")));
            0
        }
        ["help"] | ["--help"] | ["-h"] => {
            console::out(USAGE);
            0
        }
        _ => usage_error(),
    }
}

#[cfg(windows)]
fn open_dir() -> i32 {
    let dir = common::paths::log_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        console::err(&format!("Could not create the log folder: {e}"));
        return 10;
    }
    match std::process::Command::new("explorer.exe").arg(&dir).spawn() {
        Ok(_) => {
            console::out(&dir.display().to_string());
            0
        }
        Err(e) => {
            console::err(&format!("Could not start Explorer: {e}"));
            10
        }
    }
}

#[cfg(not(windows))]
fn dispatch(args: &[&str], _explicit_dir: Option<&Path>) -> i32 {
    match args {
        ["version"] => {
            console::out(&format!("fuashiato {}", env!("CARGO_PKG_VERSION")));
            0
        }
        _ => {
            console::err("fuashiato runs on Windows only");
            usage_error()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn dir_option() {
        let (d, rest) = split_dir(&s(&["start", "--dir", r"D:\logs"])).unwrap();
        assert_eq!(d, Some(PathBuf::from(r"D:\logs")));
        assert_eq!(rest, s(&["start"]));
        let (d, rest) = split_dir(&s(&["--dir=logs", "status", "--brief"])).unwrap();
        assert_eq!(d, Some(PathBuf::from("logs")));
        assert_eq!(rest, s(&["status", "--brief"]));
        let (d, _) = split_dir(&s(&["today"])).unwrap();
        assert_eq!(d, None);
        assert!(split_dir(&s(&["start", "--dir"])).is_none());
        assert!(split_dir(&s(&["start", "--dir="])).is_none());
    }
}
