//! `eidos` command-line tool.
//!
//! Usage:
//!   eidos new <name>                 scaffold a new .eidos project
//!   eidos check <file>              verify a `.eidos` source file
//!   eidos build <file> --out-dir D  emit a verified `no_std` Rust crate (erasure + codegen)

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use tpt_eidos_flight_math::check_module;
use tpt_eidos_parser::parse;
use tpt_eidos_parser::Span;

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
        "check" => cmd_check(args),
        "new" => cmd_new(args.get(1).map(String::as_str)),
        "build" => cmd_build(args),
        "test" => cmd_test(args),
        "" => Err(usage()),
        other => Err(format!("unknown subcommand `{other}`\n{}", usage())),
    }
}

fn usage() -> String {
    "usage:\n  eidos new <name>\n  eidos check <file> [--verbose] [--json] [--emit ast|core]\n  eidos build <file> --out-dir <dir> [--force] [--verbose] [--json]\n  eidos test <dir> [--verbose]\n  eidos --version\n  eidos --help"
        .to_string()
}

fn byte_to_line_col(src: &str, offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for (i, c) in src.char_indices() {
        if i >= offset {
            break;
        }
        if c == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

fn render_error(path: &str, src: &str, e: &tpt_eidos_kernel::CheckError) {
    match e.span {
        Some(Span { lo, .. }) if lo > 0 => {
            let (line, col) = byte_to_line_col(src, lo);
            eprintln!("  {}:{}:{}: error: {}", path, line, col, e.message);
        }
        _ => {
            eprintln!("  error: {}", e.message);
        }
    }
}

fn render_parse_error(path: &str, src: &str, e: &tpt_eidos_parser::ParseError) {
    match e.span {
        Some(Span { lo, .. }) if lo > 0 => {
            let (line, col) = byte_to_line_col(src, lo);
            eprintln!("  {}:{}:{}: parse error: {}", path, line, col, e);
        }
        _ => {
            eprintln!("  parse error: {}", e);
        }
    }
}

fn cmd_new(name: Option<&str>) -> Result<ExitCode, String> {
    let name = name.ok_or_else(|| format!("new requires a project name\n{}", usage()))?;
    let dir = PathBuf::from(name);
    if dir.exists() {
        return Err(format!("directory `{name}` already exists"));
    }
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create `{name}`: {e}"))?;
    let src_path = dir.join(format!("{name}.eidos"));
    let src = "// A refined type: a normalized 3D vector.\n\
         type NormalizedVector3 = { v: Array<f64, 3> | v.magnitude() <= 1.0 };\n\
         \n\
         // A verified function: division by zero is provably impossible.\n\
         fn calibrate(raw: Array<f64, 3>, bias: Array<f64, 3>) -> NormalizedVector3\n\
         requires raw.len() == 3 && bias.len() == 3\n\
         ensures |result| result.v.magnitude() <= 1.0\n\
         {\n\
         let corrected = raw.zip(bias).map(|(r, b)| r - b);\n\
         let mag = corrected.magnitude();\n\
         if mag > 0.0 {\n\
         return { v: corrected.map(|x| x / mag) } as NormalizedVector3;\n\
         } else {\n\
         return { v: [0.0, 0.0, 0.0] } as NormalizedVector3;\n\
         }\n\
         }\n"
    .to_string();
    fs::write(&src_path, &src).map_err(|e| format!("cannot write `{:?}`: {e}", src_path))?;
    println!("eidos: scaffolded new project `{name}` in `{name}/`");
    Ok(ExitCode::SUCCESS)
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

fn cmd_check(args: &[String]) -> Result<ExitCode, String> {
    let mut file: Option<&str> = None;
    let mut verbose = false;
    let mut json = false;
    let mut emit: Option<&str> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--verbose" | "-v" => {
                verbose = true;
                i += 1;
            }
            "--json" | "-j" => {
                json = true;
                i += 1;
            }
            "--emit" => {
                emit = Some(
                    args.get(i + 1)
                        .ok_or("--emit requires a value (ast or core)")?,
                );
                i += 2;
            }
            other if !other.starts_with('-') && file.is_none() => {
                file = Some(other);
                i += 1;
            }
            other => return Err(format!("unexpected check argument `{other}`")),
        }
    }
    let path = file.ok_or_else(|| format!("check requires a file path\n{}", usage()))?;
    let src = fs::read_to_string(path).map_err(|e| format!("cannot read `{path}`: {e}"))?;
    let module = match parse(&src) {
        Ok(m) => m,
        Err(e) => {
            if json {
                println!(
                    "{{\"status\":\"parse_error\",\"file\":\"{}\",\"error\":\"{}\"}}",
                    json_escape(path),
                    json_escape(&e.to_string())
                );
            } else {
                render_parse_error(path, &src, &e);
            }
            return Ok(ExitCode::FAILURE);
        }
    };

    // Handle --emit flag before verification.
    if let Some(target) = emit {
        match target {
            "ast" => {
                println!("{:#?}", module);
                return Ok(ExitCode::SUCCESS);
            }
            "core" => {
                let report = check_module(&module);
                if !report.ok() {
                    if !json {
                        eprintln!(
                            "eidos: {}: REJECTED (cannot emit core for unverified code)",
                            path
                        );
                    }
                    return Ok(ExitCode::FAILURE);
                }
                let core = tpt_eidos_erasure::erase(&module);
                println!("{:#?}", core);
                return Ok(ExitCode::SUCCESS);
            }
            other => {
                return Err(format!(
                    "unknown emit target `{other}` (expected ast or core)"
                ))
            }
        }
    }

    let report = check_module(&module);
    if report.ok() {
        if json {
            println!(
                "{{\"status\":\"verified\",\"file\":\"{}\",\"obligations\":{}}}",
                json_escape(path),
                json_obligations(&report)
            );
        } else if verbose {
            println!("eidos: {}: verified", path);
            render_obligations(path, &report);
        } else {
            println!("eidos: {}: verified ({})", path, count_ok(&report));
        }
        Ok(ExitCode::SUCCESS)
    } else {
        if json {
            println!(
                "{{\"status\":\"rejected\",\"file\":\"{}\",\"errors\":[{}],\"obligations\":{}}}",
                json_escape(path),
                json_errors(path, &src, &report),
                json_obligations(&report)
            );
        } else {
            eprintln!("eidos: {}: REJECTED", path);
            for e in &report.errors {
                render_error(path, &src, e);
            }
            if verbose {
                render_obligations(path, &report);
            }
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

fn render_obligations(_path: &str, report: &tpt_eidos_kernel::Report) {
    for o in &report.obligations {
        let tag = match o.status {
            tpt_eidos_kernel::ObligationStatus::Verified => "Verified",
            tpt_eidos_kernel::ObligationStatus::Trusted => "Trusted",
            tpt_eidos_kernel::ObligationStatus::Unverified => "Unverified",
        };
        match o.span {
            Some(Span { lo, .. }) if lo > 0 => {
                eprintln!("  [{}] {}", tag, o.description);
            }
            _ => {
                eprintln!("  [{}] {}", tag, o.description);
            }
        }
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

fn json_obligations(report: &tpt_eidos_kernel::Report) -> String {
    let items: Vec<String> = report
        .obligations
        .iter()
        .map(|o| {
            let status = match o.status {
                tpt_eidos_kernel::ObligationStatus::Verified => "verified",
                tpt_eidos_kernel::ObligationStatus::Trusted => "trusted",
                tpt_eidos_kernel::ObligationStatus::Unverified => "unverified",
            };
            format!(
                "{{\"status\":\"{}\",\"description\":\"{}\"}}",
                status,
                json_escape(&o.description)
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

fn json_errors(_path: &str, src: &str, report: &tpt_eidos_kernel::Report) -> String {
    let items: Vec<String> = report
        .errors
        .iter()
        .map(|e| {
            let (line, col) = match e.span {
                Some(Span { lo, .. }) if lo > 0 => byte_to_line_col(src, lo),
                _ => (0, 0),
            };
            format!(
                "{{\"line\":{},\"col\":{},\"message\":\"{}\"}}",
                line,
                col,
                json_escape(&e.message)
            )
        })
        .collect();
    items.join(",")
}

fn cmd_test(args: &[String]) -> Result<ExitCode, String> {
    let mut dir: Option<&str> = None;
    let mut verbose = false;
    let mut json = false;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--verbose" | "-v" => {
                verbose = true;
                i += 1;
            }
            "--json" | "-j" => {
                json = true;
                i += 1;
            }
            other if !other.starts_with('-') && dir.is_none() => {
                dir = Some(other);
                i += 1;
            }
            other => return Err(format!("unexpected test argument `{other}`")),
        }
    }
    let dir = dir.ok_or_else(|| format!("test requires a directory\n{}", usage()))?;
    let mut passed = 0u32;
    let mut failed = 0u32;
    let mut results: Vec<String> = Vec::new();

    let entries = fs::read_dir(dir).map_err(|e| format!("cannot read directory `{dir}`: {e}"))?;
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|ext| ext.to_string_lossy() == "eidos")
        })
        .collect();
    files.sort();

    for file in &files {
        let path = file.to_string_lossy();
        let src = match fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) => {
                failed += 1;
                if json {
                    results.push(format!(
                        "{{\"file\":\"{}\",\"status\":\"error\",\"message\":\"{}\"}}",
                        json_escape(&path),
                        json_escape(&e.to_string())
                    ));
                } else {
                    eprintln!("  FAIL {}: {}", path, e);
                }
                continue;
            }
        };
        let module = match parse(&src) {
            Ok(m) => m,
            Err(e) => {
                failed += 1;
                if json {
                    results.push(format!(
                        "{{\"file\":\"{}\",\"status\":\"parse_error\",\"message\":\"{}\"}}",
                        json_escape(&path),
                        json_escape(&e.to_string())
                    ));
                } else {
                    eprintln!("  FAIL {}: parse error: {}", path, e);
                }
                continue;
            }
        };
        let report = check_module(&module);
        if report.ok() {
            passed += 1;
            if json {
                results.push(format!(
                    "{{\"file\":\"{}\",\"status\":\"verified\",\"obligations\":{}}}",
                    json_escape(&path),
                    json_obligations(&report)
                ));
            } else if verbose {
                eprintln!("  OK   {}", path);
                for o in &report.obligations {
                    let tag = match o.status {
                        tpt_eidos_kernel::ObligationStatus::Verified => "Verified",
                        tpt_eidos_kernel::ObligationStatus::Trusted => "Trusted",
                        tpt_eidos_kernel::ObligationStatus::Unverified => "Unverified",
                    };
                    eprintln!("       [{}] {}", tag, o.description);
                }
            }
        } else {
            failed += 1;
            if json {
                results.push(format!(
                    "{{\"file\":\"{}\",\"status\":\"rejected\",\"errors\":[{}],\"obligations\":{}}}",
                    json_escape(&path),
                    json_errors(&path, &src, &report),
                    json_obligations(&report)
                ));
            } else {
                eprintln!("  FAIL {}", path);
                for e in &report.errors {
                    render_error(&path, &src, e);
                }
            }
        }
    }

    if json {
        println!(
            "{{\"passed\":{},\"failed\":{},\"results\":[{}]}}",
            passed,
            failed,
            results.join(",")
        );
    } else {
        println!(
            "eidos: {} passed, {} failed ({} total)",
            passed,
            failed,
            passed + failed
        );
    }

    if failed > 0 {
        Ok(ExitCode::FAILURE)
    } else {
        Ok(ExitCode::SUCCESS)
    }
}

fn cmd_build(args: &[String]) -> Result<ExitCode, String> {
    let mut file: Option<&str> = None;
    let mut out_dir: Option<String> = None;
    let mut force = false;
    let mut verbose = false;
    let mut json = false;
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
            "--verbose" | "-v" => {
                verbose = true;
                i += 1;
            }
            "--json" | "-j" => {
                json = true;
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
    let module = match parse(&src) {
        Ok(m) => m,
        Err(e) => {
            render_parse_error(file, &src, &e);
            return Ok(ExitCode::FAILURE);
        }
    };
    let report = check_module(&module);
    if !report.ok() {
        if json {
            println!(
                "{{\"status\":\"rejected\",\"file\":\"{}\",\"errors\":[{}],\"obligations\":{}}}",
                json_escape(file),
                json_errors(file, &src, &report),
                json_obligations(&report)
            );
        } else {
            eprintln!(
                "eidos: {}: REJECTED (refusing to emit unverified code)",
                file
            );
            for e in &report.errors {
                render_error(file, &src, e);
            }
        }
        return Ok(ExitCode::FAILURE);
    }

    let dir = PathBuf::from(&out_dir);
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create `{out_dir}`: {e}"))?;
    let core = tpt_eidos_erasure::erase(&module);
    let rust = tpt_eidos_codegen::codegen_with_source(&core, Some(file))
        .map_err(|e| format!("codegen failed: {e}"))?;
    let lib = dir.join("lib.rs");
    fs::write(&lib, &rust).map_err(|e| format!("cannot write `{:?}`: {e}", lib))?;
    let cargo = dir.join("Cargo.toml");
    let cargo_toml = format!(
        "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n",
        crate_name(file)
    );
    fs::write(&cargo, cargo_toml).map_err(|e| format!("cannot write `{:?}`: {e}", cargo))?;
    if json {
        println!(
            "{{\"status\":\"built\",\"file\":\"{}\",\"out_dir\":\"{}\",\"obligations\":{}}}",
            json_escape(file),
            json_escape(&out_dir),
            json_obligations(&report)
        );
    } else {
        if verbose {
            println!("eidos: {}: verified", file);
            render_obligations(file, &report);
        }
        println!(
            "eidos: {}: emitted verified no_std crate to {} (lib.rs + Cargo.toml)",
            file, out_dir
        );
    }
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
