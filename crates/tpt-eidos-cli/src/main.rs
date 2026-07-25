//! `eidos` command-line tool.
//!
//! Usage:
//!   eidos check <file>              verify a `.eidos` source file
//!   eidos build <file> --out-dir D  emit a verified `no_std` Rust crate (erasure + codegen)

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use tpt_eidos_flight_math::check_module;
use tpt_eidos_parser::parse;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(msg) => {
            eprintln!("eidos: error: {msg}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    let cmd = args.first().map(String::as_str).unwrap_or("");
    match cmd {
        "--version" | "-V" => {
            println!("eidos {}", env!("CARGO_PKG_VERSION"));
            Ok(ExitCode::SUCCESS)
        }
        "--help" | "-h" => {
            println!("{}", usage());
            Ok(ExitCode::SUCCESS)
        }
        "check" => cmd_check(args.get(1).map(String::as_str)),
        "build" => cmd_build(args),
        "" => Err(usage()),
        other => Err(format!("unknown subcommand `{other}`\n{}", usage())),
    }
}

fn usage() -> String {
    "usage:\n  eidos check <file>\n  eidos build <file> --out-dir <dir>\n  eidos --version\n  eidos --help"
        .to_string()
}

/// Derive a valid Rust crate name from the source file path. Cargo package
/// names must be non-empty and start with an ASCII letter or underscore, and
/// contain only alphanumerics, `-`, or `_`. This sanitizes arbitrary file
/// stems (including all-non-alphanumeric or digit-leading stems) into a name
/// that `cargo` will accept (bug #16).
fn crate_name(file: &str) -> String {
    let base = std::path::Path::new(file)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "eidos_out".into());
    let mut name: String = base
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    // Cargo package names must be non-empty and start with an ASCII letter; a
    // stem that is all-non-alphanumeric or digit-leading (or otherwise starts
    // with a non-letter) is rejected by `cargo`, so prefix it (bug #16).
    if name.is_empty() || !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
        name = format!("eidos_{name}");
    }
    name
}

fn cmd_check(path: Option<&str>) -> Result<ExitCode, String> {
    let path = path.ok_or_else(|| format!("check requires a file path\n{}", usage()))?;
    let src = fs::read_to_string(path).map_err(|e| format!("cannot read `{path}`: {e}"))?;
    let module = parse(&src).map_err(|e| format!("parse error: {e}"))?;
    let report = check_module(&module);
    if report.ok() {
        println!("eidos: {}: verified ({})", path, count_ok(&report));
        Ok(ExitCode::SUCCESS)
    } else {
        eprintln!("eidos: {}: REJECTED", path);
        for e in &report.errors {
            eprintln!("  error: {}", e.message);
        }
        Ok(ExitCode::FAILURE)
    }
}

fn count_ok(report: &tpt_eidos_kernel::Report) -> String {
    let verified = report
        .obligations
        .iter()
        .filter(|o| matches!(o.status, tpt_eidos_kernel::ObligationStatus::Verified))
        .count();
    let trusted = report
        .obligations
        .iter()
        .filter(|o| matches!(o.status, tpt_eidos_kernel::ObligationStatus::Trusted))
        .count();
    format!("{verified} verified, {trusted} trusted-lemma")
}

fn cmd_build(args: &[String]) -> Result<ExitCode, String> {
    let mut file: Option<&str> = None;
    let mut out_dir: Option<String> = None;
    let mut force = false;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--out-dir" => {
                out_dir = Some(args.get(i + 1).ok_or("--out-dir requires a value")?.clone());
                i += 2;
            }
            "--force" => {
                force = true;
                i += 1;
            }
            other if !other.starts_with('-') && file.is_none() => {
                file = Some(other);
                i += 1;
            }
            other => return Err(format!("unexpected build argument `{other}`")),
        }
    }
    let file = file.ok_or_else(|| format!("build requires a file path\n{}", usage()))?;
    let out_dir = out_dir.ok_or_else(|| format!("build requires --out-dir\n{}", usage()))?;

    // Refuse to clobber a non-empty output directory unless --force is given.
    let dir = PathBuf::from(&out_dir);
    if !force && dir.exists() {
        if let Ok(mut entries) = fs::read_dir(&dir) {
            if entries.next().is_some() {
                return Err(format!(
                    "output directory `{out_dir}` is not empty; pass --force to overwrite"
                ));
            }
        }
    }

    let src = fs::read_to_string(file).map_err(|e| format!("cannot read `{file}`: {e}"))?;
    let module = parse(&src).map_err(|e| format!("parse error: {e}"))?;
    let report = check_module(&module);
    if !report.ok() {
        eprintln!(
            "eidos: {}: REJECTED (refusing to emit unverified code)",
            file
        );
        for e in &report.errors {
            eprintln!("  error: {}", e.message);
        }
        return Ok(ExitCode::FAILURE);
    }

    let dir = PathBuf::from(&out_dir);
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create `{out_dir}`: {e}"))?;
    let core = tpt_eidos_erasure::erase(&module);
    let rust = tpt_eidos_codegen::codegen(&core).map_err(|e| format!("codegen failed: {e}"))?;
    let lib = dir.join("lib.rs");
    fs::write(&lib, &rust).map_err(|e| format!("cannot write `{:?}`: {e}", lib))?;
    let cargo = dir.join("Cargo.toml");
    let cargo_toml = format!(
        "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n",
        crate_name(file)
    );
    fs::write(&cargo, cargo_toml).map_err(|e| format!("cannot write `{:?}`: {e}", cargo))?;
    println!(
        "eidos: {}: emitted verified no_std crate to {} (lib.rs + Cargo.toml)",
        file, out_dir
    );
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_sanitizes_digit_leading_stem() {
        // A digit-leading stem must be prefixed so Cargo accepts the package.
        let n = crate_name("123abc.eidos");
        assert!(!n.is_empty());
        assert!(n
            .chars()
            .next()
            .map(|c| c.is_ascii_alphabetic())
            .unwrap_or(false));
        assert!(n
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'));
    }

    #[test]
    fn crate_name_sanitizes_non_alphanumeric_stem() {
        let n = crate_name("!!!.eidos");
        assert!(!n.is_empty());
        assert!(n
            .chars()
            .next()
            .map(|c| c.is_ascii_alphabetic())
            .unwrap_or(false));
        assert!(n
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'));
    }

    #[test]
    fn crate_name_lowercases_and_normalizes() {
        assert_eq!(crate_name("My.Mod.eidos"), "my_mod");
        assert_eq!(crate_name("CamelCase.eidos"), "camelcase");
    }

    #[test]
    fn version_flag_succeeds() {
        for flag in ["--version", "-V"] {
            let r = run(&[flag.to_string()]);
            assert!(matches!(r, Ok(ExitCode::SUCCESS)), "flag: {flag}");
        }
    }

    #[test]
    fn help_flag_succeeds() {
        for flag in ["--help", "-h"] {
            let r = run(&[flag.to_string()]);
            assert!(matches!(r, Ok(ExitCode::SUCCESS)), "flag: {flag}");
        }
    }
}
