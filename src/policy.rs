//! Capability registry, manifest extraction, policy check and rights diff.
//!
//! A WebAssembly module can only reach the outside world through its
//! *imports*. Terrarium reads that import list as a capability manifest and
//! decides, before any guest code runs, whether the module may be
//! instantiated at all.

use std::collections::BTreeSet;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::diagnostics::{Code, Violation};

/// Namespace of every host function Terrarium provides.
pub const NAMESPACE: &str = "terrarium";

/// Static description of one host import.
pub struct HostSpec {
    pub name: &'static str,
    pub capability: &'static str,
    /// `false` = declared (so denials can explain it) but never provided.
    pub implemented: bool,
    pub repair_hint: &'static str,
}

/// The complete host interface. Anything not listed here does not exist.
pub const HOST_SPECS: &[HostSpec] = &[
    HostSpec {
        name: "log",
        capability: "stdout",
        implemented: true,
        repair_hint: "Return the value from `run` instead of logging it.",
    },
    HostSpec {
        name: "clock_now",
        capability: "time",
        implemented: true,
        repair_hint: "Take timestamps as input data instead of reading a clock.",
    },
    HostSpec {
        name: "rand_u64",
        capability: "random",
        implemented: true,
        repair_hint: "Pass a seed as input data and derive values from it.",
    },
    HostSpec {
        name: "net_connect",
        capability: "net",
        implemented: false,
        repair_hint: "Pass the data the guest needs as input instead of fetching it; \
                      this sandbox has no network.",
    },
];

pub fn lookup(module: &str, name: &str) -> Option<&'static HostSpec> {
    if module != NAMESPACE {
        return None;
    }
    HOST_SPECS.iter().find(|s| s.name == name)
}

/// What a caller is allowed to grant to a module, plus resource budgets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    #[serde(default = "default_name")]
    pub name: String,
    /// Capability names (`stdout`, `time`, `random`, ...).
    #[serde(default)]
    pub allow: BTreeSet<String>,
    #[serde(default)]
    pub budget: Budget,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Budget {
    /// Metered WebAssembly operators per call.
    #[serde(default = "default_fuel")]
    pub fuel: u64,
    /// Cap on bytes the guest may write through `log`.
    #[serde(default = "default_max_output")]
    pub max_output_bytes: usize,
}

fn default_name() -> String {
    "unnamed".into()
}
fn default_fuel() -> u64 {
    1_000_000
}
fn default_max_output() -> usize {
    64 * 1024
}

impl Default for Budget {
    fn default() -> Self {
        Budget {
            fuel: default_fuel(),
            max_output_bytes: default_max_output(),
        }
    }
}

impl Policy {
    /// Pure computation only: no capabilities at all.
    pub fn tier0() -> Self {
        Policy {
            name: "tier-0".into(),
            allow: BTreeSet::new(),
            budget: Budget::default(),
        }
    }

    pub fn from_toml_str(s: &str) -> anyhow::Result<Self> {
        toml::from_str(s).context("parsing policy TOML")
    }

    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading policy {}", path.display()))?;
        Self::from_toml_str(&text)
    }
}

/// One import as declared by the module.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct ImportEntry {
    pub module: String,
    pub name: String,
    /// `None` = not part of the Terrarium host interface.
    pub capability: Option<String>,
}

/// The module's *requested* rights (its imports), not what it actually uses.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Manifest {
    pub imports: Vec<ImportEntry>,
    /// Capabilities derived from the known imports.
    pub capabilities: BTreeSet<String>,
}

impl Manifest {
    pub fn from_imports<'a>(imports: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut m = Manifest::default();
        for (module, name) in imports {
            let capability = lookup(module, name).map(|s| s.capability.to_string());
            if let Some(c) = &capability {
                m.capabilities.insert(c.clone());
            }
            m.imports.push(ImportEntry {
                module: module.into(),
                name: name.into(),
                capability,
            });
        }
        m.imports.sort();
        m
    }

    /// Items a human or agent has to approve: known capabilities by name,
    /// unknown imports as `unknown:<module>.<name>`.
    fn rights(&self) -> BTreeSet<String> {
        let mut r = self.capabilities.clone();
        for i in self.imports.iter().filter(|i| i.capability.is_none()) {
            r.insert(format!("unknown:{}.{}", i.module, i.name));
        }
        r
    }
}

