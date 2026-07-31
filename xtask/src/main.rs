//! `cargo xtask <subcommand>` — see `cargo xtask help`.

use std::process::{Command, ExitCode};

use anyhow::{Result, bail};
use xtask::{Violation, cfg_leak, locale, repo_root};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let subcommand = args.first().map(String::as_str).unwrap_or("help");

    let outcome = match subcommand {
        "locale-check" => locale_check(),
        "cfg-check" => cfg_check(),
        "check" => check_all(),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        other => {
            eprintln!("unknown subcommand `{other}`\n");
            print_help();
            return ExitCode::FAILURE;
        }
    };

    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("\x1b[31merror\x1b[0m: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn print_help() {
    println!(
        "\
cargo xtask <subcommand>

  locale-check   Every locale defines exactly the keys en-US defines
  cfg-check      Platform-conditional code stays in src-tauri/ and pandaspy-store/
  check          The full pre-push gate: fmt, clippy, tests, both checks above,
                 and the frontend lint + typecheck if pnpm is installed
  help           This message
"
    );
}

fn locale_check() -> Result<()> {
    let root = repo_root();
    let (summary, violations) = locale::check(&root.join("locales"))?;

    report("locale parity", &violations)?;
    println!(
        "  {} locale(s) [{}], {} key(s) in {}",
        summary.locales.len(),
        summary.locales.join(", "),
        summary.reference_keys,
        locale::REFERENCE,
    );
    Ok(())
}

fn cfg_check() -> Result<()> {
    let violations = cfg_leak::check(&repo_root())?;

    report("platform-conditional code", &violations)?;
    println!(
        "  scanned {} — allowed only in src-tauri/ and {}",
        cfg_leak::SCANNED
            .iter()
            .map(|dir| format!("{dir}/"))
            .collect::<Vec<_>>()
            .join(", "),
        cfg_leak::ALLOWED.join(", "),
    );
    Ok(())
}

fn report(what: &str, violations: &[Violation]) -> Result<()> {
    if violations.is_empty() {
        println!("\x1b[32mok\x1b[0m: {what}");
        return Ok(());
    }

    for violation in violations {
        eprintln!("  {violation}");
    }
    bail!("{what}: {} problem(s) — see above", violations.len());
}

/// The gate. One definition, so that "it passed locally" means the same thing
/// as "it passed in CI".
fn check_all() -> Result<()> {
    cargo(&["fmt", "--all", "--check"])?;
    cargo(&[
        "clippy",
        "--workspace",
        "--all-targets",
        "--",
        "-D",
        "warnings",
    ])?;
    cargo(&["test", "--workspace"])?;

    // `pandaspy-proto` must stay free of I/O. wasm is a target where I/O does not
    // exist, so this failing is the earliest possible warning that something
    // impure has crept in.
    cargo(&[
        "check",
        "--package",
        "pandaspy-proto",
        "--target",
        "wasm32-unknown-unknown",
    ])?;

    locale_check()?;
    cfg_check()?;

    match pnpm(&["run", "check"]) {
        Ok(()) => {}
        Err(PnpmError::NotInstalled) => {
            // Not silent, and not fatal: a Rust-only contributor should still
            // be able to run the gate, but must know what it skipped.
            println!("\x1b[33mskipped\x1b[0m: frontend lint + typecheck (pnpm not found on PATH)");
        }
        Err(PnpmError::Failed(error)) => return Err(error),
    }

    println!("\n\x1b[32mall checks passed\x1b[0m");
    Ok(())
}

fn cargo(args: &[&str]) -> Result<()> {
    // Respect the toolchain cargo already selected rather than whatever `cargo`
    // resolves to, so a nested call cannot silently use a different rustc.
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());

    println!("\n\x1b[1m$ cargo {}\x1b[0m", args.join(" "));
    let status = Command::new(cargo)
        .args(args)
        .current_dir(repo_root())
        .status()?;

    if !status.success() {
        bail!("cargo {} failed", args.join(" "));
    }
    Ok(())
}

enum PnpmError {
    NotInstalled,
    Failed(anyhow::Error),
}

/// Run pnpm, probing for the executable name at runtime.
///
/// On Windows pnpm is `pnpm.cmd`, not `pnpm`. Resolving that with
/// `#[cfg(windows)]` would be the very thing this repository's cfg-check
/// forbids — and it would also be wrong, because the right name depends on how
/// pnpm was installed, not on what the binary was compiled for. Probe instead.
fn pnpm(args: &[&str]) -> std::result::Result<(), PnpmError> {
    println!("\n\x1b[1m$ pnpm {}\x1b[0m", args.join(" "));

    for executable in ["pnpm", "pnpm.cmd"] {
        match Command::new(executable)
            .args(args)
            .current_dir(repo_root())
            .status()
        {
            Ok(status) if status.success() => return Ok(()),
            Ok(_) => {
                return Err(PnpmError::Failed(anyhow::anyhow!(
                    "pnpm {} failed",
                    args.join(" ")
                )));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(PnpmError::Failed(error.into())),
        }
    }

    Err(PnpmError::NotInstalled)
}
