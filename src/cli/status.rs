//! status / today。IPCは使わず、当日のJSONLと current.json を直接読んで計算する
//! （常駐プロセスが固まっていても表示できるように）。

use std::collections::HashMap;

use chrono::TimeDelta;

use super::find_daemon;
use crate::common::clock::{self, Time};
use crate::common::model::{Checkpoint, OpenSeg, Record, St};
use crate::common::{console, paths};

#[derive(Default)]
struct Summary {
    active: TimeDelta,
    per_proc: HashMap<String, TimeDelta>,
    /// 当日のログ中、最後の abnormal_exit イベントの (from, to)
    last_abnormal: Option<(Option<Time>, Option<Time>)>,
}

impl Summary {
    fn add_active(&mut self, proc: &str, s: Time, e: Time, from: Time, to: Time) {
        let s = s.max(from);
        let e = e.min(to);
        if e > s {
            let d = e - s;
            self.active += d;
            *self.per_proc.entry(proc.to_string()).or_default() += d;
        }
    }
}

/// 当日の集計。`open` は記録中（またはクラッシュ時点）の区間とその終了とみなす時刻。
fn summarize(text: &str, from: Time, now: Time, open: Option<(&OpenSeg, Time)>) -> Summary {
    let mut sum = Summary::default();
    for line in text.lines() {
        let Ok(rec) = serde_json::from_str::<Record>(line) else { continue };
        match rec {
            Record::Seg(s) if s.st == St::Active => sum.add_active(&s.proc, s.s, s.e, from, now),
            Record::Event(e) if e.ev == "abnormal_exit" => sum.last_abnormal = Some((e.from, e.to)),
            _ => {}
        }
    }
    if let Some((o, end)) = open {
        if o.st == St::Active {
            sum.add_active(&o.proc, o.s, end, from, now);
        }
    }
    sum
}

fn read_checkpoint() -> Option<Checkpoint> {
    let text = std::fs::read_to_string(paths::checkpoint_path()).ok()?;
    serde_json::from_str(&text).ok()
}

struct State {
    now: Time,
    pid: Option<u32>,
    cp: Option<Checkpoint>,
    sum: Summary,
}

fn load() -> State {
    let now = clock::now();
    let from = clock::start_of_day(now.date_naive());
    let pid = find_daemon().map(|(_, p)| p);
    let cp = read_checkpoint();
    let text = std::fs::read_to_string(paths::log_file(now.date_naive())).unwrap_or_default();
    // 記録中なら現在まで、クラッシュしていれば最後のheartbeatまでを数える
    let open = cp.as_ref().and_then(|c| {
        let end = if pid.is_some() { now } else { c.heartbeat };
        c.open.as_ref().map(|o| (o, end))
    });
    let sum = summarize(&text, from, now, open);
    State { now, pid, cp, sum }
}

/// (一時停止中か, 残り時間)
fn pause_info(st: &State) -> Option<Option<TimeDelta>> {
    st.pid?;
    let cp = st.cp.as_ref()?;
    if !cp.paused {
        return None;
    }
    Some(cp.paused_until.map(|u| (u - st.now).max(TimeDelta::zero())))
}

fn fmt_remaining(d: TimeDelta) -> String {
    // 残り59秒を「0m」と出さないよう切り上げる
    clock::fmt_duration(d + TimeDelta::seconds(59))
}

