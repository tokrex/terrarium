**VISION.md**

# Terrarium: Toward an Agent-Native Execution Substrate

Agents do not merely generate code. They must run it, observe it, repair it, and eventually compose it into larger systems. The dominant response today is still to drop that code into a container or microVM and hope the boundary holds. Terrarium explores a narrower and more radical premise: a WebAssembly module can touch the outside world only through its imports. Those imports can therefore be declared, checked, metered, and recorded as ground truth.

This is not a claim that existing sandboxes are useless. It is the observation that they were designed for a different workload—long-running, multi-purpose processes written by humans. Agents need something else: short-lived, pure computational units whose every side-effect is policy-controlled and fully replayable. The question is what becomes possible when the execution environment itself is shaped around that requirement.

## Three Observations

### 1. The Human Projection Tax

Contemporary programming languages were designed for human developers. They prioritise readability, rich standard libraries, and interactive tooling. Agent-centric experiments such as Zerolang show that code can be made dramatically more token-efficient and runtime-efficient when generation happens close to the compiler. Yet even these languages still carry an explicit human-readable projection as part of their design.

From an agent’s perspective that projection is pure overhead. A consistent alternative moves auditability into the runtime. Every capability use, every side-effect, and every failure crosses the module’s import boundary and can therefore be journaled. Manifest, rights diff, and deterministic flight recorder become the primary artefacts of review. A separate human-readable rendering of the source is no longer required for the question “what can this do to the world?”

**Status.** Manifest checking, journaling, replay with divergence detection, and rights diffs are demonstrated in this repository. Closed-world test suites, coverage analysis, and detection of environment-sensitive behaviour remain open. Effect auditing is not correctness auditing; accountability may still require a human statement of intent. The journal is currently unauthenticated (see LIMITS.md).

### 2. The Closed Lifecycle Loop

Rethinking agent sandboxing cannot stop at code generation. Compilation, execution, diagnosis, repair, deployment, and shipping form a single loop. WebAssembly supplies a convenient substrate: modules are validated before instantiation, memory is bounds-checked, imports are explicit capabilities, and cold-start cost is low enough to instantiate per call.

Terrarium builds on Wasmer Core to make isolation, resource control, and observability intrinsic rather than external. Module imports become natural capability guardrails. Instruction-level metering enforces hard bounds on compute and output. Structured diagnostics return stable status codes and repair hints so that the next generation step can act on precise feedback.

**Status.** Static denials and runtime failures already surface as structured JSON with repair hints; fuel and output caps bound a call. Whether this shortens an agent’s repair loop in practice is a hypothesis until measured. Claims about deployment and shipping remain speculative. Containers and microVMs can also be short-lived; the distinguishing claim is the per-call capability manifest and deterministic replay, not raw speed.

### 3. A Tiny World per Call

Existing sandboxes were built to host long-running processes. They deliver coarse isolation at the price of non-determinism, opaque failure modes, and high cold-start overhead. Agents, by contrast, repeatedly need small, pure functions—checks, transformations, glue—whose interactions with the outside world are both policy-controlled and fully replayable.

Only an execution model that treats the guest as a pure function with explicitly declared, journaled side-effects can close the generation–execution–repair loop with the required precision. When such units can be published, verified, and reused, repeated generation work can shrink. This is the point at which a registry and an edge fabric become interesting.

**Status.** The “tiny world” abstraction is the core design of Terrarium. Registry reuse, cross-module composition ABIs, and large-scale swarm efficiency remain hypotheses. Trust, selection cost, and composition semantics are open problems that must be solved before reuse can be claimed to dominate regeneration.

## Outlook

Current agent design discussions frequently centre on the Ambiguity Tax and the Hidden Tax—the costs of over-generalisation and of opaque intermediate state. These pressures push systems toward single, heavyweight harnesses that attempt to handle every task. Work such as Voyager, ToolMaker, and Agent S4 already points beyond that model: agents that construct and refine their own tooling during operation. In this trajectory, secure, observable code execution ceases to be a side concern and becomes foundational infrastructure.

Terrarium treats generation and execution as the orchestration of isolated WebAssembly modules. Each module is a tiny world. From the agent’s perspective these worlds can be composed into arbitrarily large systems provided that reuse is both safe and efficient. A registry of verified modules can amortise generation cost across tasks and agents. An edge fabric can assemble networked modules while keeping observability, metering, and capability boundaries intrinsic to the runtime. Humans retain the ability to audit, replay, and bound every path.

Taken further, such registries begin to resemble an agent-owned ecosystem. Swarms that today waste tokens and compute on duplicated effort could instead publish specialised tools that other agents simply invoke. Whether this economics holds depends on the relative cost of retrieval-plus-trust versus regeneration—an open empirical question.

## Economic Stakes

Agent code execution and composition is likely to become one of the consequential infrastructure layers of the next decade. SaaS platforms already face pressure from users who expect logic and reporting to be reshaped by prompt rather than locked into static schemas. Search engines are experimenting with on-demand assembly of micro-applications. App-store models confront the possibility that “prompt-to-app” becomes the default interaction. In each case the decisive capability is not generation alone but safe, observable, and composable execution.

The most defensible near-term value is accountability: when an agent-written program acts, what evidence exists of the rights it held and the effects it produced? A rights diff before deployment and a replayable journal afterwards are the kind of artefacts a company or regulator can examine. Larger claims about the displacement of packaged software remain speculative and must be tested against the realities of data, integrations, compliance, and responsibility that incumbents currently supply.

## A Closing Provocation

Today we still embed agents inside sandboxes and languages designed for humans. The situation is analogous to giving a robot arms and legs so that it can navigate a world built for people. We force the machine to adapt to a human-centric substrate.

The more interesting design question runs in the opposite direction: what becomes possible when the execution environment, the language, and the composition model are shaped primarily around the needs and efficiencies of the agents themselves?

The counterpoint is real. The human-built world is where the data, the APIs, and the systems live. An agent-native core still requires compatibility at its edges. How large that compatibility surface must be—and how much of the human projection tax can ultimately be retired—is the open question this project exists to explore.