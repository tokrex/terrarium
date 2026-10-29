//! Structured, agent-readable diagnostics.
//!
//! Every outcome of a Terrarium call is a [`Report`] with a stable
//! machine-readable [`Code`]. The goal is that an agent can branch on `status`
//! and act on `repair_hint` without parsing free-form error text.

use serde::{Deserialize, Serialize};

/// Stable status codes. Treat the string form as part of the public contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Code {
    Ok,
    /// Module imports a known capability that the policy does not grant.
    CapDenied,
    /// Module imports something Terrarium does not provide at all (e.g. WASI).
    CapUnknownImport,
    /// Bytes are not a valid WebAssembly module.
    InvalidModule,
    /// Instantiation failed (e.g. import signature mismatch).
    LinkError,
    /// The module does not export the expected `run` function.
    MissingExport,
    /// The fuel budget was exhausted.
    FuelExhausted,
    /// The guest trapped (out-of-bounds access, unreachable, ...).
    Trap,
    /// Replay finished and matched the journal exactly.
    ReplayVerified,
    /// Replay behaved differently from the recording.
    ReplayDivergence,
    /// Journal was recorded for a different module binary.
    ReplayModuleMismatch,
    /// Journal could not be parsed or has an unsupported format.
    JournalInvalid,
}

impl Code {
    /// Process exit code for the CLI: 0 on success, 1 on a diagnosed failure.
    pub fn exit_code(self) -> i32 {
        match self {
            Code::Ok | Code::ReplayVerified => 0,
            _ => 1,
        }
    }
}

/// One concrete reason a module was rejected.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Violation {
    pub import: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capability: Option<String>,
    pub reason: String,
    pub repair_hint: String,
}

/// Where and how a replay deviated from the recording.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Divergence {
    /// `event_mismatch`, `journal_exhausted`, `journal_unconsumed`,
    /// `output_mismatch`, `result_mismatch` or `outcome_mismatch`.
    pub kind: String,
    /// Index of the host-call event, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    pub expected: String,
    pub actual: String,
}

/// The single result type printed by the CLI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub status: Code,
    /// `true` if any guest instruction ran. Static denials report `false`.
    pub executed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub violations: Vec<Violation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub divergence: Option<Divergence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fuel_used: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_calls: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repair_hint: Option<String>,
}

impl Report {
    fn base(status: Code, executed: bool) -> Self {
        Report {
            status,
            executed,
            stage: None,
            message: None,
            violations: vec![],
            divergence: None,
            result: None,
            output: None,
            fuel_used: None,
            host_calls: None,
            repair_hint: None,
        }
    }

    pub fn ok(result: i64, output: String, fuel_used: u64, host_calls: usize) -> Self {
        Report {
            result: Some(result),
            output: Some(output),
            fuel_used: Some(fuel_used),
            host_calls: Some(host_calls),
            ..Self::base(Code::Ok, true)
        }
    }

    /// Static rejection: no guest code ran.
    pub fn denied(code: Code, violations: Vec<Violation>) -> Self {
        Report {
            stage: Some("static".into()),
            message: Some(format!(
                "module requests {} import(s) that the policy does not provide",
                violations.len()
            )),
            violations,
            ..Self::base(code, false)
        }
    }

    pub fn invalid_module(msg: impl Into<String>) -> Self {
        Report {
            stage: Some("static".into()),
            message: Some(msg.into()),
            repair_hint: Some("Provide a valid core WebAssembly module (.wasm or .wat).".into()),
            ..Self::base(Code::InvalidModule, false)
        }
    }

    pub fn link_error(msg: impl Into<String>) -> Self {
        Report {
            stage: Some("instantiate".into()),
            message: Some(msg.into()),
            repair_hint: Some(
                "Check that imported host functions use the signatures documented in README.md."
                    .into(),
            ),
            ..Self::base(Code::LinkError, false)
        }
    }

    /// The module's `start` function ran during instantiation and trapped
    /// (this includes running out of fuel there). Guest code did execute.
    pub fn start_trap(msg: impl Into<String>) -> Self {
        Report {
            stage: Some("instantiate".into()),
            message: Some(format!(
                "start function trapped or exhausted its fuel: {}",
                msg.into()
            )),
            repair_hint: Some(
                "Avoid long-running work in a `start` function; it is metered like `run`.".into(),
            ),
            ..Self::base(Code::Trap, true)
        }
    }

    pub fn missing_export(name: &str) -> Self {
        Report {
            stage: Some("instantiate".into()),
            message: Some(format!("module does not export `{name}` as `() -> i64`")),
            repair_hint: Some(format!("Export a function `{name}` with type `() -> i64`.")),
            ..Self::base(Code::MissingExport, false)
        }
    }

    pub fn fuel_exhausted(limit: u64, output: String, host_calls: usize) -> Self {
        Report {
            stage: Some("run".into()),
            message: Some(format!("fuel budget of {limit} was exhausted")),
            output: Some(output),
            fuel_used: Some(limit),
            host_calls: Some(host_calls),
            repair_hint: Some(
                "Reduce the work per call, or ask for a larger budget in the policy.".into(),
            ),
            ..Self::base(Code::FuelExhausted, true)
        }
    }

    pub fn trap(msg: impl Into<String>, output: String, host_calls: usize) -> Self {
        Report {
            stage: Some("run".into()),
            message: Some(msg.into()),
            output: Some(output),
            host_calls: Some(host_calls),
            ..Self::base(Code::Trap, true)
        }
    }

    pub fn replay_verified(result: Option<i64>, host_calls: usize) -> Self {
        Report {
            stage: Some("replay".into()),
            message: Some("replay matched the recording".into()),
            result,
            host_calls: Some(host_calls),
            ..Self::base(Code::ReplayVerified, true)
        }
    }

    pub fn divergence(d: Divergence, executed: bool) -> Self {
        Report {
            stage: Some("replay".into()),
            message: Some(format!("replay diverged from the recording ({})", d.kind)),
            divergence: Some(d),
            ..Self::base(Code::ReplayDivergence, executed)
        }
    }

    pub fn module_mismatch(expected: &str, actual: &str) -> Self {
        Report {
            stage: Some("replay".into()),
            message: Some(format!(
                "journal was recorded for module sha256 {expected}, got {actual}"
            )),
            repair_hint: Some("Replay against the exact binary that produced the journal.".into()),
            ..Self::base(Code::ReplayModuleMismatch, false)
        }
    }

    pub fn journal_invalid(msg: impl Into<String>) -> Self {
        Report {
            stage: Some("replay".into()),
            message: Some(msg.into()),
            ..Self::base(Code::JournalInvalid, false)
        }
    }

    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).expect("Report is always serializable")
    }
}
