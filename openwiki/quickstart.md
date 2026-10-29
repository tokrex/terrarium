---
type: Guide
title: Quickstart
description: Getting started guide for Terrarium, covering module inspection, execution, policy enforcement, flight recording, and deterministic replay.
tags: [quickstart, usage, cli, sandbox, wasm]
---

# Quickstart

Terrarium is a capability-checked, journaled, and replayable WebAssembly agent sandbox built on Wasmer. It allows hosts to inspect WebAssembly modules before execution, verify requested capabilities against policy rules, record runtime interactions, and replay execution traces deterministically.

## Prerequisites & Installation

To build and run Terrarium, ensure you have Rust 2024 edition installed along with `cargo`.

```bash
cargo build --release
```

The resulting binary will be located at `target/release/terrarium`.

---

## Core Operations

Terrarium provides five primary CLI subcommands: `inspect`, `diff`, `run`, `replay`, and `bench`.

### 1. Inspecting Module Capabilities (`inspect`)

Examine the required imports of a WebAssembly binary or WAT text file without executing any code.

```bash
terrarium inspect guests/clock_rand.wat
```

**Example JSON Output:**
```json
{
  "imports": [
    {
      "module": "terrarium",
      "name": "clock_now"
    },
    {
      "module": "terrarium",
      "name": "rand_u64"
    },
    {
      "module": "terrarium",
      "name": "log"
    }
  ]
}
```

### 2. Diffing Module Capabilities (`diff`)

Compare requested capabilities between an older module and a newer module. If the new module requests no additional capabilities, `diff` exits with code `0` (`AutoApprove`). If new permissions are requested, it exits with code `3` (`RequiresApproval`).

```bash
terrarium diff guests/pure_add.wat guests/clock_rand.wat
```

### 3. Running and Recording Execution (`run`)

Run a WebAssembly module against a TOML policy file and optionally write the runtime flight record to a journal file.

```bash
terrarium run guests/clock_rand.wat --policy policy.toml --journal run.json
```

**Example JSON Output Report:**
```json
{
  "status": "OK",
  "executed": true,
  "result": 42,
  "output": "clock: 1718000000000, rand: 88291029",
  "fuel_used": 142,
  "host_calls": 3
}
```

### 4. Deterministic Replay (`replay`)

Re-run a WebAssembly module using a recorded journal file. The sandbox verifies that the guest calls the exact same host imports in the exact same sequence and produces identical outputs.

```bash
terrarium replay guests/clock_rand.wat run.json
```

**Output on Success:**
```json
{
  "status": "REPLAY_VERIFIED",
  "executed": true,
  "result": 42
}
```

### 5. Microbenchmarking (`bench`)

Measure compilation and per-call latency over multiple iterations.

```bash
terrarium bench guests/clock_rand.wat --policy policy.toml --iterations 1000 --compile-iterations 20
```

---

## Sample Policy (`policy.toml`)

Policy files define which host capabilities are granted to guest modules and set execution resource limits:

```toml
name = "demo"
allow = ["stdout", "time", "random"]

[budget]
fuel = 1000000          # Maximum metered Wasm operators per call
max_output_bytes = 65536  # Maximum bytes guest can emit via log
```
