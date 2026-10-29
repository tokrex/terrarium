---
type: Source Map
title: Source Map
description: Navigation guide and directory mapping for the Terrarium codebase, modules, data structures, test cases, and guest WebAssembly examples.
tags: [source-map, repository-structure, code-layout, rust, modules]
---

# Source Map

This document maps out the repository structure, Rust modules, test suites, and WebAssembly guest fixtures that compose Terrarium.

---

## Directory Tree

```
/
├── Cargo.toml                 # Cargo package metadata and dependencies (Wasmer 7.5, Serde, Clap)
├── policy.toml                # Default policy configuration file
├── LIMITS.md                  # System boundaries and architecture constraints
├── README.md                  # Project overview and introduction
├── guests/                    # WebAssembly Text (.wat) test fixtures
│   ├── clock_rand.wat         # Guest using clock, random, and logging capabilities
│   ├── denied_net.wat         # Guest attempting network access (denied by default)
│   ├── fixed_no_net.wat       # Repaired version of denied_net using input parameters
│   ├── pure_add.wat           # Pure computation module requiring zero capabilities
│   ├── spin.wat               # Infinite loop guest for testing fuel limit enforcement
│   ├── trap_oob.wat           # Guest triggering out-of-bounds memory trap
│   └── wasi_hello.wat         # Guest attempting WASI imports (rejected as unknown)
├── src/                       # Rust source code
│   ├── lib.rs                 # Library entrypoint, module definitions, helper functions
│   ├── main.rs                # CLI application commands and report printing
│   ├── policy.rs              # Capability registry, manifest parsing, policy checks, diffing
│   ├── sandbox.rs             # Wasmer engine runtime, host function bindings, record/replay
│   ├── journal.rs             # Journal data model, JSON persistence, tape state machine
│   └── diagnostics.rs         # Machine-readable report schemas, status codes, divergence types
└── tests/                     # Integration tests
    └── e2e.rs                 # End-to-end test suite for static checks, budgets, replay, and traps
```

---

## Module Responsibilities

### Core Runtime (`src/`)

| File / Module | Key Functions & Structs | Responsibilities |
| :--- | :--- | :--- |
| **`src/lib.rs`** | `load_module_bytes` | Library root; re-exports core APIs and provides `.wat` to `.wasm` parsing using `wasmer::wat2wasm`. |
| **`src/main.rs`** | `Cli`, `Cmd`, `real_main`, `print_report` | Parses command-line arguments (`inspect`, `diff`, `run`, `replay`, `bench`) and formats reports to `stdout`. |
| **`src/policy.rs`** | `HostSpec`, `HOST_SPECS`, `Policy`, `Budget`, `Manifest`, `diff` | Maintains the static host interface registry, parses module imports, verifies capabilities against policy rules, and computes diffs between module versions. |
| **`src/sandbox.rs`** | `Prepared`, `record`, `replay`, `host_log`, `host_clock_now`, `host_rand_u64` | Embeds Wasmer; handles Cranelift compilation, fuel metering middleware, host function bindings, and execution in record or replay mode. |
| **`src/journal.rs`** | `Journal`, `Event`, `Tape`, `Outcome`, `sha256_hex` | Manages JSON flight recordings, records/plays event sequences, and computes module SHA-256 hashes. |
| **`src/diagnostics.rs`** | `Report`, `Code`, `Violation`, `Divergence` | Defines machine-readable status codes, failure violations, repair hints, and divergence report payloads. |

---

## WebAssembly Guest Fixtures (`guests/`)

The `guests/` directory contains WAT files used for testing and validating sandbox security invariants:

* **`clock_rand.wat`**: Imports `clock_now`, `rand_u64`, and `log`. Verifies multi-capability execution and journal recording.
* **`denied_net.wat`**: Imports `net_connect` to test static capability denial before execution.
* **`fixed_no_net.wat`**: Validates remediation workflows where network dependency is removed.
* **`pure_add.wat`**: Pure function (`run() -> 42`) testing execution under zero-capability (`Policy::tier0()`) rules.
* **`spin.wat`**: Infinite loop (`loop ... br 0`) testing operator fuel metering and `Code::FuelExhausted`.
* **`trap_oob.wat`**: Triggers WebAssembly memory traps (`i32.load` out of bounds) to test `Code::Trap`.
* **`wasi_hello.wat`**: Imports `wasi_snapshot_preview1::fd_write` to test `Code::CapUnknownImport`.

---

## Integration Tests (`tests/e2e.rs`)

The test suite validates sandbox invariants across several categories:
1. **Static Capability Enforcement:** Network denial pre-check, repaired module execution, zero-capability policy (`tier0`).
2. **Import Validation:** Unknown WASI import rejection, missing entry points, malformed Wasm bytes.
3. **Budget Enforcement:** Fuel exhaustion trapping and deterministic replay verification of fuel-exhausted runs.
4. **Deterministic Replay Safety:** Successful replay matching, divergence detection on altered outputs/results, and binary SHA-256 hash mismatches (`Code::ReplayModuleMismatch`).
