//! Terrarium: a tiny, inspectable world per agent call, built on Wasmer.
//!
//! The pipeline is deliberately small:
//!
//! 1. [`sandbox::Prepared::new`] compiles the module and reads its
//!    import list as a capability manifest ([`policy::Manifest`]).
//! 2. The manifest is checked against a [`policy::Policy`] *before* any guest
//!    code runs. Denials come back as structured [`diagnostics::Report`]s.
//! 3. [`sandbox::record`] runs the guest with a fuel budget and
//!    records every host interaction into a [`journal::Journal`];
//!    [`sandbox::replay`] re-runs it against a journal and detects divergence.

pub mod diagnostics;
pub mod journal;
pub mod policy;
pub mod sandbox;

pub use diagnostics::{Code, Report};
pub use journal::Journal;
pub use policy::{Manifest, Policy};
pub use sandbox::{Prepared, RunRecord, inspect, record, replay};

/// Load a guest module from bytes: binary `.wasm` or text `.wat`.
pub fn load_module_bytes(path: &std::path::Path) -> anyhow::Result<Vec<u8>> {
    use anyhow::Context;
    let raw = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if path.extension().is_some_and(|e| e == "wat") {
        Ok(wasmer::wat2wasm(&raw)
            .with_context(|| format!("parsing WAT in {}", path.display()))?
            .into_owned())
    } else {
        Ok(raw)
    }
}
