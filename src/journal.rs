//! Flight recorder: every host interaction of a run, in order.
//!
//! In *record* mode the host answers nondeterministic calls (clock, random)
//! for real and writes the answers to the journal. In *replay* mode the host
//! answers from the journal instead and checks that the guest makes the same
//! calls in the same order and produces the same output and result.
//!
//! Scope: this only gives reproducibility for guests whose nondeterminism
//! enters exclusively through Terrarium's imports (single-threaded, no WASI).

use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::diagnostics::Divergence;

pub const FORMAT_VERSION: u32 = 1;

/// SHA-256 of the module binary, lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// One host call made by the guest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Event {
    pub seq: u64,
    /// Fully qualified, e.g. `terrarium.clock_now`.
    pub import: String,
    /// Value returned to the guest (clock, random).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<i64>,
    /// Text emitted by the guest (log).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// How the recorded run ended.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Returned,
    Trap,
    FuelExhausted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Journal {
    pub format: u32,
    pub module_sha256: String,
    pub fuel_limit: u64,
    pub max_output_bytes: usize,
    /// Capabilities that were granted during the recording.
    pub granted: Vec<String>,
    pub events: Vec<Event>,
    pub outcome: Outcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<i64>,
}

impl Journal {
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json).with_context(|| format!("writing {}", path.display()))
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let j: Journal = serde_json::from_str(&text).context("parsing journal JSON")?;
        anyhow::ensure!(
            j.format == FORMAT_VERSION,
            "unsupported journal format {} (expected {FORMAT_VERSION})",
            j.format
        );
        Ok(j)
    }
}

/// The tape the host functions read from or write to.
pub enum Tape {
    Record { events: Vec<Event> },
    Replay { events: Vec<Event>, pos: usize },
}

impl Default for Tape {
    fn default() -> Self {
        Tape::Record { events: Vec::new() }
    }
}

impl Tape {
    pub fn recording() -> Self {
        Tape::Record { events: Vec::new() }
    }

    pub fn replaying(events: Vec<Event>) -> Self {
        Tape::Replay { events, pos: 0 }
    }

    /// Number of host calls seen so far.
    pub fn calls(&self) -> usize {
        match self {
            Tape::Record { events } => events.len(),
            Tape::Replay { pos, .. } => *pos,
        }
    }

    /// A nondeterministic input: live value when recording, recorded value
    /// when replaying.
    pub fn input(&mut self, import: &str, live: impl FnOnce() -> i64) -> Result<i64, Divergence> {
        match self {
            Tape::Record { events } => {
                let value = live();
                let seq = events.len() as u64;
                events.push(Event {
                    seq,
                    import: import.into(),
                    value: Some(value),
                    text: None,
                });
                Ok(value)
            }
            Tape::Replay { events, pos } => {
                let ev = next_event(events, pos, import)?;
                ev.value.ok_or_else(|| Divergence {
                    kind: "event_mismatch".into(),
                    seq: Some(ev.seq),
                    expected: format!("{} with a value", ev.import),
                    actual: "recorded event has no value".into(),
                })
            }
        }
    }

    /// An output of the guest: recorded when recording, compared when
    /// replaying.
    pub fn output(&mut self, import: &str, text: String) -> Result<(), Divergence> {
        match self {
            Tape::Record { events } => {
                let seq = events.len() as u64;
                events.push(Event {
                    seq,
                    import: import.into(),
                    value: None,
                    text: Some(text),
                });
                Ok(())
            }
            Tape::Replay { events, pos } => {
                let ev = next_event(events, pos, import)?;
                let expected = ev.text.clone().unwrap_or_default();
                if expected != text {
                    return Err(Divergence {
                        kind: "output_mismatch".into(),
                        seq: Some(ev.seq),
                        expected,
                        actual: text,
                    });
                }
                Ok(())
            }
        }
    }

    /// After a finished replay: were all recorded events consumed?
    pub fn finish_replay(&self) -> Result<(), Divergence> {
        if let Tape::Replay { events, pos } = self
            && *pos < events.len()
        {
            let ev = &events[*pos];
            return Err(Divergence {
                kind: "journal_unconsumed".into(),
                seq: Some(ev.seq),
                expected: format!("further call to {}", ev.import),
                actual: "guest finished without making it".into(),
            });
        }
        Ok(())
    }

    pub fn into_events(self) -> Vec<Event> {
        match self {
            Tape::Record { events } | Tape::Replay { events, .. } => events,
        }
    }
}

fn next_event<'a>(
    events: &'a [Event],
    pos: &mut usize,
    import: &str,
) -> Result<&'a Event, Divergence> {
    let Some(ev) = events.get(*pos) else {
        return Err(Divergence {
            kind: "journal_exhausted".into(),
            seq: Some(*pos as u64),
            expected: "no further host calls".into(),
            actual: format!("guest called {import}"),
        });
    };
    if ev.import != import {
        return Err(Divergence {
            kind: "event_mismatch".into(),
            seq: Some(ev.seq),
            expected: ev.import.clone(),
            actual: import.into(),
        });
    }
    *pos += 1;
    Ok(ev)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_then_replay_roundtrip() {
        let mut rec = Tape::recording();
        assert_eq!(rec.input("terrarium.clock_now", || 41).unwrap(), 41);
        rec.output("terrarium.log", "hi".into()).unwrap();
        let events = rec.into_events();

        let mut rep = Tape::replaying(events);
        assert_eq!(rep.input("terrarium.clock_now", || 999).unwrap(), 41);
        rep.output("terrarium.log", "hi".into()).unwrap();
        rep.finish_replay().unwrap();
    }

    #[test]
    fn replay_detects_wrong_call_order() {
        let mut rec = Tape::recording();
        rec.input("terrarium.clock_now", || 1).unwrap();
        let mut rep = Tape::replaying(rec.into_events());
        let err = rep.input("terrarium.rand_u64", || 0).unwrap_err();
        assert_eq!(err.kind, "event_mismatch");
    }

    #[test]
    fn replay_detects_unconsumed_events() {
        let mut rec = Tape::recording();
        rec.input("terrarium.clock_now", || 1).unwrap();
        let rep = Tape::replaying(rec.into_events());
        assert_eq!(rep.finish_replay().unwrap_err().kind, "journal_unconsumed");
    }

    #[test]
    fn sha256_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
