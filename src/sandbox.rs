//! The Wasmer embedding: compile, check, run (record) and replay.
//!
//! Deliberately no WASI/WASIX: the guest only sees the tiny `terrarium` host
//! interface defined in [`crate::policy::HOST_SPECS`], and only the imports it
//! declared *and* the policy allows are ever wired up.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use wasmer::sys::{CompilerConfig, Cranelift};
use wasmer::wasmparser::Operator;
use wasmer::{
    Engine, Function, FunctionEnv, FunctionEnvMut, Imports, Instance, Memory, Module, RuntimeError,
    Store,
};
use wasmer_middlewares::Metering;
use wasmer_middlewares::metering::{MeteringPoints, get_remaining_points};

use crate::diagnostics::{Divergence, Report};
use crate::journal::{self, FORMAT_VERSION, Journal, Outcome, Tape};
use crate::policy::{self, Budget, Manifest, NAMESPACE, Policy};

/// Boxed so `Result<_, Failure>` stays small; deref it to get the [`Report`].
pub type Failure = Box<Report>;

/// Name of the entry point every guest must export: `() -> i64`.
pub const ENTRY: &str = "run";

// ---------------------------------------------------------------------------
// Host state and host functions
// ---------------------------------------------------------------------------

enum Fault {
    Divergence(Divergence),
    Message(String),
}

#[derive(Default)]
struct HostState {
    memory: Option<Memory>,
    tape: Tape,
    output: String,
    max_output: usize,
    fault: Option<Fault>,
}

fn fail<T>(state: &mut HostState, msg: impl Into<String>) -> Result<T, RuntimeError> {
    let msg = msg.into();
    state.fault = Some(Fault::Message(msg.clone()));
    Err(RuntimeError::new(msg))
}

fn diverge<T>(state: &mut HostState, d: Divergence) -> Result<T, RuntimeError> {
    state.fault = Some(Fault::Divergence(d));
    Err(RuntimeError::new("replay diverged from the recording"))
}

fn host_log(mut env: FunctionEnvMut<HostState>, ptr: i32, len: i32) -> Result<(), RuntimeError> {
    let (state, store) = env.data_and_store_mut();
    if ptr < 0 || len < 0 {
        return fail(state, "log: negative pointer or length");
    }
    let len = len as usize;
    if state.output.len() + len > state.max_output {
        return fail(state, "log: output limit exceeded");
    }
    let Some(memory) = state.memory.clone() else {
        return fail(state, "log: guest does not export a `memory`");
    };
    let mut buf = vec![0u8; len];
    if memory.view(&store).read(ptr as u64, &mut buf).is_err() {
        return fail(state, "log: pointer/length outside guest memory");
    }
    let text = String::from_utf8_lossy(&buf).into_owned();
    if let Err(d) = state.tape.output("terrarium.log", text.clone()) {
        return diverge(state, d);
    }
    state.output.push_str(&text);
    Ok(())
}

fn host_clock_now(mut env: FunctionEnvMut<HostState>) -> Result<i64, RuntimeError> {
    let state = env.data_mut();
    let live = || {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    };
    match state.tape.input("terrarium.clock_now", live) {
        Ok(v) => Ok(v),
        Err(d) => diverge(state, d),
    }
}

fn host_rand_u64(mut env: FunctionEnvMut<HostState>) -> Result<i64, RuntimeError> {
    let state = env.data_mut();
    let live = || {
        let mut b = [0u8; 8];
        getrandom::fill(&mut b).expect("operating system RNG unavailable");
        i64::from_le_bytes(b)
    };
    match state.tape.input("terrarium.rand_u64", live) {
        Ok(v) => Ok(v),
        Err(d) => diverge(state, d),
    }
}

// ---------------------------------------------------------------------------
// Prepared module
// ---------------------------------------------------------------------------

/// A compiled module whose import list passed the policy check.
///
/// Compilation is the expensive step; [`Prepared::run`] can be called many
/// times on the same value (each call gets a fresh store and instance).
pub struct Prepared {
    engine: Engine,
    module: Module,
    pub manifest: Manifest,
    pub wasm_sha256: String,
    pub budget: Budget,
}

fn metered_engine(fuel: u64) -> Engine {
    // One metering instance per module: the middleware must not be shared.
    let mut compiler = Cranelift::default();
    compiler.push_middleware(Arc::new(Metering::new(fuel, |_op: &Operator| 1u64)));
    compiler.into()
}

