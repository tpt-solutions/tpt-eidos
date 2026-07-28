//! `eidos` command-line tool.
//!
//! Usage:
//!   eidos new <name>                 scaffold a new .eidos project (+ Cargo workspace wrapper)
//!   eidos check <file>              verify a `.eidos` source file
//!   eidos build <file> --out-dir D  emit a verified `no_std` Rust crate (erasure + codegen)
//!   eidos fmt <file>                format a `.eidos` source file (round-trip via AST)
//!   eidos test <dir>                batch-verify all `.eidos` files in a directory
//!   eidos lsp                       start the LSP server (JSON-RPC 2.0 over stdio)
//!   eidos serve [--port P]          serve the web playground on localhost:P (default 7070)

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use tpt_eidos_kernel::check as check_module;
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
        "fmt" => cmd_fmt(args),
        "lsp" => cmd_lsp(),
        "serve" => cmd_serve(args),
        "" => Err(usage()),
        other => Err(format!("unknown subcommand `{other}`\n{}", usage())),
    }
}

fn usage() -> String {
    "usage:\n  eidos new <name>\n  eidos check <file> [--verbose] [--json] [--emit ast|core]\n  eidos build <file> --out-dir <dir> [--force] [--verbose] [--json] [--run]\n  eidos test <dir> [--verbose] [--json]\n  eidos fmt <file> [--check]\n  eidos lsp\n  eidos serve [--port <port>]\n  eidos --version\n  eidos --help"
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
    // .eidos source dir
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create `{name}`: {e}"))?;
    let src_path = dir.join(format!("{name}.eidos"));
    let src = "/// A normalized 3D vector: magnitude is at most 1.0.\n\
         type NormalizedVector3 = { v: Array<f64, 3> | v.magnitude() <= 1.0 };\n\
         \n\
         /// Calibrate a raw sensor reading and normalise the result.\n\
         /// Division by zero is provably impossible when mag > 0.\n\
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

    // Cargo workspace wrapper so `eidos build --out-dir verified/` has an obvious home.
    let workspace_toml = dir.join("Cargo.toml");
    let workspace_content = format!(
        "[workspace]\n\
         members = [\"verified/{name}\"]\n\
         resolver = \"2\"\n\
         \n\
         # Run `eidos build {name}.eidos --out-dir verified/{name}` to populate the\n\
         # workspace member, then `cargo build` to compile the verified no_std crate.\n"
    );
    fs::write(&workspace_toml, workspace_content)
        .map_err(|e| format!("cannot write `{:?}`: {e}", workspace_toml))?;

    // Pre-create the output directory so Cargo sees it immediately.
    let verified_dir = dir.join("verified").join(name);
    fs::create_dir_all(&verified_dir)
        .map_err(|e| format!("cannot create `{:?}`: {e}", verified_dir))?;

    println!("eidos: scaffolded `{name}/`");
    println!("  {name}/{name}.eidos     — eidos source");
    println!("  {name}/Cargo.toml       — Cargo workspace wrapper");
    println!("  {name}/verified/{name}/ — eidos build output directory");
    println!();
    println!("  next: eidos check {name}/{name}.eidos");
    println!("        eidos build {name}/{name}.eidos --out-dir {name}/verified/{name}/");
    println!("        cargo build --manifest-path {name}/Cargo.toml");
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
    let mut run = false;
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
            "--run" => {
                run = true;
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

    if run {
        let cargo_manifest = PathBuf::from(&out_dir).join("Cargo.toml");
        let manifest_path = cargo_manifest.to_string_lossy().into_owned();
        if !json {
            println!("eidos: invoking `cargo build --manifest-path {manifest_path}`");
        }
        let status = std::process::Command::new("cargo")
            .args(["build", "--manifest-path", &manifest_path])
            .status()
            .map_err(|e| format!("failed to invoke cargo: {e}"))?;
        if !status.success() {
            return Ok(ExitCode::FAILURE);
        }
    }

    Ok(ExitCode::SUCCESS)
}

/// Normalize source text: trim trailing whitespace from each line and ensure
/// a single trailing newline. This is the canonical format `eidos fmt` produces.
fn normalize_source(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for line in src.lines() {
        out.push_str(line.trim_end());
        out.push('\n');
    }
    // Remove any blank-line cluster at the very end, then add one newline.
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
}

fn cmd_fmt(args: &[String]) -> Result<ExitCode, String> {
    let mut file: Option<&str> = None;
    let mut check = false;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--check" => {
                check = true;
                i += 1;
            }
            other if !other.starts_with('-') && file.is_none() => {
                file = Some(other);
                i += 1;
            }
            other => return Err(format!("unexpected fmt argument `{other}`")),
        }
    }
    let path = file.ok_or_else(|| format!("fmt requires a file path\n{}", usage()))?;
    let src = fs::read_to_string(path).map_err(|e| format!("cannot read `{path}`: {e}"))?;

    // Parse to validate: eidos fmt refuses to format a broken file.
    if let Err(e) = parse(&src) {
        render_parse_error(path, &src, &e);
        return Ok(ExitCode::FAILURE);
    }

    let formatted = normalize_source(&src);

    if check {
        if src == formatted {
            println!("eidos: {path}: ok");
            Ok(ExitCode::SUCCESS)
        } else {
            eprintln!("eidos: {path}: needs formatting (run `eidos fmt {path}` to fix)");
            Ok(ExitCode::FAILURE)
        }
    } else {
        if src != formatted {
            fs::write(path, &formatted).map_err(|e| format!("cannot write `{path}`: {e}"))?;
            println!("eidos: {path}: formatted");
        } else {
            println!("eidos: {path}: ok (no changes)");
        }
        Ok(ExitCode::SUCCESS)
    }
}

