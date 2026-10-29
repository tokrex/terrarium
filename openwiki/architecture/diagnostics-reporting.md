---
type: Reference
title: Diagnostics & Reporting
description: Comprehensive reference for Terrarium's machine-readable diagnostic reporting system, status codes, violation models, and repair hints.
tags: [diagnostics, error-handling, reports, agents, status-codes]
---

# Diagnostics & Reporting

Terrarium is built for automated tool loops and AI agent consumption. Every CLI invocation and API execution returns a structured, machine-readable JSON report (`Report`) with stable status codes (`Code`). Agents can inspect these fields to programmatically recover from denials or execution errors without parsing unstructured logs.

---

## Report Structure (`Report`)

```json
{
  "status": "CAP_DENIED",
  "executed": false,
  "stage": "static_check",
  "message": "policy denied 1 requested capability",
  "violations": [
    {
      "import": "terrarium.net_connect",
      "capability": "net",
      "reason": "capability 'net' is not allowed by policy 'demo'",
      "repair_hint": "Pass the data the guest needs as input instead of fetching it; this sandbox has no network."
    }
  ],
  "repair_hint": "Pass the data the guest needs as input instead of fetching it; this sandbox has no network."
}
```

### Report Fields Reference

| Field | Type | Description |
| :--- | :--- | :--- |
| `status` | `Code` | Stable status string in `SCREAMING_SNAKE_CASE` |
| `executed` | `bool` | `true` if any guest WebAssembly instructions ran; `false` on static pre-check failures |
| `stage` | `Option<String>` | Execution phase where failure occurred (e.g. `static_check`, `replay`) |
| `message` | `Option<String>` | Summary message describing the status |
| `violations` | `Vec<Violation>` | List of static capability or import violations |
| `divergence` | `Option<Divergence>` | Mismatch details if replay diverged |
| `result` | `Option<i64>` | Integer return value from guest `run` function |
| `output` | `Option<String>` | Logged string output emitted by guest |
| `fuel_used` | `Option<u64>` | Number of metered WebAssembly instructions executed |
| `host_calls` | `Option<usize>` | Number of host functions invoked during execution |
| `repair_hint` | `Option<String>` | Actionable advice for AI agents on how to fix code or configuration |

---

## Status Codes (`Code`)

Status codes represent fixed contract values guaranteed not to break across versions.

| Code String | Description | Process Exit Code |
| :--- | :--- | :--- |
| `OK` | Guest executed successfully and returned a value | `0` |
| `REPLAY_VERIFIED` | Replay pass matched journal recording exactly | `0` |
| `CAP_DENIED` | Module requested a known capability not granted by policy | `1` |
| `CAP_UNKNOWN_IMPORT` | Module requested an import unknown to Terrarium (e.g., WASI) | `1` |
| `INVALID_MODULE` | Bytes could not be parsed as valid WebAssembly or WAT | `1` |
| `LINK_ERROR` | Module failed host function signature linking | `1` |
| `MISSING_EXPORT` | Module does not export the required `run: () -> i64` function | `1` |
| `FUEL_EXHAUSTED` | Execution exceeded assigned instruction fuel budget | `1` |
| `TRAP` | Guest triggered a WebAssembly trap (e.g. out-of-bounds memory access) | `1` |
| `REPLAY_DIVERGENCE` | Guest behavior diverged from recorded journal during replay | `1` |
| `REPLAY_MODULE_MISMATCH` | Replay module binary SHA-256 differs from journal hash | `1` |
| `JOURNAL_INVALID` | Journal JSON is malformed or uses an unsupported schema format | `1` |

---

## CLI Process Exit Codes

Terrarium CLI uses deterministic process exit codes for script and tool orchestration:

* **`0`**: Success (`OK`, `REPLAY_VERIFIED`, or `diff` with `AutoApprove`).
* **`1`**: Diagnosed sandbox failure (e.g., `CAP_DENIED`, `FUEL_EXHAUSTED`, `TRAP`).
* **`2`**: Unhandled host I/O or CLI parameter parsing error.
* **`3`**: Capability diff requires approval (`Verdict::RequiresApproval`).

---

## Automated Remediation via `repair_hint`

When static checks deny execution or imports are missing, host specifications supply a `repair_hint`. This field instructs AI code generation agents on how to refactor their generated code to comply with sandbox restrictions.

**Example Scenario:**
1. Agent submits WAT module importing `terrarium.net_connect`.
2. Terrarium returns `Code::CapDenied` with `repair_hint: "Pass the data the guest needs as input instead of fetching it; this sandbox has no network."`.
3. Agent reads `repair_hint` and rewrites guest code to receive data parameters directly instead of performing network fetching.
