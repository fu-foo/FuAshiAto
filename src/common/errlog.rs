//! `fuashiato-error.log` への追記。常駐プロセスは標準出力を持たないため、エラーはすべてここに書く。

use std::fs::OpenOptions;
use std::io::Write;

use super::{clock, paths};

pub fn log(msg: &str) {
    let path = paths::log_dir().join(paths::ERROR_LOG);
    let _ = std::fs::create_dir_all(paths::log_dir());
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{} [{}] {}", clock::fmt(&clock::now()), std::process::id(), msg);
    }
}

/// 直前のWin32エラーを付けて記録する
#[cfg(windows)]
pub fn log_win32(what: &str) {
    let code = unsafe { windows_sys::Win32::Foundation::GetLastError() };
    log(&format!("{what} failed (GetLastError={code})"));
}
