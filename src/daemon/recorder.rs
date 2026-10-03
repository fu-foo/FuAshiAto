//! 状態機械。Win32に依存しない。
//!
//! 入力（前面の変化、アイドル、ロック、時刻のジャンプなど）を受けて、書き出すレコードを溜める。
//! 呼び出し側は各入力のあとに [`Recorder::take`] で溜まったレコードを取り出して書き出す。
//!
//! 状態の優先順位は `sleep` ＞ `locked` ＞ `paused` ＞ `idle` ＞ `active`。
//! 各フラグを独立に保持し、フラグが変わるたびに「あるべき区間」を計算し直して、
//! 現在の区間と異なれば区間を切り替える。

use crate::common::clock::{self, Time};
use crate::common::model::{Event, OpenSeg, Record, Seg, St};

/// 記録時のアイドル閾値（ミリ秒）
pub const IDLE_THRESHOLD_MS: u32 = 60_000;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Win {
    pub proc: String,
    pub title: String,
}

impl Win {
    pub fn new(proc: &str, title: &str) -> Self {
        Win { proc: proc.into(), title: title.into() }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Open {
    s: Time,
    st: St,
    win: Win,
}

pub struct Recorder {
    /// 最新の前面ウィンドウ
    fg: Win,
    idle: bool,
    locked: bool,
    paused: bool,
    paused_until: Option<Time>,
    /// 電源通知（PBT_APMSUSPEND）で入ったスリープの開始時刻
    suspended_at: Option<Time>,
    /// 最後に記録した sleep 区間の終了時刻（検出方式どうしの重複防止）
    last_sleep_end: Option<Time>,
    open: Option<Open>,
    /// 最後に受け取った時刻（逆行の検出用）
    last_t: Time,
    out: Vec<Record>,
}

impl Recorder {
    /// 記録を開始する。`start` イベントを出し、最初の区間を開く。
    pub fn new(t: Time, fg: Option<Win>, mode: &str) -> Self {
        let mut r = Recorder {
            fg: fg.unwrap_or_default(),
            idle: false,
            locked: false,
            paused: false,
            paused_until: None,
            suspended_at: None,
            last_sleep_end: None,
            open: None,
            last_t: t,
            out: Vec::new(),
        };
        let mut ev = Event::new(t, "start");
        ev.mode = Some(mode.to_string());
        r.out.push(Record::Event(ev));
        r.transition(t);
        r
    }

    /// 溜まったレコードを取り出す
    pub fn take(&mut self) -> Vec<Record> {
        std::mem::take(&mut self.out)
    }

    pub fn open_seg(&self) -> Option<OpenSeg> {
        self.open.as_ref().map(|o| OpenSeg {
            s: o.s,
            st: o.st,
            proc: o.win.proc.clone(),
            title: o.win.title.clone(),
        })
    }

    /// (一時停止中か, 期限)
    pub fn pause_state(&self) -> (bool, Option<Time>) {
        (self.paused, self.paused_until)
    }

    pub fn is_stopped(&self) -> bool {
        self.open.is_none()
    }

    // ---- 入力 ----

    /// 前面ウィンドウの切り替え、またはタイトルの変更
    pub fn foreground(&mut self, t: Time, win: Win) {
        if self.is_stopped() {
            return;
        }
        self.advance(t);
        self.fg = win;
        self.transition(t);
    }

    /// アイドル判定。`idle_ms` は最終入力からの経過ミリ秒。
    pub fn idle(&mut self, t: Time, idle_ms: u32) {
        if self.is_stopped() {
            return;
        }
        self.advance(t);
        let last_input = t - clock::ms(idle_ms as i64);

        // 電源通知でスリープに入ったまま復帰通知が来ていない場合、入力があれば復帰とみなす
        if let Some(sa) = self.suspended_at {
            if idle_ms < IDLE_THRESHOLD_MS && last_input > sa {
                self.wake(last_input);
            }
            return;
        }

        if !self.idle && idle_ms >= IDLE_THRESHOLD_MS {
            self.idle = true;
            self.transition(last_input);
        } else if self.idle && idle_ms < IDLE_THRESHOLD_MS {
            self.idle = false;
            self.transition(last_input);
        }
    }

    /// ロック、リモート切断
    pub fn lock(&mut self, t: Time) {
        if self.is_stopped() {
            return;
        }
        self.advance(t);
        self.locked = true;
        self.transition(t);
    }

    /// ロック解除、リモート接続
    pub fn unlock(&mut self, t: Time) {
        if self.is_stopped() {
            return;
        }
        self.advance(t);
        self.locked = false;
        self.idle = false;
        self.transition(t);
    }

    /// 一時停止。`until` が `None` なら無期限。
    pub fn pause(&mut self, t: Time, until: Option<Time>) {
        if self.is_stopped() {
            return;
        }
        self.advance(t);
        self.paused = true;
        self.paused_until = until;
        self.transition(t);
    }

    pub fn resume(&mut self, t: Time) {
        if self.is_stopped() || !self.paused {
            return;
        }
        self.advance(t);
        self.paused = false;
        self.paused_until = None;
        self.idle = false;
        self.transition(t);
    }

    /// 1秒ごとの定期処理：日付変更と一時停止の期限
    pub fn tick(&mut self, t: Time) {
        if self.is_stopped() {
            return;
        }
        self.advance(t);
        if self.paused {
            if let Some(until) = self.paused_until {
                if until <= t {
                    self.paused = false;
                    self.paused_until = None;
                    self.idle = false;
                    self.transition(until);
                }
            }
        }
    }

    /// 時刻ジャンプによるスリープ検出：`[from, to]` をスリープとして記録する
    pub fn sleep_gap(&mut self, from: Time, to: Time) {
        if self.is_stopped() {
            return;
        }
        self.check_backward(to);
        if self.suspended_at.is_some() {
            // 電源通知で既にスリープ区間を開いている。復帰時刻で閉じるだけ
            self.wake(to);
            self.last_t = to;
            return;
        }
        if matches!(self.last_sleep_end, Some(end) if end >= from) {
            // 電源通知側で既に記録済み
            self.last_t = to;
            return;
        }
        self.suspended_at = Some(from);
        self.transition(from);
        self.suspended_at = None;
        self.idle = false;
        self.last_sleep_end = Some(to);
        self.transition(to);
        self.last_t = to;
    }

    /// 電源通知 PBT_APMSUSPEND
    pub fn suspend(&mut self, t: Time) {
        if self.is_stopped() || self.suspended_at.is_some() {
            return;
        }
        self.advance(t);
        self.suspended_at = Some(t);
        self.transition(t);
    }

    /// 電源通知 PBT_APMRESUMEAUTOMATIC / PBT_APMRESUMESUSPEND
    pub fn power_resume(&mut self, t: Time) {
        if self.is_stopped() || self.suspended_at.is_none() {
            return;
        }
        self.check_backward(t);
        self.wake(t);
        self.last_t = t;
    }

    /// 検証用・マイクなどのイベントをそのまま記録する
    pub fn event(&mut self, ev: Event) {
        self.out.push(Record::Event(ev));
    }

    /// 停止：現在の区間を閉じて `stop` イベントを書く
    pub fn stop(&mut self, t: Time, reason: &str) {
        if self.is_stopped() {
            return;
        }
        self.advance(t);
        if let Some(o) = self.open.take() {
            self.close(o, t);
        }
        let mut ev = Event::new(t, "stop");
        ev.reason = Some(reason.to_string());
        self.out.push(Record::Event(ev));
    }

    // ---- 内部 ----

    fn wake(&mut self, t: Time) {
        self.suspended_at = None;
        self.idle = false;
        let t = self.clamp(t);
        self.last_sleep_end = Some(t);
        self.transition(t);
    }

    /// 時刻の逆行の検出と日付変更の処理
    fn advance(&mut self, t: Time) {
        self.check_backward(t);
        self.last_t = t;
        if let Some(o) = &mut self.open {
            if t.date_naive() > o.s.date_naive() {
                let mid = clock::start_of_day(t.date_naive());
                let closing = Open { s: o.s, st: o.st, win: o.win.clone() };
                o.s = mid;
                emit_split(&mut self.out, closing.s, mid - clock::ms(1), closing.st, &closing.win, false);
            }
        }
    }

    fn check_backward(&mut self, t: Time) {
        if t < self.last_t {
            let mut ev = Event::new(t, "clock_backward");
            ev.from = Some(self.last_t);
            ev.to = Some(t);
            self.out.push(Record::Event(ev));
        }
    }

    fn clamp(&self, t: Time) -> Time {
        match &self.open {
            Some(o) if t < o.s => o.s,
            _ => t,
        }
    }

    fn desired(&self) -> (St, Win) {
        let st = if self.suspended_at.is_some() {
            St::Sleep
        } else if self.locked {
            St::Locked
        } else if self.paused {
            St::Paused
        } else if self.idle {
            St::Idle
        } else {
            St::Active
        };
        let win = if st == St::Active { self.fg.clone() } else { Win::default() };
        (st, win)
    }

    /// あるべき区間と現在の区間が異なれば、`at` で切り替える
    fn transition(&mut self, at: Time) {
        let (st, win) = self.desired();
        if let Some(o) = &self.open {
            if o.st == st && o.win == win {
                return;
            }
        }
        let at = self.clamp(at);
        if let Some(o) = self.open.take() {
            self.close(o, at);
        }
        self.open = Some(Open { s: at, st, win });
    }

    fn close(&mut self, o: Open, e: Time) {
        let e = if e < o.s { o.s } else { e };
        emit_split(&mut self.out, o.s, e, o.st, &o.win, false);
    }
}

/// 区間を書き出す。0時をまたぐ場合は `23:59:59.999` と `00:00:00.000` で分割する。
pub fn emit_split(out: &mut Vec<Record>, s: Time, e: Time, st: St, win: &Win, end_unknown: bool) {
    let mut s = s;
    let e = if e < s { s } else { e };
    while e.date_naive() > s.date_naive() {
        let eod = clock::end_of_day(s.date_naive());
        out.push(seg(s, eod, st, win, false));
        s = clock::start_of_day(s.date_naive().succ_opt().unwrap_or(s.date_naive()));
    }
    out.push(seg(s, e, st, win, end_unknown));
}

fn seg(s: Time, e: Time, st: St, win: &Win, end_unknown: bool) -> Record {
    Record::Seg(Seg {
        s,
        e,
        st,
        proc: win.proc.clone(),
        title: win.title.clone(),
        end_unknown,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Local, TimeZone};

    fn at(h: u32, m: u32, s: u32) -> Time {
        Local.with_ymd_and_hms(2026, 9, 28, h, m, s).unwrap()
    }

    fn segs(recs: &[Record]) -> Vec<(Time, Time, St, String)> {
        recs.iter()
            .filter_map(|r| match r {
                Record::Seg(s) => Some((s.s, s.e, s.st, s.proc.clone())),
                _ => None,
            })
            .collect()
    }

    fn events(recs: &[Record]) -> Vec<String> {
        recs.iter()
            .filter_map(|r| match r {
                Record::Event(e) => Some(e.ev.clone()),
                _ => None,
            })
            .collect()
    }

    fn rec() -> Recorder {
        let mut r = Recorder::new(at(9, 0, 0), Some(Win::new("Code.exe", "a.rs")), "run");
        assert_eq!(events(&r.take()), vec!["start"]);
        r
    }

    #[test]
    fn foreground_switch_cuts_segment() {
        let mut r = rec();
        r.foreground(at(9, 1, 0), Win::new("chrome.exe", "Google"));
        let out = r.take();
        assert_eq!(segs(&out), vec![(at(9, 0, 0), at(9, 1, 0), St::Active, "Code.exe".into())]);
        // 同じウィンドウなら切らない
        r.foreground(at(9, 1, 5), Win::new("chrome.exe", "Google"));
        assert!(r.take().is_empty());
        // タイトル変更で切る
        r.foreground(at(9, 1, 6), Win::new("chrome.exe", "Gmail"));
        assert_eq!(segs(&r.take()).len(), 1);
    }

    #[test]
    fn idle_backdates_to_last_input() {
        let mut r = rec();
        r.idle(at(9, 5, 0), 30_000);
        assert!(r.take().is_empty());
        // 9:05:05 に 65秒 無入力 → 最終入力 9:04:00 で切る
        r.idle(at(9, 5, 5), 65_000);
        let out = segs(&r.take());
        assert_eq!(out, vec![(at(9, 0, 0), at(9, 4, 0), St::Active, "Code.exe".into())]);
        assert_eq!(r.open_seg().unwrap().st, St::Idle);
        assert_eq!(r.open_seg().unwrap().s, at(9, 4, 0));
        // 入力が戻る：9:10:00 時点で 2秒前に入力 → idle は 9:09:58 まで
        r.idle(at(9, 10, 0), 2_000);
        let out = segs(&r.take());
        assert_eq!(out, vec![(at(9, 4, 0), at(9, 9, 58), St::Idle, "".into())]);
        let o = r.open_seg().unwrap();
        assert_eq!((o.s, o.st, o.proc.as_str()), (at(9, 9, 58), St::Active, "Code.exe"));
    }

    #[test]
    fn idle_segments_have_empty_proc_and_title() {
        let mut r = rec();
        r.idle(at(9, 5, 0), 120_000);
        r.foreground(at(9, 6, 0), Win::new("chrome.exe", "x"));
        assert!(segs(&r.take()).iter().all(|s| s.2 == St::Active));
        let o = r.open_seg().unwrap();
        assert_eq!((o.st, o.proc.as_str(), o.title.as_str()), (St::Idle, "", ""));
        // 復帰時は最新の前面ウィンドウで active
        r.idle(at(9, 7, 0), 0);
        assert_eq!(r.open_seg().unwrap().proc, "chrome.exe");
    }

    #[test]
    fn lock_and_unlock() {
        let mut r = rec();
        r.lock(at(10, 0, 0));
        r.unlock(at(10, 30, 0));
        let out = segs(&r.take());
        assert_eq!(
            out,
            vec![
                (at(9, 0, 0), at(10, 0, 0), St::Active, "Code.exe".into()),
                (at(10, 0, 0), at(10, 30, 0), St::Locked, "".into()),
            ]
        );
        assert_eq!(r.open_seg().unwrap().st, St::Active);
    }

    #[test]
    fn locked_beats_idle_and_paused() {
        let mut r = rec();
        r.pause(at(9, 10, 0), None);
        r.lock(at(9, 20, 0));
        r.idle(at(9, 25, 0), 300_000); // ロック中のアイドルは区間を切らない
        let out = segs(&r.take());
        assert_eq!(out.iter().map(|s| s.2).collect::<Vec<_>>(), vec![St::Active, St::Paused]);
        assert_eq!(r.open_seg().unwrap().st, St::Locked);
        r.unlock(at(9, 30, 0));
        // 一時停止中のまま
        assert_eq!(r.open_seg().unwrap().st, St::Paused);
    }

    #[test]
    fn pause_with_expiry() {
        let mut r = rec();
        r.pause(at(9, 10, 0), Some(at(9, 11, 0)));
        assert_eq!(r.pause_state(), (true, Some(at(9, 11, 0))));
        r.tick(at(9, 10, 59));
        assert_eq!(r.open_seg().unwrap().st, St::Paused);
        r.tick(at(9, 11, 0));
        let out = segs(&r.take());
        assert_eq!(out[1], (at(9, 10, 0), at(9, 11, 0), St::Paused, "".into()));
        assert_eq!(r.open_seg().unwrap().st, St::Active);
        assert_eq!(r.pause_state(), (false, None));
    }

    #[test]
    fn resume_manually() {
        let mut r = rec();
        r.pause(at(9, 10, 0), None);
        r.resume(at(9, 20, 0));
        let out = segs(&r.take());
        assert_eq!(out[1], (at(9, 10, 0), at(9, 20, 0), St::Paused, "".into()));
        assert_eq!(r.open_seg().unwrap().st, St::Active);
    }

    #[test]
    fn sleep_by_time_jump() {
        let mut r = rec();
        r.sleep_gap(at(12, 0, 0), at(13, 0, 0));
        let out = segs(&r.take());
        assert_eq!(
            out,
            vec![
                (at(9, 0, 0), at(12, 0, 0), St::Active, "Code.exe".into()),
                (at(12, 0, 0), at(13, 0, 0), St::Sleep, "".into()),
            ]
        );
        let o = r.open_seg().unwrap();
        assert_eq!((o.s, o.st), (at(13, 0, 0), St::Active));
    }

    #[test]
    fn sleep_by_power_then_jump_is_not_duplicated() {
        let mut r = rec();
        r.suspend(at(12, 0, 0));
        // 復帰後、時刻ジャンプ検出が先に来る
        r.sleep_gap(at(11, 59, 59), at(13, 0, 0));
        r.power_resume(at(13, 0, 1)); // 既に復帰済み → 何もしない
        let out = segs(&r.take());
        assert_eq!(
            out,
            vec![
                (at(9, 0, 0), at(12, 0, 0), St::Active, "Code.exe".into()),
                (at(12, 0, 0), at(13, 0, 0), St::Sleep, "".into()),
            ]
        );
    }

    #[test]
    fn sleep_by_power_resume_then_jump_is_not_duplicated() {
        let mut r = rec();
        r.suspend(at(12, 0, 0));
        r.power_resume(at(13, 0, 0));
        r.sleep_gap(at(11, 59, 59), at(13, 0, 1));
        let out = segs(&r.take());
        assert_eq!(out.len(), 2);
        assert_eq!(out[1], (at(12, 0, 0), at(13, 0, 0), St::Sleep, "".into()));
        assert_eq!(r.open_seg().unwrap().s, at(13, 0, 0));
    }

    #[test]
    fn suspend_without_resume_wakes_on_input() {
        let mut r = rec();
        r.suspend(at(12, 0, 0));
        r.foreground(at(12, 0, 1), Win::new("x.exe", "x")); // スリープ中は切らない
        r.idle(at(12, 0, 5), 10_000); // 最終入力がスリープ前 → 復帰ではない
        assert_eq!(r.open_seg().unwrap().st, St::Sleep);
        r.idle(at(12, 30, 0), 1_000);
        let out = segs(&r.take());
        assert_eq!(out[1], (at(12, 0, 0), at(12, 29, 59), St::Sleep, "".into()));
        assert_eq!(r.open_seg().unwrap().proc, "x.exe");
    }

    #[test]
    fn date_change_splits_segment() {
        let mut r = rec();
        let next = Local.with_ymd_and_hms(2026, 9, 29, 0, 0, 1).unwrap();
        r.tick(next);
        let out = segs(&r.take());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, at(9, 0, 0));
        assert_eq!(clock::fmt(&out[0].1)[..23].to_string(), "2026-09-28T23:59:59.999");
        let o = r.open_seg().unwrap();
        assert_eq!(clock::fmt(&o.s)[..23].to_string(), "2026-09-29T00:00:00.000");
        assert_eq!(o.proc, "Code.exe");
    }

    #[test]
    fn sleep_across_midnight_is_split() {
        let mut r = rec();
        let wake = Local.with_ymd_and_hms(2026, 9, 29, 8, 0, 0).unwrap();
        r.sleep_gap(at(23, 0, 0), wake);
        let out = segs(&r.take());
        assert_eq!(out.len(), 3);
        assert_eq!(out[1].2, St::Sleep);
        assert_eq!(out[2].2, St::Sleep);
        assert_eq!(out[2].1, wake);
        assert_eq!(r.open_seg().unwrap().s, wake);
    }

    #[test]
    fn clock_backward_clamps_end() {
        let mut r = rec();
        r.foreground(at(10, 0, 0), Win::new("a.exe", "a"));
        r.take();
        r.foreground(at(9, 59, 0), Win::new("b.exe", "b"));
        let out = r.take();
        assert_eq!(events(&out), vec!["clock_backward"]);
        let s = segs(&out);
        assert_eq!(s, vec![(at(10, 0, 0), at(10, 0, 0), St::Active, "a.exe".into())]);
    }

    #[test]
    fn stop_closes_segment() {
        let mut r = rec();
        r.stop(at(18, 0, 0), "user");
        let out = r.take();
        assert_eq!(segs(&out), vec![(at(9, 0, 0), at(18, 0, 0), St::Active, "Code.exe".into())]);
        assert_eq!(events(&out), vec!["stop"]);
        assert!(r.open_seg().is_none());
        // 停止後の入力は無視
        r.foreground(at(18, 1, 0), Win::new("x", "y"));
        assert!(r.take().is_empty());
    }
}
