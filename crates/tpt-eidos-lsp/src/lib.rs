//! Minimal LSP (Language Server Protocol) server for tpt-eidos.
//!
//! Implements the JSON-RPC 2.0 protocol over stdio with the subset of LSP
//! messages needed for real-time diagnostics on `.eidos` files. No external
//! crates — pure `std`.
//!
//! Supported lifecycle: `initialize` → `initialized` → `textDocument/didOpen`
//! / `textDocument/didChange` / `textDocument/didClose` → `shutdown` → `exit`.
//!
//! On every open/change, the source is re-parsed and kernel-checked; errors are
//! published via `textDocument/publishDiagnostics`.

use std::io::{self, BufRead, Write};

use tpt_eidos_flight_math::check_module;
use tpt_eidos_parser::parse;

// ─── Minimal JSON helpers (no external crates) ───────────────────────────────

/// Extract a string value for `key` from a JSON object string.
/// Returns `None` if the key is absent or the value is not a JSON string.
fn json_str(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\":", key);
    let start = json.find(&needle)?;
    let after = json[start + needle.len()..].trim_start();
    if !after.starts_with('"') {
        return None;
    }
    let inner = &after[1..];
    let mut out = String::new();
    let mut chars = inner.chars();
    loop {
        match chars.next()? {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    if let Ok(n) = u32::from_str_radix(&hex, 16) {
                        if let Some(c) = char::from_u32(n) {
                            out.push(c);
                        }
                    }
                }
                c => out.push(c),
            },
            c => out.push(c),
        }
    }
}

/// Extract a numeric (integer) value for `key` from a JSON object string.
fn json_i64(json: &str, key: &str) -> Option<i64> {
    let needle = format!("\"{}\":", key);
    let start = json.find(&needle)?;
    let after = json[start + needle.len()..].trim_start();
    let end = after
        .find(|c: char| !c.is_ascii_digit() && c != '-')
        .unwrap_or(after.len());
    after[..end].parse().ok()
}

/// JSON-escape a string (for embedding in a JSON response).
fn json_esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

// ─── LSP wire protocol ───────────────────────────────────────────────────────

fn send(writer: &mut impl Write, body: &str) {
    let _ = write!(writer, "Content-Length: {}\r\n\r\n{}", body.len(), body);
    let _ = writer.flush();
}

fn response_ok(id: i64, result: &str) -> String {
    format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{result}}}")
}

fn notification(method: &str, params: &str) -> String {
    format!("{{\"jsonrpc\":\"2.0\",\"method\":\"{method}\",\"params\":{params}}}")
}

// ─── Diagnostics ─────────────────────────────────────────────────────────────

