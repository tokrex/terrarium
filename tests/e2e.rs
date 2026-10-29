use std::path::{Path, PathBuf};

use terrarium::diagnostics::Code;
use terrarium::journal::Outcome;
use terrarium::policy::Policy;
use terrarium::{Journal, load_module_bytes, sandbox};

fn guest(name: &str) -> Vec<u8> {
    let p: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("guests")
        .join(name);
    load_module_bytes(&p).unwrap_or_else(|e| panic!("{name}: {e:#}"))
}

fn demo_policy() -> Policy {
    Policy::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("policy.toml")).unwrap()
}

fn recorded_clock_rand() -> (Vec<u8>, Journal) {
    let wasm = guest("clock_rand.wat");
    let (report, journal) = sandbox::record(&wasm, &demo_policy());
    assert_eq!(report.status, Code::Ok, "{}", report.to_json_pretty());
    (wasm, journal.expect("journal for an executed run"))
}

// --- static capability checks -------------------------------------------

#[test]
fn network_request_is_denied_before_anything_runs() {
    let (report, journal) = sandbox::record(&guest("denied_net.wat"), &demo_policy());
    assert_eq!(report.status, Code::CapDenied);
    assert!(
        !report.executed,
        "static denial must not execute guest code"
    );
    assert_eq!(report.violations.len(), 1);
    assert_eq!(report.violations[0].import, "terrarium.net_connect");
    assert_eq!(report.violations[0].capability.as_deref(), Some("net"));
    assert!(!report.violations[0].repair_hint.is_empty());
    assert!(journal.is_none());
}

#[test]
fn repaired_version_runs() {
    let (report, journal) = sandbox::record(&guest("fixed_no_net.wat"), &demo_policy());
    assert_eq!(report.status, Code::Ok);
    assert_eq!(
        report.output.as_deref(),
        Some("data passed in, no network needed")
    );
    assert_eq!(journal.unwrap().events.len(), 1);
}

#[test]
fn pure_module_runs_under_tier0_but_capabilities_do_not() {
    let (ok, _) = sandbox::record(&guest("pure_add.wat"), &Policy::tier0());
    assert_eq!(ok.status, Code::Ok);
    assert_eq!(ok.result, Some(42));

    let (denied, _) = sandbox::record(&guest("clock_rand.wat"), &Policy::tier0());
    assert_eq!(denied.status, Code::CapDenied);
    assert_eq!(denied.violations.len(), 3);
}

#[test]
fn wasi_imports_are_unknown_not_silently_stubbed() {
    let (report, _) = sandbox::record(&guest("wasi_hello.wat"), &demo_policy());
    assert_eq!(report.status, Code::CapUnknownImport);
    assert!(!report.executed);
}

#[test]
fn garbage_and_missing_entry_point_are_diagnosed() {
    let (report, _) = sandbox::record(b"not wasm at all", &demo_policy());
    assert_eq!(report.status, Code::InvalidModule);

    let empty = wasmer::wat2wasm(b"(module)").unwrap();
    let (report, _) = sandbox::record(&empty, &demo_policy());
    assert_eq!(report.status, Code::MissingExport);
    assert!(!report.executed);
}

// --- budgets and traps ---------------------------------------------------

#[test]
fn fuel_exhaustion_is_reported_and_replays_deterministically() {
    let wasm = guest("spin.wat");
    let mut policy = demo_policy();
    policy.budget.fuel = 10_000;
    let (report, journal) = sandbox::record(&wasm, &policy);
    assert_eq!(report.status, Code::FuelExhausted);
    assert!(report.executed);

    let journal = journal.unwrap();
    assert_eq!(journal.outcome, Outcome::FuelExhausted);
    assert_eq!(
        sandbox::replay(&wasm, &journal).status,
        Code::ReplayVerified
    );
}

#[test]
fn runaway_start_function_cannot_hang_instantiation() {
    let wasm = wasmer::wat2wasm(
        br#"(module
              (func $s (loop $l (br $l)))
              (start $s)
              (func (export "run") (result i64) (i64.const 0)))"#,
    )
    .unwrap();
    let mut policy = demo_policy();
    policy.budget.fuel = 10_000;
    let (report, journal) = sandbox::record(&wasm, &policy);
    assert_eq!(report.status, Code::Trap);
    assert!(report.executed, "start function is guest code");
    assert_eq!(report.stage.as_deref(), Some("instantiate"));
    assert!(journal.is_none());
}

