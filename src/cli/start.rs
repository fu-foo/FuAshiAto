//! 切り離し起動：自分自身を `run --detached` として起動して即終了する

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS,
    PROCESS_INFORMATION, STARTUPINFOW,
};

use super::{find_daemon, wait_until};
use crate::common::{console, errlog, paths};
use crate::daemon::wide;

/// ハンドルを一切継承させずに起動する。
/// （`std::process::Command` は継承可能なハンドルをすべて渡すため、呼び出し元の標準出力のパイプを
/// 常駐プロセスが握ったままになり、`$x = fuashiato start` などが終わらなくなる）
fn spawn_detached(exe: &str, flags: u32) -> Result<u32, u32> {
    let dir = super::quote_arg(&paths::log_dir().to_string_lossy());
    let mut cmdline = wide(&format!("\"{exe}\" run --detached --dir {dir}"));
    let cwd = wide(&paths::log_dir().to_string_lossy());
    unsafe {
        let si = STARTUPINFOW { cb: std::mem::size_of::<STARTUPINFOW>() as u32, ..std::mem::zeroed() };
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
        let ok = CreateProcessW(
            std::ptr::null(),
            cmdline.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            flags,
            std::ptr::null(),
            cwd.as_ptr(),
            &si,
            &mut pi,
        );
        if ok == 0 {
            return Err(windows_sys::Win32::Foundation::GetLastError());
        }
        CloseHandle(pi.hThread);
        CloseHandle(pi.hProcess);
        Ok(pi.dwProcessId)
    }
}

pub fn start() -> i32 {
    if let Some((_, pid)) = find_daemon() {
        console::out(&format!("Already running (PID {pid})"));
        return 2;
    }
    let exe = match std::env::current_exe() {
        Ok(p) => p.to_string_lossy().into_owned(),
        Err(e) => {
            console::err(&format!("Could not get the path of the executable: {e}"));
            return 10;
        }
    };
    let _ = std::fs::create_dir_all(paths::log_dir());

    // 起動元のシェルやターミナルのジョブに巻き込まれて終了しないよう、可能ならジョブから抜ける
    let base = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
    let spawned = spawn_detached(&exe, base | CREATE_BREAKAWAY_FROM_JOB).or_else(|_| spawn_detached(&exe, base));
    let child_pid = match spawned {
        Ok(pid) => pid,
        Err(code) => {
            errlog::log(&format!("CreateProcessW failed (GetLastError={code})"));
            console::err(&format!("Could not start the recorder (error {code})"));
            return 10;
        }
    };

    if wait_until(5_000, || find_daemon().is_some()) {
        let pid = find_daemon().map(|(_, p)| p).unwrap_or(child_pid);
        console::out(&format!("Started (PID {pid})"));
        0
    } else {
        console::err(&format!(
            "Could not confirm that the recorder started. See {}",
            paths::log_dir().join(paths::ERROR_LOG).display()
        ));
        10
    }
}
