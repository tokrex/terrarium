use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use anyhow::Result;
use clap::{Parser, Subcommand};
use serde_json::json;

use terrarium::journal::Tape;
use terrarium::policy::{self, Policy, Verdict};
use terrarium::{Journal, Prepared, Report, load_module_bytes, sandbox};

#[derive(Parser)]
#[command(
    name = "terrarium",
    version,
    about = "Capability-checked, replayable agent sandbox on Wasmer"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print the capability manifest (imports) of a module. Nothing is executed.
    Inspect { module: PathBuf },
    /// Compare the rights requested by two versions of a module.
    /// Exit code 0 = no new rights (auto-approve), 3 = needs approval.
    Diff { old: PathBuf, new: PathBuf },
    /// Check, run and (optionally) record a module.
    Run {
        module: PathBuf,
        #[arg(long, default_value = "policy.toml")]
        policy: PathBuf,
        /// Write the journal of this run to FILE.
        #[arg(long, value_name = "FILE")]
        journal: Option<PathBuf>,
    },
    /// Re-run a module against a journal and verify identical behaviour.
    Replay { module: PathBuf, journal: PathBuf },
    /// Measure compile time and per-call latency (fresh store + instantiate + call).
    Bench {
        module: PathBuf,
        #[arg(long, default_value = "policy.toml")]
        policy: PathBuf,
        #[arg(short = 'n', long, default_value_t = 1000)]
        iterations: usize,
        #[arg(long, default_value_t = 20)]
        compile_iterations: usize,
    },
}

/// Print to stdout, ignoring a closed pipe (`terrarium ... | head`).
fn emit(text: &str) {
    let _ = writeln!(std::io::stdout(), "{text}");
}

fn print_report(report: &Report) -> ExitCode {
    emit(&report.to_json_pretty());
    ExitCode::from(report.status.exit_code() as u8)
}

fn main() -> ExitCode {
    match real_main() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("terrarium: {err:#}");
            ExitCode::from(2)
        }
    }
}

fn real_main() -> Result<ExitCode> {
    match Cli::parse().cmd {
        Cmd::Inspect { module } => {
            let wasm = load_module_bytes(&module)?;
            match sandbox::inspect(&wasm) {
                Ok(manifest) => {
                    emit(&serde_json::to_string_pretty(&manifest)?);
                    Ok(ExitCode::SUCCESS)
                }
                Err(report) => Ok(print_report(&report)),
            }
        }
        Cmd::Diff { old, new } => {
            let (old_m, new_m) = (
                sandbox::inspect(&load_module_bytes(&old)?),
                sandbox::inspect(&load_module_bytes(&new)?),
            );
            match (old_m, new_m) {
                (Ok(o), Ok(n)) => {
                    let d = policy::diff(&o, &n);
                    emit(&serde_json::to_string_pretty(&d)?);
                    Ok(if d.verdict == Verdict::AutoApprove {
                        ExitCode::SUCCESS
                    } else {
                        ExitCode::from(3)
                    })
                }
                (Err(r), _) | (_, Err(r)) => Ok(print_report(&r)),
            }
        }
        Cmd::Run {
            module,
            policy,
            journal,
        } => {
            let wasm = load_module_bytes(&module)?;
            let policy = Policy::load(&policy)?;
            let (report, j) = sandbox::record(&wasm, &policy);
            if let (Some(path), Some(j)) = (journal, j) {
                j.save(&path)?;
            }
            Ok(print_report(&report))
        }
        Cmd::Replay { module, journal } => {
            let wasm = load_module_bytes(&module)?;
            let report = match Journal::load(&journal) {
                Ok(j) => sandbox::replay(&wasm, &j),
                Err(e) => Report::journal_invalid(format!("{e:#}")),
            };
            Ok(print_report(&report))
        }
        Cmd::Bench {
            module,
            policy,
            iterations,
            compile_iterations,
        } => {
            let wasm = load_module_bytes(&module)?;
            let policy = Policy::load(&policy)?;

            let mut compile = Vec::with_capacity(compile_iterations);
            for _ in 0..compile_iterations.max(1) {
                let t = Instant::now();
                if let Err(report) = Prepared::new(&wasm, &policy) {
                    return Ok(print_report(&report));
                }
                compile.push(t.elapsed());
            }

            let prepared = match Prepared::new(&wasm, &policy) {
                Ok(p) => p,
                Err(report) => return Ok(print_report(&report)),
            };
            let mut runs = Vec::with_capacity(iterations);
            let total = Instant::now();
            for _ in 0..iterations.max(1) {
                let t = Instant::now();
                if let Err(report) = prepared.run(Tape::recording()) {
                    return Ok(print_report(&report));
                }
                runs.push(t.elapsed());
            }
            let total = total.elapsed();

            let out = json!({
                "module": module.display().to_string(),
                "note": "run = fresh Store + instantiate + call, compiled module reused",
                "compile_check": stats(&mut compile, "ms", 1_000.0),
                "run": stats(&mut runs, "us", 1_000_000.0),
                "calls_per_sec": (runs.len() as f64 / total.as_secs_f64()).round(),
            });
            emit(&serde_json::to_string_pretty(&out)?);
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// min / median / p99 / max, reported in `unit` (`scale` = units per second).
fn stats(samples: &mut [std::time::Duration], unit: &str, scale: f64) -> serde_json::Value {
    samples.sort();
    let at = |q: f64| {
        let i = ((samples.len() as f64 - 1.0) * q).round() as usize;
        (samples[i].as_secs_f64() * scale * 100.0).round() / 100.0
    };
    json!({
        "unit": unit,
        "samples": samples.len(),
        "min": at(0.0),
        "median": at(0.5),
        "p99": at(0.99),
        "max": at(1.0),
    })
}