fn compile(wasm: &[u8], fuel: u64) -> Result<(Engine, Module, Manifest), Failure> {
    let engine = metered_engine(fuel);
    let module = Module::new(&engine, wasm)
        .map_err(|e| Failure::from(Report::invalid_module(e.to_string())))?;
    let imports: Vec<(String, String)> = module
        .imports()
        .map(|i| (i.module().to_string(), i.name().to_string()))
        .collect();
    let manifest = Manifest::from_imports(imports.iter().map(|(m, n)| (m.as_str(), n.as_str())));
    Ok((engine, module, manifest))
}

/// Read a module's requested rights without running or policy-checking it.
pub fn inspect(wasm: &[u8]) -> Result<Manifest, Failure> {
    compile(wasm, 0).map(|(_, _, m)| m)
}

impl Prepared {
    /// Compile `wasm` and check its imports against `policy`.
    ///
    /// On denial no guest code has run; the returned [`Report`] says so
    /// (`executed: false`).
    pub fn new(wasm: &[u8], policy: &Policy) -> Result<Self, Failure> {
        let (engine, module, manifest) = compile(wasm, policy.budget.fuel)?;
        let (code, violations) = policy::check(&manifest, policy);
        if !violations.is_empty() {
            return Err(Report::denied(code, violations).into());
        }
        Ok(Prepared {
            engine,
            module,
            manifest,
            wasm_sha256: journal::sha256_hex(wasm),
            budget: policy.budget.clone(),
        })
    }

    /// Instantiate in a fresh store and call the entry point.
    ///
    /// `Err` means the module could not be started (nothing ran). `Ok` means
    /// guest code executed; inspect [`RunRecord::outcome`].
    pub fn run(&self, tape: Tape) -> Result<RunRecord, Failure> {
        let mut store = Store::new(self.engine.clone());
        let env = FunctionEnv::new(
            &mut store,
            HostState {
                memory: None,
                tape,
                output: String::new(),
                max_output: self.budget.max_output_bytes,
                fault: None,
            },
        );

        // Wire up exactly the imports the module declared. They have all been
        // checked against the policy in `Prepared::new`.
        let mut imports = Imports::new();
        for entry in &self.manifest.imports {
            let f = match entry.name.as_str() {
                "log" => Function::new_typed_with_env(&mut store, &env, host_log),
                "clock_now" => Function::new_typed_with_env(&mut store, &env, host_clock_now),
                "rand_u64" => Function::new_typed_with_env(&mut store, &env, host_rand_u64),
                other => {
                    return Err(
                        Report::link_error(format!("no implementation for `{other}`")).into(),
                    );
                }
            };
            imports.define(NAMESPACE, &entry.name, f);
        }

        let instance = match Instance::new(&mut store, &self.module, &imports) {
            Ok(i) => i,
            Err(wasmer::InstantiationError::Start(e)) => {
                return Err(Report::start_trap(e.message()).into());
            }
            Err(e) => return Err(Report::link_error(e.to_string()).into()),
        };
        env.as_mut(&mut store).memory = instance.exports.get_memory("memory").ok().cloned();

        let Ok(entry) = instance
            .exports
            .get_typed_function::<(), i64>(&store, ENTRY)
        else {
            return Err(Report::missing_export(ENTRY).into());
        };

        let call = entry.call(&mut store);
        let remaining = get_remaining_points(&mut store, &instance);

        let state = env.as_mut(&mut store);
        let tape = std::mem::take(&mut state.tape);
        let output = std::mem::take(&mut state.output);
        let fault = state.fault.take();
        let unconsumed = tape.finish_replay().err();
        let host_calls = tape.calls();

        let (exhausted, fuel_used) = match remaining {
            MeteringPoints::Remaining(left) => (false, self.budget.fuel.saturating_sub(left)),
            MeteringPoints::Exhausted => (true, self.budget.fuel),
        };

        // A fault raised by a host function is the root cause, so it wins over
        // the generic trap it caused; fuel exhaustion wins over a plain trap.
        let (outcome, result, trap_message, divergence) = match (call, fault) {
            (_, Some(Fault::Divergence(d))) => (Outcome::Trap, None, None, Some(d)),
            (_, Some(Fault::Message(m))) => (Outcome::Trap, None, Some(m), None),
            (Ok(v), None) => (Outcome::Returned, Some(v), None, None),
            (Err(_), None) if exhausted => (Outcome::FuelExhausted, None, None, None),
            (Err(e), None) => (Outcome::Trap, None, Some(e.message()), None),
        };

        Ok(RunRecord {
            outcome,
            result,
            trap_message,
            divergence,
            unconsumed,
            output,
            fuel_used,
            host_calls,
            events: tape.into_events(),
        })
    }
}