/// Check a manifest against a policy. An empty vector means "may run".
///
/// Unknown imports take precedence in the returned [`Code`]: they are a
/// structural mismatch, not a policy decision.
pub fn check(manifest: &Manifest, policy: &Policy) -> (Code, Vec<Violation>) {
    let mut violations = Vec::new();
    let mut any_unknown = false;

    for imp in &manifest.imports {
        let full = format!("{}.{}", imp.module, imp.name);
        match lookup(&imp.module, &imp.name) {
            None => {
                any_unknown = true;
                violations.push(Violation {
                    import: full,
                    capability: None,
                    reason: if imp.module == NAMESPACE {
                        format!("`{}` is not part of the Terrarium host interface", imp.name)
                    } else {
                        format!(
                            "import namespace `{}` is not provided (no WASI/WASIX here)",
                            imp.module
                        )
                    },
                    repair_hint: format!(
                        "Only imports from `{NAMESPACE}` are available: {}.",
                        HOST_SPECS
                            .iter()
                            .map(|s| s.name)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                });
            }
            Some(spec) if !policy.allow.contains(spec.capability) => violations.push(Violation {
                import: full,
                capability: Some(spec.capability.into()),
                reason: format!(
                    "policy `{}` does not grant capability `{}`",
                    policy.name, spec.capability
                ),
                repair_hint: spec.repair_hint.into(),
            }),
            Some(spec) if !spec.implemented => violations.push(Violation {
                import: full,
                capability: Some(spec.capability.into()),
                reason: format!(
                    "capability `{}` is declared but not implemented",
                    spec.capability
                ),
                repair_hint: spec.repair_hint.into(),
            }),
            Some(_) => {}
        }
    }

    let code = if any_unknown {
        Code::CapUnknownImport
    } else {
        Code::CapDenied
    };
    (code, violations)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Verdict {
    /// The new version requests no additional rights.
    AutoApprove,
    /// The new version requests rights the old one did not have.
    NeedsApproval,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RightsDiff {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub unchanged: Vec<String>,
    pub verdict: Verdict,
}

/// Compare the rights requested by two versions of a module.
pub fn diff(old: &Manifest, new: &Manifest) -> RightsDiff {
    let (o, n) = (old.rights(), new.rights());
    let added: Vec<String> = n.difference(&o).cloned().collect();
    let removed: Vec<String> = o.difference(&n).cloned().collect();
    let unchanged: Vec<String> = o.intersection(&n).cloned().collect();
    let verdict = if added.is_empty() {
        Verdict::AutoApprove
    } else {
        Verdict::NeedsApproval
    };
    RightsDiff {
        added,
        removed,
        unchanged,
        verdict,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(imports: &[(&str, &str)]) -> Manifest {
        Manifest::from_imports(imports.iter().copied())
    }

    #[test]
    fn allowed_capabilities_pass() {
        let m = manifest(&[("terrarium", "log"), ("terrarium", "clock_now")]);
        let mut p = Policy::tier0();
        p.allow.insert("stdout".into());
        p.allow.insert("time".into());
        assert!(check(&m, &p).1.is_empty());
    }

    #[test]
    fn missing_capability_is_denied_with_hint() {
        let m = manifest(&[("terrarium", "clock_now")]);
        let (code, v) = check(&m, &Policy::tier0());
        assert_eq!(code, Code::CapDenied);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].capability.as_deref(), Some("time"));
        assert!(!v[0].repair_hint.is_empty());
    }

    #[test]
    fn wasi_import_is_unknown_not_denied() {
        let m = manifest(&[("wasi_snapshot_preview1", "fd_write")]);
        let (code, v) = check(&m, &Policy::tier0());
        assert_eq!(code, Code::CapUnknownImport);
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn unimplemented_capability_stays_denied_even_if_granted() {
        let m = manifest(&[("terrarium", "net_connect")]);
        let mut p = Policy::tier0();
        p.allow.insert("net".into());
        assert_eq!(check(&m, &p).1.len(), 1);
    }

    #[test]
    fn diff_flags_added_rights_only() {
        let old = manifest(&[("terrarium", "log")]);
        let same = manifest(&[("terrarium", "log")]);
        let more = manifest(&[("terrarium", "log"), ("terrarium", "rand_u64")]);
        let less = manifest(&[]);
        assert_eq!(diff(&old, &same).verdict, Verdict::AutoApprove);
        assert_eq!(diff(&old, &less).verdict, Verdict::AutoApprove);
        let d = diff(&old, &more);
        assert_eq!(d.verdict, Verdict::NeedsApproval);
        assert_eq!(d.added, vec!["random".to_string()]);
    }

    #[test]
    fn policy_parses_from_toml() {
        let p = Policy::from_toml_str("name = \"t\"\nallow = [\"stdout\"]\n[budget]\nfuel = 5\n")
            .unwrap();
        assert_eq!(p.budget.fuel, 5);
        assert!(p.allow.contains("stdout"));
    }
}
