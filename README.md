# Terrarium

An experiment in agent sandboxing on [Wasmer](https://github.com/wasmerio/wasmer).

Instead of fencing code in, Terrarium builds a tiny world per call. The module's
import list is read as a **capability manifest** and checked against a policy
*before* any guest code runs. Every host interaction is **journaled** so a run
can be replayed and verified. Denials come back as **structured, repairable
diagnostics**, so the sandbox becomes part of an agent's compile-and-fix loop.

> Research prototype, not audited, not a security product. Read
> [LIMITS.md](LIMITS.md) before drawing conclusions.

## Ideas

1. **Imports are the manifest.** A Wasm module can only reach the outside world
   through its imports. Terrarium reads them from the compiled module, compares
   them with `policy.toml` and refuses to instantiate anything that asks for
   more. No WASI/WASIX is provided: the whole host interface is four functions.
2. **Diagnostics for agents.** Every outcome is JSON with a stable `status`
   code and, where possible, a `repair_hint`. A static denial states
   `"executed": false`.
3. **Flight recorder and replay.** Clock, randomness and guest output go
   through the host, so they can be recorded and fed back. Replay checks the
   module hash, the order of host calls, the output and the result, and
   reports *where* it diverged.
4. **Rights diff.** `terrarium diff old new` shows which capabilities a new
   version of a module adds or removes. Narrowing or unchanged rights can be
   auto-approved, widening needs a decision.
5. **Budgets.** Fuel (metered Wasm operators) and an output cap per call.

## Quick start

```sh
cargo build --release
alias terrarium=./target/release/terrarium
cargo test
```

### 1. A module asks for the network, is rejected before it runs

```sh
terrarium run guests/denied_net.wat
```

```json
{
  "status": "CAP_DENIED",
  "executed": false,
  "stage": "static",
  "message": "module requests 1 import(s) that the policy does not provide",
  "violations": [
    {
      "import": "terrarium.net_connect",
      "capability": "net",
      "reason": "policy `demo` does not grant capability `net`",
      "repair_hint": "Pass the data the guest needs as input instead of fetching it; this sandbox has no network."
    }
  ]
}
```

### 2. The repaired version, and the rights diff

```sh
terrarium diff guests/denied_net.wat guests/fixed_no_net.wat   # exit 0
```

```json
{ "added": [], "removed": ["net"], "unchanged": ["stdout"], "verdict": "AUTO_APPROVE" }
```

The reverse direction reports `NEEDS_APPROVAL` and exits with code 3.

### 3. Run with a journal, then replay it

```sh
terrarium run guests/clock_rand.wat --journal run.json
terrarium replay guests/clock_rand.wat run.json
```

```json
{ "status": "REPLAY_VERIFIED", "executed": true, "stage": "replay",
  "message": "replay matched the recording", "result": -6622958541201295590, "host_calls": 3 }
```

The result depends on the clock and a random number, yet replay reproduces it
because both answers come from the journal. The run itself is repeated; what
is replaced is the outside world.

### 4. Tamper with the journal

Change one recorded clock value in `run.json` and replay again:

```json
{ "status": "REPLAY_DIVERGENCE", "executed": true, "stage": "replay",
  "message": "replay diverged from the recording (result_mismatch)",
  "divergence": { "kind": "result_mismatch",
                  "expected": "Some(-6622958541201295590)",
                  "actual": "Some(-6622958541201295589)" } }
```

Other divergences are located by event index: `event_mismatch` (wrong call
order), `output_mismatch`, `journal_unconsumed`, `journal_exhausted`,
`outcome_mismatch`. A journal replayed against a different binary yields
`REPLAY_MODULE_MISMATCH` without running anything.

Note what this does and does not show: replay detects *inconsistency* between
module and journal. It does not authenticate the journal (see
[LIMITS.md](LIMITS.md)).

## Host interface

Namespace `terrarium`. A guest must export `run: () -> i64` and, to use `log`,
a `memory`.

| Import | Signature | Capability | Journaled as |
|---|---|---|---|
| `log` | `(ptr: i32, len: i32)` | `stdout` | output (compared on replay) |
| `clock_now` | `() -> i64` (ms since epoch) | `time` | input value |
| `rand_u64` | `() -> i64` | `random` | input value |
| `net_connect` | `(ptr: i32, len: i32) -> i32` | `net` | declared, **never implemented** |

`net_connect` exists so that a denial can explain itself and suggest a repair.

## Policy

```toml
name = "demo"
allow = ["stdout", "time", "random"]

[budget]
fuel = 1_000_000
max_output_bytes = 65536
```

## Status codes

`OK`, `CAP_DENIED`, `CAP_UNKNOWN_IMPORT`, `INVALID_MODULE`, `LINK_ERROR`,
`MISSING_EXPORT`, `FUEL_EXHAUSTED`, `TRAP`, `REPLAY_VERIFIED`,
`REPLAY_DIVERGENCE`, `REPLAY_MODULE_MISMATCH`, `JOURNAL_INVALID`.

The CLI exits with 0 on `OK`/`REPLAY_VERIFIED`, 1 on any other diagnosed status,
2 on I/O or usage errors, and 3 for `diff` when rights would be widened.

## Benchmark

```sh
terrarium bench guests/fixed_no_net.wat -n 1000
```

It reports three things separately: compile + policy check, per-call latency
(fresh store + instantiate + call, compiled module reused) and calls per
second. Compile and instantiate are different questions, so they are not
mixed. Numbers depend heavily on hardware and are deliberately not included
here; run it on your machine. This benchmark is **not** comparable to
container or microVM numbers or to the Wasmer SDK: those are different layers
with different guarantees.

## Relation to the Wasmer SDK

Terrarium uses the `wasmer` runtime crate directly (Cranelift, plus the
metering middleware from `wasmer-middlewares`). It does not use the
[Wasmer SDK](https://github.com/wasmerio/wasmer-sdk), which works one level
up with registry packages and WASIX. Whether the SDK exposes hooks that would
allow the same checks on WASIX guests has not been evaluated here.

## Layout

```
src/policy.rs       host-interface registry, manifest, policy check, rights diff
src/sandbox.rs      Wasmer embedding, host functions, metering, record/replay
src/journal.rs      tape (record/replay), module hash, divergence detection
src/diagnostics.rs  status codes and the JSON report
src/main.rs         CLI: inspect, diff, run, replay, bench
guests/*.wat        sample guests (denied, repaired, replay, spin, trap, WASI)
tests/e2e.rs        end-to-end tests of denial, budgets, replay and tampering
```

## Possible next steps

- memory limits via custom tunables; module size limit before compile
- signed or hash-chained journals
- "learn mode": propose the minimal policy from what a run actually used
- network capability with a per-host allowlist
- a Rust guest crate (`wasm32-unknown-unknown`) next to the WAT samples
- an adapter that turns Wasmer SDK command results into the same diagnostics
- agent-first languages with explicit capability parameters (for example Zero)
  as a front end that emits modules for this host interface