/// Everything that happened in one execution.
pub struct RunRecord {
    pub outcome: Outcome,
    pub result: Option<i64>,
    pub trap_message: Option<String>,
    /// Set if a replay deviated while the guest was running.
    pub divergence: Option<Divergence>,
    /// Set after a replay if recorded events were never consumed.
    pub unconsumed: Option<Divergence>,
    pub output: String,
    pub fuel_used: u64,
    pub host_calls: usize,
    pub events: Vec<journal::Event>,
}

impl RunRecord {
    fn into_report(self, fuel_limit: u64) -> Report {
        match self.outcome {
            Outcome::Returned => Report::ok(
                self.result.unwrap_or_default(),
                self.output,
                self.fuel_used,
                self.host_calls,
            ),
            Outcome::FuelExhausted => {
                Report::fuel_exhausted(fuel_limit, self.output, self.host_calls)
            }
            Outcome::Trap => Report::trap(
                self.trap_message.unwrap_or_else(|| "guest trapped".into()),
                self.output,
                self.host_calls,
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Record and replay
// ---------------------------------------------------------------------------

/// Run `wasm` under `policy` and record every host interaction.
///
/// The journal is `Some` whenever guest code executed (including traps and
/// fuel exhaustion) and `None` for static rejections.
pub fn record(wasm: &[u8], policy: &Policy) -> (Report, Option<Journal>) {
    let prepared = match Prepared::new(wasm, policy) {
        Ok(p) => p,
        Err(report) => return (*report, None),
    };
    let rec = match prepared.run(Tape::recording()) {
        Ok(rec) => rec,
        Err(report) => return (*report, None),
    };
    let journal = Journal {
        format: FORMAT_VERSION,
        module_sha256: prepared.wasm_sha256.clone(),
        fuel_limit: policy.budget.fuel,
        max_output_bytes: policy.budget.max_output_bytes,
        granted: prepared.manifest.capabilities.iter().cloned().collect(),
        events: rec.events.clone(),
        outcome: rec.outcome,
        result: rec.result,
    };
    (rec.into_report(policy.budget.fuel), Some(journal))
}

/// Re-run `wasm` against `journal` and verify it behaves identically.
///
/// The guest runs again, but every nondeterministic answer comes from the
/// journal and no capability beyond the recorded grants is available.
pub fn replay(wasm: &[u8], journal: &Journal) -> Report {
    let sha = journal::sha256_hex(wasm);
    if sha != journal.module_sha256 {
        return Report::module_mismatch(&journal.module_sha256, &sha);
    }
    let policy = Policy {
        name: "replay".into(),
        allow: journal.granted.iter().cloned().collect(),
        budget: Budget {
            fuel: journal.fuel_limit,
            max_output_bytes: journal.max_output_bytes,
        },
    };
    let prepared = match Prepared::new(wasm, &policy) {
        Ok(p) => p,
        Err(report) => return *report,
    };
    let rec = match prepared.run(Tape::replaying(journal.events.clone())) {
        Ok(rec) => rec,
        Err(report) => return *report,
    };

    if let Some(d) = rec.divergence {
        return Report::divergence(d, true);
    }
    if rec.outcome != journal.outcome {
        return Report::divergence(
            Divergence {
                kind: "outcome_mismatch".into(),
                seq: None,
                expected: format!("{:?}", journal.outcome),
                actual: format!("{:?}", rec.outcome),
            },
            true,
        );
    }
    if rec.result != journal.result {
        return Report::divergence(
            Divergence {
                kind: "result_mismatch".into(),
                seq: None,
                expected: format!("{:?}", journal.result),
                actual: format!("{:?}", rec.result),
            },
            true,
        );
    }
    if let Some(d) = rec.unconsumed {
        return Report::divergence(d, true);
    }
    Report::replay_verified(rec.result, rec.host_calls)
}