fn cmd_lsp() -> Result<ExitCode, String> {
    tpt_eidos_lsp::run_lsp_server();
    Ok(ExitCode::SUCCESS)
}

fn cmd_serve(args: &[String]) -> Result<ExitCode, String> {
    let mut port: u16 = 7070;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--port" => {
                port = args
                    .get(i + 1)
                    .ok_or("--port requires a value")?
                    .parse()
                    .map_err(|_| "--port value must be a number 1-65535")?;
                i += 2;
            }
            other => return Err(format!("unexpected serve argument `{other}`")),
        }
    }

    let addr = format!("127.0.0.1:{port}");
    let listener =
        std::net::TcpListener::bind(&addr).map_err(|e| format!("cannot bind to {addr}: {e}"))?;
    println!("eidos: playground listening on http://{addr}/");
    println!("eidos: press Ctrl-C to stop");

    for stream in listener.incoming() {
        let mut stream = match stream {
            Ok(s) => s,
            Err(_) => continue,
        };
        handle_http_request(&mut stream);
    }
    Ok(ExitCode::SUCCESS)
}

fn handle_http_request(stream: &mut std::net::TcpStream) {
    use std::io::{BufRead, BufReader, Write};

    let clone = match stream.try_clone() {
        Ok(c) => c,
        Err(_) => return,
    };
    let mut reader = BufReader::new(clone);

    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let request_line = request_line.trim().to_string();

    // Read headers to find Content-Length.
    let mut content_length: usize = 0;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).is_err() {
            break;
        }
        let header = header.trim();
        if header.is_empty() {
            break;
        }
        if let Some(rest) = header.to_ascii_lowercase().strip_prefix("content-length:") {
            content_length = rest.trim().parse().unwrap_or(0);
        }
    }

    // Parse method and path.
    let mut parts = request_line.splitn(3, ' ');
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/");

    let (status, content_type, body) = if method == "GET" && (path == "/" || path == "/index.html")
    {
        ("200 OK", "text/html; charset=utf-8", playground_html())
    } else if method == "POST" && path == "/check" {
        // Read body.
        let mut body_bytes = vec![0u8; content_length.min(1 << 20)];
        let mut pos = 0;
        while pos < body_bytes.len() {
            use std::io::Read;
            match reader.read(&mut body_bytes[pos..]) {
                Ok(0) | Err(_) => break,
                Ok(n) => pos += n,
            }
        }
        let src = String::from_utf8_lossy(&body_bytes[..pos]).into_owned();
        let result = serve_check(&src);
        ("200 OK", "application/json; charset=utf-8", result)
    } else {
        ("404 Not Found", "text/plain", "not found\n".to_string())
    };

    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

fn serve_check(src: &str) -> String {
    match parse(src) {
        Err(e) => {
            format!(
                "{{\"status\":\"parse_error\",\"message\":\"{}\"}}",
                json_escape(&e.to_string())
            )
        }
        Ok(module) => {
            let report = check_module(&module);
            if report.ok() {
                let n_verified = report
                    .obligations
                    .iter()
                    .filter(|o| matches!(o.status, tpt_eidos_kernel::ObligationStatus::Verified))
                    .count();
                format!(
                    "{{\"status\":\"verified\",\"obligations\":{},\"message\":\"All {n_verified} obligation(s) verified.\"}}",
                    json_obligations(&report)
                )
            } else {
                let errors: Vec<String> = report
                    .errors
                    .iter()
                    .map(|e| format!("\"{}\"", json_escape(&e.message)))
                    .collect();
                format!(
                    "{{\"status\":\"rejected\",\"obligations\":{},\"errors\":[{}]}}",
                    json_obligations(&report),
                    errors.join(",")
                )
            }
        }
    }
}

