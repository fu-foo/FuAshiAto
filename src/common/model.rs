//! レコードの型定義（JSON Lines の1行と current.json）

use serde::{Deserialize, Serialize};

use super::clock::Time;

pub const SCHEMA: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum St {
    Active,
    Idle,
    Locked,
    Sleep,
    Paused,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Record {
    Header(Header),
    Seg(Seg),
    Event(Event),
}

impl Record {
    /// どの日のファイルに書くかを決める時刻（区間は開始時刻、イベントは発生時刻）
    pub fn time(&self) -> Option<Time> {
        match self {
            Record::Header(_) => None,
            Record::Seg(s) => Some(s.s),
            Record::Event(e) => Some(e.t),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Header {
    pub schema: u32,
    pub app: String,
    pub host: String,
    pub user: String,
    pub tz: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Seg {
    #[serde(with = "ts")]
    pub s: Time,
    #[serde(with = "ts")]
    pub e: Time,
    pub st: St,
    pub proc: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub end_unknown: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Event {
    #[serde(with = "ts")]
    pub t: Time,
    pub ev: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "ts_opt")]
    pub from: Option<Time>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "ts_opt")]
    pub to: Option<Time>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ms: Option<i64>,
}

impl Event {
    pub fn new(t: Time, ev: &str) -> Self {
        Event {
            t,
            ev: ev.to_string(),
            mode: None,
            reason: None,
            app: None,
            from: None,
            to: None,
            method: None,
            ms: None,
        }
    }
}

/// current.json の中身
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Checkpoint {
    #[serde(with = "ts")]
    pub heartbeat: Time,
    pub open: Option<OpenSeg>,
    /// 一時停止の期限。無期限の一時停止は `paused: true` かつ `null`
    #[serde(default, with = "ts_opt")]
    pub paused_until: Option<Time>,
    #[serde(default)]
    pub paused: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct OpenSeg {
    #[serde(with = "ts")]
    pub s: Time,
    pub st: St,
    pub proc: String,
    pub title: String,
}

fn is_false(b: &bool) -> bool {
    !*b
}

mod ts {
    use super::super::clock::{self, Time};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(t: &Time, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&clock::fmt(t))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Time, D::Error> {
        let s = String::deserialize(d)?;
        clock::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("bad time: {s}")))
    }
}

mod ts_opt {
    use super::super::clock::{self, Time};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(t: &Option<Time>, s: S) -> Result<S::Ok, S::Error> {
        match t {
            Some(t) => s.serialize_str(&clock::fmt(t)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Time>, D::Error> {
        match Option::<String>::deserialize(d)? {
            Some(s) => clock::parse(&s)
                .map(Some)
                .ok_or_else(|| serde::de::Error::custom(format!("bad time: {s}"))),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::clock;

    #[test]
    fn seg_json_shape() {
        let s = clock::parse("2026-09-28T09:12:03.412+09:00").unwrap();
        let e = clock::parse("2026-09-28T09:14:47.090+09:00").unwrap();
        let r = Record::Seg(Seg {
            s,
            e,
            st: St::Active,
            proc: "Code.exe".into(),
            title: "schema.rs".into(),
            end_unknown: false,
        });
        let j = r.to_json();
        assert!(j.starts_with(r#"{"type":"seg","s":""#), "{j}");
        assert!(j.contains(r#""st":"active","proc":"Code.exe","title":"schema.rs"}"#), "{j}");
        assert!(!j.contains("end_unknown"));
        let back: Record = serde_json::from_str(&j).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn event_json_shape() {
        let t = clock::parse("2026-09-28T10:00:12.003+09:00").unwrap();
        let mut ev = Event::new(t, "mic_on");
        ev.app = Some("MSTeams_8wekyb3d8bbwe".into());
        let j = Record::Event(ev).to_json();
        assert!(j.starts_with(r#"{"type":"event","t":""#), "{j}");
        assert!(j.ends_with(r#""ev":"mic_on","app":"MSTeams_8wekyb3d8bbwe"}"#), "{j}");
    }

    #[test]
    fn checkpoint_roundtrip() {
        let t = clock::parse("2026-09-28T10:03:00.000+09:00").unwrap();
        let c = Checkpoint { heartbeat: t, open: None, paused_until: None, paused: false };
        let j = serde_json::to_string(&c).unwrap();
        assert!(j.contains(r#""paused_until":null"#), "{j}");
        let back: Checkpoint = serde_json::from_str(&j).unwrap();
        assert_eq!(back, c);
    }
}