#[test]
fn out_of_bounds_access_is_a_trap() {
    let (report, journal) = sandbox::record(&guest("trap_oob.wat"), &demo_policy());
    assert_eq!(report.status, Code::Trap);
    assert_eq!(journal.unwrap().outcome, Outcome::Trap);
}

#[test]
fn output_limit_is_enforced() {
    let mut policy = demo_policy();
    policy.budget.max_output_bytes = 5;
    let (report, _) = sandbox::record(&guest("fixed_no_net.wat"), &policy);
    assert_eq!(report.status, Code::Trap);
    assert!(report.message.unwrap().contains("output limit"));
}

// --- journal and replay --------------------------------------------------

#[test]
fn record_then_replay_is_verified() {
    let (wasm, journal) = recorded_clock_rand();
    assert_eq!(journal.events.len(), 3); // log, clock_now, rand_u64
    let report = sandbox::replay(&wasm, &journal);
    assert_eq!(
        report.status,
        Code::ReplayVerified,
        "{}",
        report.to_json_pretty()
    );
    assert_eq!(report.result, journal.result);
}

#[test]
fn replay_survives_a_trip_through_a_file() {
    let (wasm, journal) = recorded_clock_rand();
    let path = std::env::temp_dir().join(format!("terrarium-{}.json", std::process::id()));
    journal.save(&path).unwrap();
    let loaded = Journal::load(&path).unwrap();
    std::fs::remove_file(&path).ok();
    assert_eq!(sandbox::replay(&wasm, &loaded).status, Code::ReplayVerified);
}

#[test]
fn replay_uses_recorded_values_not_the_live_clock() {
    let (wasm, journal) = recorded_clock_rand();
    std::thread::sleep(std::time::Duration::from_millis(20));
    // Still verified 20 ms later: the clock answer came from the journal.
    assert_eq!(
        sandbox::replay(&wasm, &journal).status,
        Code::ReplayVerified
    );
}

#[test]
fn tampered_input_value_is_detected() {
    let (wasm, mut journal) = recorded_clock_rand();
    let ev = journal
        .events
        .iter_mut()
        .find(|e| e.import == "terrarium.clock_now")
        .unwrap();
    ev.value = Some(ev.value.unwrap() + 1);
    let report = sandbox::replay(&wasm, &journal);
    assert_eq!(report.status, Code::ReplayDivergence);
    assert_eq!(report.divergence.unwrap().kind, "result_mismatch");
}

#[test]
fn reordered_events_are_detected_with_position() {
    let (wasm, mut journal) = recorded_clock_rand();
    journal.events.swap(1, 2);
    let report = sandbox::replay(&wasm, &journal);
    assert_eq!(report.status, Code::ReplayDivergence);
    let d = report.divergence.unwrap();
    assert_eq!(d.kind, "event_mismatch");
    assert_eq!(d.expected, "terrarium.rand_u64");
    assert_eq!(d.actual, "terrarium.clock_now");
}

#[test]
fn changed_output_is_detected() {
    let (wasm, mut journal) = recorded_clock_rand();
    journal.events[0].text = Some("something else".into());
    let d = sandbox::replay(&wasm, &journal).divergence.unwrap();
    assert_eq!(d.kind, "output_mismatch");
    assert_eq!(d.seq, Some(0));
}

#[test]
fn extra_recorded_events_are_detected() {
    let (wasm, mut journal) = recorded_clock_rand();
    let mut extra = journal.events[1].clone();
    extra.seq = 3;
    journal.events.push(extra);
    let d = sandbox::replay(&wasm, &journal).divergence.unwrap();
    assert_eq!(d.kind, "journal_unconsumed");
}

#[test]
fn journal_for_another_module_is_rejected() {
    let (_, journal) = recorded_clock_rand();
    let report = sandbox::replay(&guest("pure_add.wat"), &journal);
    assert_eq!(report.status, Code::ReplayModuleMismatch);
    assert!(!report.executed);
}

// --- rights diff ---------------------------------------------------------

#[test]
fn rights_diff_distinguishes_widening_from_narrowing() {
    use terrarium::policy::{Verdict, diff};
    let fixed = sandbox::inspect(&guest("fixed_no_net.wat")).unwrap();
    let denied = sandbox::inspect(&guest("denied_net.wat")).unwrap();

    let narrowing = diff(&denied, &fixed);
    assert_eq!(narrowing.verdict, Verdict::AutoApprove);
    assert_eq!(narrowing.removed, vec!["net".to_string()]);

    let widening = diff(&fixed, &denied);
    assert_eq!(widening.verdict, Verdict::NeedsApproval);
    assert_eq!(widening.added, vec!["net".to_string()]);
}
