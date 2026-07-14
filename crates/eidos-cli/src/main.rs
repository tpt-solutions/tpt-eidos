//! `eidos` command-line tool.
//!
//! Usage:
//!   eidos check <file>              verify a `.eidos` source file
//!   eidos build <file> --out-dir D  emit a `no_std` Rust crate (erasure stub; Phase 2)

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use eidos_kernel::check;
use eidos_parser::parse;

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
        "check" => cmd_check(args.get(1).map(String::as_str)),
        "build" => cmd_build(args),
        "" => Err(usage()),
        other => Err(format!("unknown subcommand `{other}`\n{}", usage())),
    }
}

fn usage() -> String {
    "usage:\n  eidos check <file>\n  eidos build <file> --out-dir <dir>".to_string()
}

fn cmd_check(path: Option<&str>) -> Result<ExitCode, String> {
    let path = path.ok_or_else(|| format!("check requires a file path\n{}", usage()))?;
    let src = fs::read_to_string(path).map_err(|e| format!("cannot read `{path}`: {e}"))?;
    let module = parse(&src).map_err(|e| format!("parse error: {e}"))?;
    let report = check(&module);
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

fn count_ok(report: &eidos_kernel::Report) -> String {
    let verified = report
        .obligations
        .iter()
        .filter(|o| matches!(o.status, eidos_kernel::ObligationStatus::Verified))
        .count();
    let trusted = report
        .obligations
        .iter()
        .filter(|o| matches!(o.status, eidos_kernel::ObligationStatus::Trusted))
        .count();
    format!("{verified} verified, {trusted} trusted-lemma")
}

fn cmd_build(args: &[String]) -> Result<ExitCode, String> {
    let mut file: Option<&str> = None;
    let mut out_dir: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--out-dir" => {
                out_dir = Some(args.get(i + 1).ok_or("--out-dir requires a value")?.clone());
                i += 2;
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

    let src = fs::read_to_string(file).map_err(|e| format!("cannot read `{file}`: {e}"))?;
    let module = parse(&src).map_err(|e| format!("parse error: {e}"))?;
    let report = check(&module);
    if !report.ok() {
        eprintln!("eidos: {}: REJECTED (refusing to emit unverified code)", file);
        for e in &report.errors {
            eprintln!("  error: {}", e.message);
        }
        return Ok(ExitCode::FAILURE);
    }

    let dir = PathBuf::from(&out_dir);
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create `{out_dir}`: {e}"))?;
    let lib = dir.join("lib.rs");
    let stub = "#![no_std]\n\
// tpt-eidos erasure target (Phase 2).\n\
// This module was verified by the eidos kernel; proof terms have been\n\
// erased. Full computational-core codegen lands in Phase 2.\n";
    fs::write(&lib, stub).map_err(|e| format!("cannot write `{:?}`: {e}", lib))?;
    println!("eidos: {}: emitted verified crate skeleton to {}", file, out_dir);
    Ok(ExitCode::SUCCESS)
}
