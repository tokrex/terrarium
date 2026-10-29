# Limits and threat model

Terrarium is a **research prototype**. It has not been audited. This page says
what it does and does not give you.

## What it is meant to protect against

A guest module that is buggy, over-eager or hostile *at the interface level*:

- asking for capabilities it was not granted (checked **before** it runs),
- looping forever (fuel budget),
- flooding output (output cap),
- being replayed or audited later against a different binary (module hash).

## What it does **not** protect against

**The runtime is the trust boundary, and it shares your process.**
Terrarium embeds Wasmer in-process. There is no hardware or kernel boundary
behind it. A bug in the runtime, the compiler backend or this host code can
break isolation. WebAssembly runtimes in general have had such bugs. For
untrusted code, run Terrarium itself inside a restricted process or container
(defense in depth).

**Side channels.** Code in the same process can attempt timing and
Spectre-style attacks. Not addressed.

**Memory.** There is **no memory cap** beyond what the module itself declares
and the runtime allows. A guest can still allocate a lot. (Planned: a limiting
`Tunables`.)

**Time.** Budgets are *fuel* (metered Wasm operators), not wall-clock time.
Fuel is counted per basic block and does not include time spent inside host
calls. The host calls here are cheap and bounded, but this matters if you add
slow ones.

**Compile cost.** The module is compiled before the policy check, so even a
rejected module costs compile time. There is no module size limit yet.

**`start` functions.** A module's `start` function runs during instantiation.
It is metered (the budget is set at compile time), so it cannot hang the host.
Running out of fuel there is reported as `TRAP` with `"stage": "instantiate"`
and cannot be told apart from an ordinary trap in that function. No journal is
written in that case.

## What the journal does and does not prove

Replay guarantees, for a **single-threaded guest whose only nondeterminism
enters through Terrarium's imports**:

- the same module binary (SHA-256) was used,
- it made the same host calls in the same order,
- it produced the same output and the same result.

It does **not**:

- **authenticate** anything. A journal is plain JSON. Someone who edits both
  an input value *and* the recorded result consistently will pass replay.
  Replay detects *inconsistency*, not forgery. Signing or hash-chaining the
  journal is future work.
- cover WASIX-style guests with threads or scheduler nondeterminism.
- cover Wasm-level nondeterminism that does not go through an import (for
  example NaN bit patterns or `memory.grow` failures); none of the sample
  guests depend on these.
- store `i64` values in a form that every JSON consumer reads losslessly.
  `serde_json` round-trips them, some other tools will not.

## The manifest shows *requested*, not *used*

A module's imports are its requested rights. A module may import a function
and never call it. The journal shows what was actually used. Comparing the two
is the basis for a "learn the minimal policy" mode, which is not implemented.
