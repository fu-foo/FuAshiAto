//! current.json の読み書きと、起動時の異常終了の検出。

use crate::common::clock::Time;
use crate::common::model::{Checkpoint, Event, OpenSeg, Record};
use crate::common::{errlog, paths};

use super::recorder::{emit_split, Win};

/// current.json を書く（一時ファイルに書いてから置き換える）
pub fn write(heartbeat: Time, open: Option<OpenSeg>, paused: bool, paused_until: Option<Time>) {
    let cp = Checkpoint { heartbeat, open, paused_until, paused };
    let json = match serde_json::to_string(&cp) {
        Ok(j) => j,
        Err(e) => {
            errlog::log(&format!("serialize checkpoint: {e}"));
            return;
        }
    };
    let path = paths::checkpoint_path();
    let tmp = path.with_extension("json.tmp");
    let result = std::fs::write(&tmp, json).and_then(|_| std::fs::rename(&tmp, &path));
    if let Err(e) = result {
        errlog::log(&format!("write checkpoint: {e}"));
    }
}

pub fn read() -> Option<Checkpoint> {
    let text = std::fs::read_to_string(paths::checkpoint_path()).ok()?;
    match serde_json::from_str(&text) {
        Ok(c) => Some(c),
        Err(e) => {
            errlog::log(&format!("parse checkpoint: {e}"));
            None
        }
    }
}

pub fn remove() {
    let path = paths::checkpoint_path();
    if path.exists() {
        if let Err(e) = std::fs::remove_file(&path) {
            errlog::log(&format!("remove checkpoint: {e}"));
        }
    }
}

/// 起動時の復旧結果
pub struct Recovery {
    pub records: Vec<Record>,
    /// 復元すべき一時停止（`Some(None)` は無期限）
    pub pause: Option<Option<Time>>,
}

/// current.json が残っていれば前回は異常終了とみなし、欠損を記録するレコードを作る
pub fn recover(now: Time) -> Recovery {
    let mut rec = Recovery { records: Vec::new(), pause: None };
    let path = paths::checkpoint_path();
    if !path.exists() {
        return rec;
    }
    let Some(cp) = read() else {
        // 壊れている場合でも異常終了の事実は残す
        let mut ev = Event::new(now, "abnormal_exit");
        ev.to = Some(now);
        rec.records.push(Record::Event(ev));
        remove();
        return rec;
    };

    if let Some(o) = &cp.open {
        emit_split(&mut rec.records, o.s, cp.heartbeat, o.st, &Win::new(&o.proc, &o.title), true);
    }
    let mut ev = Event::new(now, "abnormal_exit");
    ev.from = Some(cp.heartbeat);
    ev.to = Some(now);
    rec.records.push(Record::Event(ev));

    if cp.paused {
        match cp.paused_until {
            None => rec.pause = Some(None),
            Some(u) if u > now => rec.pause = Some(Some(u)),
            _ => {}
        }
    }
    rec
}
