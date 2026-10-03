//! 保存場所、ファイル名、ホスト名

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use chrono::NaiveDate;

pub const CHECKPOINT: &str = "current.json";
pub const ERROR_LOG: &str = "fuashiato-error.log";

static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();

/// 出力先を決める。起動直後に1回だけ呼ぶ（2回目以降は無視）
pub fn init(dir: PathBuf) {
    let _ = LOG_DIR.set(dir);
}

/// exe と同じフォルダ
pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 既定の出力先：exe の隣の `logs` フォルダ
pub fn default_dir() -> PathBuf {
    exe_dir().join("logs")
}

/// 出力先（ログ、current.json、エラーログ）。`--dir` の指定がなければ exe の隣の `logs` フォルダ
pub fn log_dir() -> PathBuf {
    LOG_DIR.get().cloned().unwrap_or_else(default_dir)
}

/// 相対パスを絶対パスにする（常駐プロセスは作業フォルダが変わるため）
pub fn absolute(p: &Path) -> PathBuf {
    std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())
}

pub fn checkpoint_path() -> PathBuf {
    log_dir().join(CHECKPOINT)
}

/// `{HOSTNAME}_{YYYY-MM-DD}.jsonl`
pub fn log_file(date: NaiveDate) -> PathBuf {
    log_dir().join(format!("{}_{}.jsonl", hostname(), date.format("%Y-%m-%d")))
}

pub fn hostname() -> String {
    static HOST: OnceLock<String> = OnceLock::new();
    HOST.get_or_init(query_hostname).clone()
}

#[cfg(windows)]
fn query_hostname() -> String {
    use windows_sys::Win32::System::SystemInformation::{ComputerNameDnsHostname, GetComputerNameExW};
    let mut buf = [0u16; 256];
    let mut len = buf.len() as u32;
    let ok = unsafe { GetComputerNameExW(ComputerNameDnsHostname, buf.as_mut_ptr(), &mut len) };
    if ok != 0 && len > 0 {
        String::from_utf16_lossy(&buf[..len as usize])
    } else {
        std::env::var("COMPUTERNAME").unwrap_or_else(|_| "UNKNOWN".into())
    }
}

#[cfg(not(windows))]
fn query_hostname() -> String {
    std::env::var("HOSTNAME").unwrap_or_else(|_| "UNKNOWN".into())
}

pub fn username() -> String {
    std::env::var("USERNAME").unwrap_or_default()
}