fn byte_to_lsp_pos(src: &str, offset: usize) -> (u32, u32) {
    let mut line: u32 = 0;
    let mut col: u32 = 0;
    for (i, c) in src.char_indices() {
        if i >= offset {
            break;
        }
        if c == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}

fn diagnostics_json(src: &str, report: &tpt_eidos_kernel::Report) -> String {
    let items: Vec<String> = report
        .errors
        .iter()
        .map(|e| {
            let (line, col) = match e.span {
                Some(tpt_eidos_parser::Span { lo, .. }) if lo > 0 => byte_to_lsp_pos(src, lo),
                _ => (0, 0),
            };
            format!(
                "{{\"range\":{{\"start\":{{\"line\":{line},\"character\":{col}}},\
                 \"end\":{{\"line\":{line},\"character\":{col}}}}},\
                 \"severity\":1,\"message\":\"{}\"}}",
                json_esc(&e.message)
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

fn publish_diagnostics(writer: &mut impl Write, uri: &str, src: &str) {
    let diags = match parse(src) {
        Err(e) => {
            let (line, col) = match e.span {
                Some(tpt_eidos_parser::Span { lo, .. }) if lo > 0 => byte_to_lsp_pos(src, lo),
                _ => (0, 0),
            };
            format!(
                "[{{\"range\":{{\"start\":{{\"line\":{line},\"character\":{col}}},\
                 \"end\":{{\"line\":{line},\"character\":{col}}}}},\
                 \"severity\":1,\"message\":\"parse error: {}\"}}]",
                json_esc(&e.to_string())
            )
        }
        Ok(module) => {
            let report = check_module(&module);
            diagnostics_json(src, &report)
        }
    };
    let params = format!("{{\"uri\":\"{}\",\"diagnostics\":{diags}}}", json_esc(uri));
    send(
        writer,
        &notification("textDocument/publishDiagnostics", &params),
    );
}

// ─── Main server loop ─────────────────────────────────────────────────────────

/// Run the LSP server. Reads JSON-RPC messages from `stdin`, writes responses
/// to `stdout`. Returns when the client sends `exit`.
pub fn run_lsp_server() {
    run_server(io::stdin().lock(), io::stdout())
}

pub fn run_server(mut input: impl BufRead, mut output: impl Write) {
    // Map from URI → current source text.
    let mut open_docs: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut initialized = false;

    loop {
        // Read headers until blank line.
        let mut content_length: usize = 0;
        loop {
            let mut line = String::new();
            match input.read_line(&mut line) {
                Ok(0) | Err(_) => return, // EOF / error → exit
                _ => {}
            }
            let line = line.trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                break;
            }
            if let Some(rest) = line.strip_prefix("Content-Length:") {
                if let Ok(n) = rest.trim().parse::<usize>() {
                    content_length = n;
                }
            }
        }
        if content_length == 0 {
            continue;
        }
        // Read body.
        let mut buf = vec![0u8; content_length];
        if read_exact_bytes(&mut input, &mut buf).is_err() {
            return;
        }
        let json = match std::str::from_utf8(&buf) {
            Ok(s) => s.to_string(),
            Err(_) => continue,
        };

        let method = match json_str(&json, "method") {
            Some(m) => m,
            None => continue,
        };
        let id = json_i64(&json, "id");

        match method.as_str() {
            "initialize" => {
                let result = "{\
                    \"capabilities\":{\
                        \"textDocumentSync\":1\
                    },\
                    \"serverInfo\":{\
                        \"name\":\"eidos-lsp\",\
                        \"version\":\"0.2.0\"\
                    }\
                }";
                if let Some(id) = id {
                    send(&mut output, &response_ok(id, result));
                }
            }
            "initialized" => {
                initialized = true;
            }
            "textDocument/didOpen" => {
                if initialized {
                    if let (Some(uri), Some(text)) =
                        (json_str(&json, "uri"), json_str(&json, "text"))
                    {
                        publish_diagnostics(&mut output, &uri, &text);
                        open_docs.insert(uri, text);
                    }
                }
            }
            "textDocument/didChange" => {
                if initialized {
                    if let Some(uri) = json_str(&json, "uri") {
                        // The last `text` in contentChanges (full-document sync).
                        if let Some(text) = json_str(&json, "text") {
                            publish_diagnostics(&mut output, &uri, &text);
                            open_docs.insert(uri, text);
                        }
                    }
                }
            }
            "textDocument/didClose" => {
                if let Some(uri) = json_str(&json, "uri") {
                    open_docs.remove(&uri);
                    // Clear diagnostics on close.
                    let params = format!("{{\"uri\":\"{}\",\"diagnostics\":[]}}", json_esc(&uri));
                    send(
                        &mut output,
                        &notification("textDocument/publishDiagnostics", &params),
                    );
                }
            }
            "shutdown" => {
                if let Some(id) = id {
                    send(&mut output, &response_ok(id, "null"));
                }
            }
            "exit" => return,
            _ => {
                // Respond with method-not-found for requests (which have an id).
                if let Some(id) = id {
                    let err = format!(
                        "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"error\":{{\"code\":-32601,\"message\":\"Method not found\"}}}}",
                    );
                    send(&mut output, &err);
                }
            }
        }
    }
}

/// Read exactly `buf.len()` bytes from a `BufRead`. Returns `Err` on EOF/error.
fn read_exact_bytes(mut reader: impl BufRead, buf: &mut [u8]) -> Result<(), ()> {
    let mut pos = 0;
    while pos < buf.len() {
        let available = reader.fill_buf().map_err(|_| ())?;
        if available.is_empty() {
            return Err(());
        }
        let take = (buf.len() - pos).min(available.len());
        buf[pos..pos + take].copy_from_slice(&available[..take]);
        reader.consume(take);
        pos += take;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_str_extracts_simple() {
        let j = r#"{"method":"initialize","id":1}"#;
        assert_eq!(json_str(j, "method"), Some("initialize".to_string()));
    }

    #[test]
    fn json_str_handles_escape() {
        let j = r#"{"text":"fn f() -> f64 {\n    return 1.0;\n}"}"#;
        let t = json_str(j, "text").unwrap();
        assert!(t.contains('\n'));
    }

    #[test]
    fn json_i64_extracts() {
        let j = r#"{"id":42,"method":"shutdown"}"#;
        assert_eq!(json_i64(j, "id"), Some(42));
    }

    #[test]
    fn json_esc_escapes_specials() {
        let s = json_esc("a\"b\nc");
        assert_eq!(s, r#"a\"b\nc"#);
    }

    #[test]
    fn lsp_session_initialize_shutdown() {
        fn msg(body: &str) -> Vec<u8> {
            format!("Content-Length: {}\r\n\r\n{}", body.len(), body).into_bytes()
        }
        let mut input: Vec<u8> = Vec::new();
        input.extend(msg(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        ));
        input.extend(msg(
            r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#,
        ));
        input.extend(msg(r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#));
        input.extend(msg(r#"{"jsonrpc":"2.0","method":"exit"}"#));

        let cursor = std::io::Cursor::new(input);
        let reader = std::io::BufReader::new(cursor);
        let mut output: Vec<u8> = Vec::new();
        run_server(reader, &mut output);
        let out = String::from_utf8(output).unwrap();
        assert!(out.contains("\"id\":1"), "missing initialize response");
        assert!(out.contains("capabilities"), "missing capabilities");
        assert!(out.contains("\"id\":2"), "missing shutdown response");
    }

    #[test]
    fn lsp_publishes_diagnostics_on_open() {
        fn msg(body: &str) -> Vec<u8> {
            format!("Content-Length: {}\r\n\r\n{}", body.len(), body).into_bytes()
        }
        let broken_source = "fn f(x: f64) -> f64 { return x / x; }";
        let open_body = format!(
            r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"file:///test.eidos","text":"{broken_source}","version":1,"languageId":"eidos"}}}}}}"#
        );

        let mut input: Vec<u8> = Vec::new();
        input.extend(msg(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        ));
        input.extend(msg(
            r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#,
        ));
        input.extend(msg(&open_body));
        input.extend(msg(r#"{"jsonrpc":"2.0","method":"exit"}"#));

        let cursor = std::io::Cursor::new(input);
        let reader = std::io::BufReader::new(cursor);
        let mut output: Vec<u8> = Vec::new();
        run_server(reader, &mut output);
        let out = String::from_utf8(output).unwrap();
        assert!(
            out.contains("publishDiagnostics"),
            "expected publishDiagnostics in:\n{out}"
        );
    }
}
