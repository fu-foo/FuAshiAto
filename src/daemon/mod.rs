//! 常駐プロセス：隠しトップレベルウィンドウ、メッセージループ、WndProc、タイマー。
//!
//! すべての処理（フックのコールバック、タイマー、ファイル書き込み）をこのスレッドで行う。

pub mod recorder;

#[cfg(windows)]
mod checkpoint;
#[cfg(windows)]
pub mod foreground;
#[cfg(windows)]
mod idle;
#[cfg(windows)]
mod mic;
#[cfg(windows)]
mod power;
#[cfg(windows)]
mod session;
#[cfg(windows)]
mod writer;

#[cfg(windows)]
pub use win::*;

#[cfg(windows)]
mod win {
    use std::cell::RefCell;
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicPtr, Ordering};

    use windows_sys::core::BOOL;
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::Console::{SetConsoleCtrlHandler, CTRL_BREAK_EVENT, CTRL_CLOSE_EVENT, CTRL_C_EVENT};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Threading::CreateMutexW;
    use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, KillTimer,
        PostMessageW, PostQuitMessage, RegisterClassExW, SetTimer, TranslateMessage, ENDSESSION_LOGOFF,
        EVENT_SYSTEM_FOREGROUND, MSG, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND, PBT_APMSUSPEND,
        WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_APP, WM_CLOSE, WM_DESTROY, WM_ENDSESSION,
        WM_POWERBROADCAST, WM_QUERYENDSESSION, WM_TIMER, WM_WTSSESSION_CHANGE, WNDCLASSEXW,
        WS_EX_TOOLWINDOW, WS_POPUP,
    };

    use super::foreground::Foreground;
    use super::mic::Mic;
    use super::power::Power;
    use super::recorder::Recorder;
    use super::writer::Writer;
    use super::{checkpoint, idle, session};
    use crate::common::model::{Event, Record};
    use crate::common::{clock, console, errlog, paths};

    pub const CLASS_NAME: &str = "fuashiato_daemon_v1";
    pub const MUTEX_NAME: &str = r"Local\fuashiato_v1";
    /// 隠しウィンドウのタイトルの接頭辞。後ろに出力先のフォルダが続く
    pub const TITLE_PREFIX: &str = "fuashiato|";
    /// 一時停止。wParam = 分数（0は無期限）
    pub const WM_PAUSE: u32 = WM_APP + 1;
    /// 再開
    pub const WM_RESUME: u32 = WM_APP + 2;

    const TIMER_1S: usize = 1;
    const TIMER_IDLE: usize = 2;
    const TIMER_MIC: usize = 3;
    const TIMER_CHECKPOINT: usize = 4;

    pub fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    struct Daemon {
        hwnd: HWND,
        rec: Recorder,
        writer: Writer,
        fg: Foreground,
        power: Power,
        mic: Mic,
    }

    thread_local! {
        static DAEMON: RefCell<Option<Daemon>> = const { RefCell::new(None) };
    }

    /// コンソールのCtrl+Cハンドラ（別スレッド）から停止を依頼するためのウィンドウハンドル
    static MAIN_HWND: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

    fn with_daemon(f: impl FnOnce(&mut Daemon)) {
        DAEMON.with(|cell| match cell.try_borrow_mut() {
            Ok(mut guard) => {
                if let Some(d) = guard.as_mut() {
                    f(d);
                }
            }
            Err(_) => errlog::log("daemon state re-entered; event dropped"),
        });
    }

    impl Daemon {
        /// 溜まったレコードを書き出す。区間が変わったら current.json も更新する
        fn flush(&mut self) {
            let recs = self.rec.take();
            if recs.is_empty() {
                return;
            }
            self.writer.write_all(&recs);
            if recs.iter().any(|r| matches!(r, Record::Seg(_))) {
                self.checkpoint();
            }
        }

        fn checkpoint(&mut self) {
            if self.rec.is_stopped() {
                return;
            }
            let (paused, until) = self.rec.pause_state();
            checkpoint::write(clock::now(), self.rec.open_seg(), paused, until);
        }

        fn on_timer(&mut self, id: usize) {
            let now = clock::now();
            match id {
                TIMER_1S => {
                    if let Some((from, gap)) = self.power.check_jump(now) {
                        self.rec.event(sleep_detected(now, "jump", gap));
                        self.rec.sleep_gap(from, now);
                    }
                    if let Some(inc) = self.power.check_unbiased() {
                        self.rec.event(sleep_detected(now, "unbiased", inc));
                    }
                    self.rec.tick(now);
                    if let Some(w) = self.fg.read() {
                        self.rec.foreground(now, w);
                    }
                }
                TIMER_IDLE => {
                    if let Some(ms) = idle::idle_ms() {
                        self.rec.idle(now, ms);
                    }
                }
                TIMER_MIC => self.poll_mic(now),
                TIMER_CHECKPOINT => self.checkpoint(),
                _ => {}
            }
            self.flush();
        }

        fn poll_mic(&mut self, now: clock::Time) {
            let Some(diff) = self.mic.poll() else { return };
            for (ev, apps) in [("mic_on", diff.on), ("mic_off", diff.off)] {
                for app in apps {
                    let mut e = Event::new(now, ev);
                    e.app = Some(app);
                    self.rec.event(e);
                }
            }
        }

        fn on_foreground(&mut self) {
            if let Some(w) = self.fg.read() {
                self.rec.foreground(clock::now(), w);
                self.flush();
            }
        }

        fn on_session(&mut self, wparam: WPARAM) {
            let now = clock::now();
            match session::classify(wparam) {
                Some(session::Change::Lock) => self.rec.lock(now),
                Some(session::Change::Unlock) => self.rec.unlock(now),
                None => {}
            }
            self.flush();
        }

        fn on_power(&mut self, event: u32) {
            let now = clock::now();
            match event {
                PBT_APMSUSPEND => {
                    self.power.suspended_at = Some(now);
                    self.rec.suspend(now);
                }
                PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND => {
                    if let Some(ms) = self.power.resumed(now) {
                        self.rec.event(sleep_detected(now, "power", ms));
                        self.rec.power_resume(now);
                    }
                }
                _ => {}
            }
            self.flush();
        }

        fn on_pause(&mut self, minutes: u32) {
            let now = clock::now();
            let until = (minutes > 0).then(|| now + chrono::TimeDelta::minutes(minutes as i64));
            self.rec.pause(now, until);
            self.flush();
            self.checkpoint();
        }

        fn on_resume(&mut self) {
            self.rec.resume(clock::now());
            self.flush();
            self.checkpoint();
        }

        fn shutdown(&mut self, reason: &str) {
            if self.rec.is_stopped() {
                return;
            }
            for id in [TIMER_1S, TIMER_IDLE, TIMER_MIC, TIMER_CHECKPOINT] {
                unsafe { KillTimer(self.hwnd, id) };
            }
            self.rec.stop(clock::now(), reason);
            self.flush();
            checkpoint::remove();
        }
    }

    fn sleep_detected(t: clock::Time, method: &str, ms: i64) -> Event {
        let mut e = Event::new(t, "sleep_detected");
        e.method = Some(method.to_string());
        e.ms = Some(ms);
        e
    }

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match msg {
            WM_TIMER => {
                with_daemon(|d| d.on_timer(wparam));
                0
            }
            WM_WTSSESSION_CHANGE => {
                with_daemon(|d| d.on_session(wparam));
                0
            }
            WM_POWERBROADCAST => {
                with_daemon(|d| d.on_power(wparam as u32));
                1
            }
            WM_QUERYENDSESSION => 1,
            WM_ENDSESSION => {
                if wparam != 0 {
                    let reason = if (lparam as u32) & ENDSESSION_LOGOFF != 0 { "logoff" } else { "shutdown" };
                    with_daemon(|d| d.shutdown(reason));
                }
                0
            }
            WM_CLOSE => {
                with_daemon(|d| d.shutdown("user"));
                DestroyWindow(hwnd);
                0
            }
            WM_DESTROY => {
                session::unregister(hwnd);
                MAIN_HWND.store(std::ptr::null_mut(), Ordering::SeqCst);
                PostQuitMessage(0);
                0
            }
            WM_PAUSE => {
                with_daemon(|d| d.on_pause(wparam as u32));
                0
            }
            WM_RESUME => {
                with_daemon(|d| d.on_resume());
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    unsafe extern "system" fn win_event(
        _hook: HWINEVENTHOOK,
        event: u32,
        _hwnd: HWND,
        _id_object: i32,
        _id_child: i32,
        _thread: u32,
        _time: u32,
    ) {
        if event == EVENT_SYSTEM_FOREGROUND {
            with_daemon(|d| d.on_foreground());
        }
    }

    unsafe extern "system" fn ctrl_handler(ctrl: u32) -> BOOL {
        match ctrl {
            CTRL_C_EVENT | CTRL_BREAK_EVENT | CTRL_CLOSE_EVENT => {
                let h = MAIN_HWND.load(Ordering::SeqCst);
                if !h.is_null() {
                    PostMessageW(h, WM_CLOSE, 0, 0);
                }
                if ctrl == CTRL_CLOSE_EVENT {
                    // ハンドラから戻ると即座に終了させられるため、メインスレッドの後始末を待つ
                    std::thread::sleep(std::time::Duration::from_secs(5));
                }
                1
            }
            _ => 0,
        }
    }

    /// 常駐プロセスの本体。戻り値は終了コード。
    pub fn run(detached: bool) -> i32 {
        let mode = if detached { "detached" } else { "run" };
        let say = |msg: &str| {
            if detached {
                errlog::log(msg);
            } else {
                console::err(msg);
            }
        };

        unsafe {
            // 二重起動防止。ハンドルはプロセス終了まで保持する
            let name = wide(MUTEX_NAME);
            let mutex = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            if mutex.is_null() {
                errlog::log_win32("CreateMutexW");
                say("Could not create the mutex");
                return 10;
            }
            if GetLastError() == ERROR_ALREADY_EXISTS {
                match crate::cli::find_daemon() {
                    Some((_, pid)) => say(&format!("Already running (PID {pid})")),
                    None => say("Already running"),
                }
                return 2;
            }

            let now = clock::now();
            let mut writer = Writer::new(!detached);

            // 前回の異常終了の検出
            let recovery = checkpoint::recover(now);
            if !recovery.records.is_empty() {
                writer.write_all(&recovery.records);
                checkpoint::remove();
                if !detached {
                    console::err("The previous run did not stop cleanly; this has been recorded");
                }
            }

            // 隠しトップレベルウィンドウ（HWND_MESSAGE だとブロードキャストが届かないため使わない）
            let hinst = GetModuleHandleW(std::ptr::null());
            let class = wide(CLASS_NAME);
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wndproc),
                hInstance: hinst,
                lpszClassName: class.as_ptr(),
                ..std::mem::zeroed()
            };
            if RegisterClassExW(&wc) == 0 {
                errlog::log_win32("RegisterClassExW");
                say("Could not register the window class");
                return 10;
            }
            // CLI が常駐プロセスの出力先を知るため、タイトルに出力先を入れる（GetWindowText は相手が固まっていても読める）
            let title = wide(&format!("{TITLE_PREFIX}{}", paths::log_dir().display()));
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                class.as_ptr(),
                title.as_ptr(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                hinst,
                std::ptr::null(),
            );
            if hwnd.is_null() {
                errlog::log_win32("CreateWindowExW");
                say("Could not create the window");
                return 10;
            }
            MAIN_HWND.store(hwnd, Ordering::SeqCst);

            let mut fg = Foreground::new();
            let mut rec = Recorder::new(now, fg.read(), mode);
            if let Some(until) = recovery.pause {
                rec.pause(now, until);
            }
            let mut d = Daemon { hwnd, rec, writer, fg, power: Power::new(now), mic: Mic::new() };
            d.poll_mic(now);
            d.flush();
            d.checkpoint();
            DAEMON.with(|cell| *cell.borrow_mut() = Some(d));

            let hook = SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                std::ptr::null_mut(),
                Some(win_event),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            );
            if hook.is_null() {
                errlog::log_win32("SetWinEventHook");
            }
            session::register(hwnd);
            for (id, ms) in [(TIMER_1S, 1_000), (TIMER_IDLE, 5_000), (TIMER_MIC, 10_000), (TIMER_CHECKPOINT, 60_000)] {
                if SetTimer(hwnd, id, ms, None) == 0 {
                    errlog::log_win32(&format!("SetTimer({id})"));
                }
            }

            if !detached {
                SetConsoleCtrlHandler(Some(ctrl_handler), 1);
                console::err(&format!("Recording (logs: {}). Press Ctrl+C to stop.", paths::log_dir().display()));
            }

            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            if !hook.is_null() {
                UnhookWinEvent(hook);
            }
            DAEMON.with(|cell| *cell.borrow_mut() = None);
        }
        0
    }
}