pub fn status(brief: bool) -> i32 {
    let st = load();
    let active = clock::fmt_duration(st.sum.active);
    let pause = pause_info(&st);

    if brief {
        match (st.pid, pause) {
            (None, _) => {
                console::out("stopped");
                return 1;
            }
            (Some(_), None) => console::out(&format!("recording {active}")),
            (Some(_), Some(None)) => console::out(&format!("paused {active}")),
            (Some(_), Some(Some(rem))) => console::out(&format!("paused({} left) {active}", fmt_remaining(rem))),
        }
        return 0;
    }

    let state = match (st.pid, &pause) {
        (None, _) => "Stopped".to_string(),
        (Some(pid), None) => format!("Recording (PID {pid})"),
        (Some(pid), Some(_)) => format!("Paused (PID {pid})"),
    };
    let pause_line = match pause {
        None => "none".to_string(),
        Some(None) => "yes (until resumed)".to_string(),
        Some(Some(rem)) => format!("yes ({} left)", fmt_remaining(rem)),
    };
    let abnormal = if st.pid.is_none() && st.cp.is_some() {
        let hb = st.cp.as_ref().map(|c| c.heartbeat.format("%m/%d %H:%M").to_string()).unwrap_or_default();
        format!("yes (last heartbeat {hb}; it will be recorded on the next start)")
    } else {
        match &st.sum.last_abnormal {
            None => "none".to_string(),
            Some((from, to)) => {
                let f = |t: &Option<Time>| t.map(|t| t.format("%m/%d %H:%M").to_string()).unwrap_or_else(|| "?".into());
                format!("yes (no record from {} to {})", f(from), f(to))
            }
        }
    };

    console::out(&format!("State        : {state}"));
    console::out(&format!("Active today : {active}"));
    console::out(&format!("Paused       : {pause_line}"));
    console::out(&format!("Abnormal exit: {abnormal}"));
    if st.pid.is_some() {
        0
    } else {
        1
    }
}

pub fn today() -> i32 {
    let st = load();
    let mut rows: Vec<(String, TimeDelta)> = st.sum.per_proc.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    console::out(&format!("Active time per process, {} ({})", st.now.format("%Y-%m-%d"), paths::hostname()));
    if rows.is_empty() {
        console::out("  No records");
        return 0;
    }
    let total = st.sum.active;
    let width = rows.iter().map(|(p, _)| p.len().max(9)).max().unwrap_or(9).min(40);
    for (proc, d) in &rows {
        let name = if proc.is_empty() { "(unknown)" } else { proc.as_str() };
        let pct = if total.num_milliseconds() > 0 {
            d.num_milliseconds() as f64 * 100.0 / total.num_milliseconds() as f64
        } else {
            0.0
        };
        console::out(&format!("  {:<width$}  {:>6}  {:>5.1}%", name, clock::fmt_duration(*d), pct));
    }
    console::out(&format!("  {:<width$}  {:>6}", "Total", clock::fmt_duration(total)));
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Local, TimeZone};

    fn at(h: u32, m: u32) -> Time {
        Local.with_ymd_and_hms(2026, 9, 28, h, m, 0).unwrap()
    }

    #[test]
    fn sums_active_and_open() {
        let text = [
            r#"{"type":"header","schema":1,"app":"0.1.0","host":"H","user":"u","tz":"+09:00"}"#.to_string(),
            format!(r#"{{"type":"seg","s":"{}","e":"{}","st":"active","proc":"a.exe","title":"x"}}"#, clock::fmt(&at(9, 0)), clock::fmt(&at(10, 0))),
            format!(r#"{{"type":"seg","s":"{}","e":"{}","st":"idle","proc":"","title":""}}"#, clock::fmt(&at(10, 0)), clock::fmt(&at(10, 30))),
            format!(r#"{{"type":"event","t":"{}","ev":"abnormal_exit","from":"{}","to":"{}"}}"#, clock::fmt(&at(10, 30)), clock::fmt(&at(10, 0)), clock::fmt(&at(10, 30))),
            "broken line".to_string(),
        ]
        .join("\n");
        let open = OpenSeg { s: at(10, 30), st: St::Active, proc: "b.exe".into(), title: "y".into() };
        let sum = summarize(&text, at(0, 0), at(11, 0), Some((&open, at(11, 0))));
        assert_eq!(sum.active, TimeDelta::minutes(90));
        assert_eq!(sum.per_proc["a.exe"], TimeDelta::minutes(60));
        assert_eq!(sum.per_proc["b.exe"], TimeDelta::minutes(30));
        assert_eq!(sum.last_abnormal, Some((Some(at(10, 0)), Some(at(10, 30)))));
    }
}