fn playground_html() -> String {
    r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>eidos playground</title>
<style>
* { box-sizing: border-box; margin: 0; padding: 0; }
body { font-family: system-ui, sans-serif; background: #0f1117; color: #e2e8f0; height: 100vh; display: flex; flex-direction: column; }
header { padding: 12px 20px; background: #1a1d2e; border-bottom: 1px solid #2d3148; display: flex; align-items: center; gap: 12px; }
header h1 { font-size: 1.1rem; font-weight: 600; color: #a78bfa; }
header span { font-size: 0.8rem; color: #64748b; }
main { flex: 1; display: grid; grid-template-columns: 1fr 1fr; gap: 0; overflow: hidden; }
.pane { display: flex; flex-direction: column; overflow: hidden; }
.pane-label { padding: 8px 16px; font-size: 0.75rem; font-weight: 600; text-transform: uppercase; letter-spacing: 0.05em; color: #64748b; background: #141624; border-bottom: 1px solid #2d3148; }
#editor { flex: 1; width: 100%; background: #0f1117; color: #e2e8f0; border: none; border-right: 1px solid #2d3148; padding: 16px; font-family: "JetBrains Mono", "Fira Code", monospace; font-size: 0.875rem; line-height: 1.6; resize: none; outline: none; tab-size: 4; }
#output { flex: 1; padding: 16px; font-family: "JetBrains Mono", "Fira Code", monospace; font-size: 0.85rem; line-height: 1.6; overflow-y: auto; white-space: pre-wrap; }
footer { padding: 8px 20px; background: #141624; border-top: 1px solid #2d3148; display: flex; gap: 12px; align-items: center; }
button { padding: 6px 18px; border-radius: 6px; border: none; cursor: pointer; font-size: 0.875rem; font-weight: 600; transition: background 0.15s; }
#btn-check { background: #7c3aed; color: white; }
#btn-check:hover { background: #6d28d9; }
#btn-check:disabled { background: #4c1d95; opacity: 0.6; cursor: not-allowed; }
.ok { color: #4ade80; }
.err { color: #f87171; }
.pending { color: #94a3b8; }
</style>
</head>
<body>
<header>
  <h1>eidos playground</h1>
  <span>tpt-eidos proof-native systems language</span>
</header>
<main>
  <div class="pane">
    <div class="pane-label">Source (.eidos)</div>
    <textarea id="editor" spellcheck="false">/// Divide x by y.
/// Division safety requires y != 0.
fn divide(x: f64, y: f64) -> f64
requires y != 0.0
ensures |result| result == x / y
{
    return x / y;
}
</textarea>
  </div>
  <div class="pane">
    <div class="pane-label">Verification result</div>
    <div id="output" class="pending">Press "Verify" to check your code.</div>
  </div>
</main>
<footer>
  <button id="btn-check">Verify</button>
  <span id="status" style="font-size:0.8rem;color:#64748b;"></span>
</footer>
<script>
const btn = document.getElementById('btn-check');
const out = document.getElementById('output');
const status = document.getElementById('status');
const editor = document.getElementById('editor');

editor.addEventListener('keydown', e => {
  if (e.key === 'Tab') {
    e.preventDefault();
    const s = editor.selectionStart;
    editor.setRangeText('    ', s, s, 'end');
  }
});

btn.addEventListener('click', async () => {
  btn.disabled = true;
  out.className = 'pending';
  out.textContent = 'Verifying...';
  status.textContent = '';
  try {
    const t0 = performance.now();
    const resp = await fetch('/check', { method: 'POST', body: editor.value });
    const data = await resp.json();
    const ms = (performance.now() - t0).toFixed(0);
    status.textContent = `${ms}ms`;
    if (data.status === 'verified') {
      out.className = 'ok';
      const obs = (data.obligations || []).map(o => `  [${o.status}] ${o.description}`).join('\n');
      out.textContent = '✓ ' + data.message + (obs ? '\n\n' + obs : '');
    } else if (data.status === 'rejected') {
      out.className = 'err';
      const errs = (data.errors || []).join('\n');
      const obs = (data.obligations || []).map(o => `  [${o.status}] ${o.description}`).join('\n');
      out.textContent = '✗ Rejected\n\n' + errs + (obs ? '\n\n' + obs : '');
    } else {
      out.className = 'err';
      out.textContent = '✗ ' + (data.message || JSON.stringify(data));
    }
  } catch (e) {
    out.className = 'err';
    out.textContent = '✗ Network error: ' + e.message;
  } finally {
    btn.disabled = false;
  }
});
</script>
</body>
</html>"#.to_string()
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
