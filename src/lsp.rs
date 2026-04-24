use crate::tooling::{self, DiagnosticSeverity, InspectParam};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufWriter, Write};
use std::path::PathBuf;

const COOL_KEYWORDS: &[&str] = &[
    "def", "class", "struct", "packed", "if", "elif", "else", "while", "for", "in", "not", "and",
    "or", "return", "break", "continue", "pass", "import", "from", "as", "try", "except",
    "finally", "raise", "with", "lambda", "assert", "global", "nonlocal", "True", "False", "None",
];

const COOL_BUILTINS: &[&str] = &[
    "print", "len", "range", "input", "str", "int", "float", "bool", "list", "dict", "tuple",
    "set", "min", "max", "sum", "abs", "round", "sorted", "reversed", "enumerate", "zip", "map",
    "filter", "type", "isinstance", "hasattr", "getattr", "open", "repr", "ord", "chr", "hex",
    "bin", "oct", "any", "all", "callable",
];

const COOL_MODULES: &[&str] = &[
    "argparse", "collections", "csv", "datetime", "ffi", "hashlib", "http", "json", "list",
    "logging", "math", "os", "path", "random", "re", "socket", "sqlite", "string", "subprocess",
    "sys", "term", "test", "time", "toml", "yaml",
];

struct LspServer {
    open_files: HashMap<String, String>,
    should_exit: bool,
}

impl LspServer {
    fn new() -> Self {
        Self {
            open_files: HashMap::new(),
            should_exit: false,
        }
    }

    fn handle(&mut self, msg: &Value) -> Vec<Value> {
        let Some(method) = msg.get("method").and_then(|m| m.as_str()) else {
            return vec![];
        };
        let id = msg.get("id").cloned();
        let params = msg.get("params").cloned().unwrap_or(Value::Null);

        match method {
            "initialize" => vec![self.handle_initialize(id)],
            "initialized" => vec![],
            "shutdown" => match id {
                Some(id) => vec![json!({"jsonrpc": "2.0", "id": id, "result": null})],
                None => vec![],
            },
            "exit" => {
                self.should_exit = true;
                vec![]
            }
            "textDocument/didOpen" => self.handle_did_open(&params),
            "textDocument/didChange" => self.handle_did_change(&params),
            "textDocument/didClose" => self.handle_did_close(&params),
            "textDocument/completion" => vec![self.handle_completion(id, &params)],
            "textDocument/hover" => vec![self.handle_hover(id, &params)],
            "textDocument/definition" => vec![self.handle_definition(id, &params)],
            "textDocument/documentSymbol" => vec![self.handle_document_symbol(id, &params)],
            "workspace/symbol" => vec![self.handle_workspace_symbol(id, &params)],
            _ => match id {
                Some(id) => vec![json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {"code": -32601, "message": "Method not found"}
                })],
                None => vec![],
            },
        }
    }

    fn handle_initialize(&self, id: Option<Value>) -> Value {
        let id = id.unwrap_or(Value::Null);
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "capabilities": {
                    "positionEncoding": "utf-16",
                    "textDocumentSync": {"openClose": true, "change": 1},
                    "completionProvider": {"triggerCharacters": ["."]},
                    "hoverProvider": true,
                    "definitionProvider": true,
                    "documentSymbolProvider": true,
                    "workspaceSymbolProvider": true
                },
                "serverInfo": {"name": "cool-lsp", "version": "1.0.0"}
            }
        })
    }

    fn handle_did_open(&mut self, params: &Value) -> Vec<Value> {
        let Some(uri) = extract_uri(params, "textDocument") else {
            return vec![];
        };
        let content = params["textDocument"]["text"].as_str().unwrap_or("").to_string();
        self.open_files.insert(uri.clone(), content.clone());
        let diags = compute_diagnostics(&uri, &content);
        vec![publish_diagnostics(&uri, diags)]
    }

    fn handle_did_change(&mut self, params: &Value) -> Vec<Value> {
        let Some(uri) = extract_uri(params, "textDocument") else {
            return vec![];
        };
        let content = params["contentChanges"][0]["text"].as_str().unwrap_or("").to_string();
        self.open_files.insert(uri.clone(), content.clone());
        let diags = compute_diagnostics(&uri, &content);
        vec![publish_diagnostics(&uri, diags)]
    }

    fn handle_did_close(&mut self, params: &Value) -> Vec<Value> {
        let Some(uri) = extract_uri(params, "textDocument") else {
            return vec![];
        };
        self.open_files.remove(&uri);
        vec![publish_diagnostics(&uri, vec![])]
    }

    fn handle_completion(&self, id: Option<Value>, params: &Value) -> Value {
        let id = id.unwrap_or(Value::Null);
        let Some(uri) = extract_uri(params, "textDocument") else {
            return json!({"jsonrpc": "2.0", "id": id, "result": []});
        };
        let line_num = params["position"]["line"].as_u64().unwrap_or(0) as usize;
        let char_num = params["position"]["character"].as_u64().unwrap_or(0) as usize;
        let content = self.open_files.get(&uri).map(String::as_str).unwrap_or("");
        let prefix = get_line_prefix(content, line_num, char_num);
        let items = compute_completions(content, &uri, &prefix);
        json!({"jsonrpc": "2.0", "id": id, "result": items})
    }

    fn handle_hover(&self, id: Option<Value>, params: &Value) -> Value {
        let id = id.unwrap_or(Value::Null);
        let Some(uri) = extract_uri(params, "textDocument") else {
            return json!({"jsonrpc": "2.0", "id": id, "result": null});
        };
        let line_num = params["position"]["line"].as_u64().unwrap_or(0) as usize;
        let char_num = params["position"]["character"].as_u64().unwrap_or(0) as usize;
        let content = self.open_files.get(&uri).map(String::as_str).unwrap_or("");
        let result = word_at_position(content, line_num, char_num)
            .and_then(|word| {
                let report = tooling::inspect_source(content, &uri);
                hover_markdown(&word, &report)
                    .map(|md| json!({"contents": {"kind": "markdown", "value": md}}))
            })
            .unwrap_or(Value::Null);
        json!({"jsonrpc": "2.0", "id": id, "result": result})
    }

    fn handle_definition(&self, id: Option<Value>, params: &Value) -> Value {
        let id = id.unwrap_or(Value::Null);
        let Some(uri) = extract_uri(params, "textDocument") else {
            return json!({"jsonrpc": "2.0", "id": id, "result": null});
        };
        let line_num = params["position"]["line"].as_u64().unwrap_or(0) as usize;
        let char_num = params["position"]["character"].as_u64().unwrap_or(0) as usize;
        let content = self.open_files.get(&uri).map(String::as_str).unwrap_or("");
        let result = word_at_position(content, line_num, char_num)
            .and_then(|word| {
                let report = tooling::inspect_source(content, &uri);
                find_definition_in_report(&word, &report, &uri).or_else(|| {
                    self.open_files.iter().filter(|(u, _)| u.as_str() != uri).find_map(
                        |(other_uri, other_content)| {
                            let r = tooling::inspect_source(other_content, other_uri);
                            find_definition_in_report(&word, &r, other_uri)
                        },
                    )
                })
            })
            .unwrap_or(Value::Null);
        json!({"jsonrpc": "2.0", "id": id, "result": result})
    }

    fn handle_document_symbol(&self, id: Option<Value>, params: &Value) -> Value {
        let id = id.unwrap_or(Value::Null);
        let Some(uri) = extract_uri(params, "textDocument") else {
            return json!({"jsonrpc": "2.0", "id": id, "result": []});
        };
        let content = self.open_files.get(&uri).map(String::as_str).unwrap_or("");
        let report = tooling::inspect_source(content, &uri);
        let symbols = report_to_symbols(&report, &uri);
        json!({"jsonrpc": "2.0", "id": id, "result": symbols})
    }

    fn handle_workspace_symbol(&self, id: Option<Value>, params: &Value) -> Value {
        let id = id.unwrap_or(Value::Null);
        let query = params["query"].as_str().unwrap_or("").to_lowercase();
        let mut symbols = Vec::new();
        for (uri, content) in &self.open_files {
            let report = tooling::inspect_source(content, uri);
            symbols.extend(report_to_symbols(&report, uri));
        }
        let filtered: Vec<Value> = if query.is_empty() {
            symbols
        } else {
            symbols
                .into_iter()
                .filter(|s| s["name"].as_str().unwrap_or("").to_lowercase().contains(&query))
                .collect()
        };
        json!({"jsonrpc": "2.0", "id": id, "result": filtered})
    }
}

// ── Protocol framing ──────────────────────────────────────────────────────────

fn read_message(reader: &mut impl BufRead) -> Result<Option<Value>, String> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if n == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(|c: char| c == '\r' || c == '\n');
        if trimmed.is_empty() {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix("Content-Length: ") {
            content_length = rest.trim().parse().ok();
        }
    }
    let len = match content_length {
        Some(l) => l,
        None => return Ok(None),
    };
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).map_err(|e| e.to_string())?;
    let msg: Value = serde_json::from_slice(&buf).map_err(|e| format!("invalid JSON: {e}"))?;
    Ok(Some(msg))
}

fn write_message(writer: &mut impl Write, msg: &Value) -> Result<(), String> {
    let body = serde_json::to_string(msg).map_err(|e| e.to_string())?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    writer.write_all(header.as_bytes()).map_err(|e| e.to_string())?;
    writer.write_all(body.as_bytes()).map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())?;
    Ok(())
}

// ── Notification helpers ──────────────────────────────────────────────────────

fn publish_diagnostics(uri: &str, diagnostics: Vec<Value>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": {"uri": uri, "diagnostics": diagnostics}
    })
}

fn compute_diagnostics(uri: &str, content: &str) -> Vec<Value> {
    let path = uri_to_path(uri)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| uri.to_string());
    tooling::diagnose_source(content, &path)
        .iter()
        .map(|d| {
            let lsp_line = d.line.unwrap_or(1).saturating_sub(1) as u64;
            let severity = match d.severity {
                DiagnosticSeverity::Error => 1u64,
                DiagnosticSeverity::Warning => 2u64,
            };
            json!({
                "range": {
                    "start": {"line": lsp_line, "character": 0},
                    "end":   {"line": lsp_line, "character": 999}
                },
                "severity": severity,
                "code": d.code,
                "source": "cool",
                "message": d.message
            })
        })
        .collect()
}

// ── Symbol helpers ────────────────────────────────────────────────────────────

fn report_to_symbols(report: &tooling::InspectReport, uri: &str) -> Vec<Value> {
    let mut symbols = Vec::new();
    let loc = |line: Option<usize>| {
        let lsp_line = line.unwrap_or(1).saturating_sub(1) as u64;
        json!({
            "uri": uri,
            "range": {
                "start": {"line": lsp_line, "character": 0},
                "end":   {"line": lsp_line, "character": 0}
            }
        })
    };
    for f in &report.functions {
        symbols.push(json!({"name": f.name, "kind": 12, "location": loc(f.line)}));
    }
    for c in &report.classes {
        symbols.push(json!({"name": c.name, "kind": 5, "location": loc(c.line)}));
        for m in &c.methods {
            symbols.push(json!({
                "name": m.name, "kind": 6,
                "containerName": c.name, "location": loc(m.line)
            }));
        }
    }
    for s in &report.structs {
        symbols.push(json!({"name": s.name, "kind": 23, "location": loc(s.line)}));
    }
    for a in &report.assignments {
        if matches!(a.kind, "assign" | "unpack") {
            for name in &a.names {
                symbols.push(json!({"name": name, "kind": 13, "location": loc(a.line)}));
            }
        }
    }
    symbols
}

fn find_definition_in_report(
    word: &str,
    report: &tooling::InspectReport,
    uri: &str,
) -> Option<Value> {
    let location = |line: Option<usize>| {
        let lsp_line = line.unwrap_or(1).saturating_sub(1) as u64;
        json!({
            "uri": uri,
            "range": {
                "start": {"line": lsp_line, "character": 0},
                "end":   {"line": lsp_line, "character": 0}
            }
        })
    };
    if let Some(f) = report.functions.iter().find(|f| f.name == word) {
        return Some(location(f.line));
    }
    if let Some(c) = report.classes.iter().find(|c| c.name == word) {
        return Some(location(c.line));
    }
    if let Some(s) = report.structs.iter().find(|s| s.name == word) {
        return Some(location(s.line));
    }
    for a in &report.assignments {
        if a.names.iter().any(|n| n == word) {
            return Some(location(a.line));
        }
    }
    None
}

// ── Hover ─────────────────────────────────────────────────────────────────────

fn hover_markdown(word: &str, report: &tooling::InspectReport) -> Option<String> {
    if let Some(f) = report.functions.iter().find(|f| f.name == word) {
        let sig = format!("def {}({})", f.name, fmt_params(&f.params));
        return Some(format!("```cool\n{sig}\n```"));
    }
    if let Some(c) = report.classes.iter().find(|c| c.name == word) {
        if let Some(init) = c.methods.iter().find(|m| m.name == "__init__") {
            let non_self: Vec<&InspectParam> =
                init.params.iter().filter(|p| p.name != "self").collect();
            let sig = format!("class {}({})", c.name, fmt_params_slice(&non_self));
            return Some(format!("```cool\n{sig}\n```"));
        }
        return Some(format!("```cool\nclass {}\n```", c.name));
    }
    if let Some(s) = report.structs.iter().find(|s| s.name == word) {
        let fields: Vec<String> = s
            .fields
            .iter()
            .map(|f| format!("    {}: {}", f.name, f.type_name))
            .collect();
        let kw = if s.is_packed { "packed struct" } else { "struct" };
        return Some(format!("```cool\n{kw} {}:\n{}\n```", s.name, fields.join("\n")));
    }
    None
}

// ── Completions ───────────────────────────────────────────────────────────────

fn compute_completions(content: &str, uri: &str, line_prefix: &str) -> Vec<Value> {
    let trimmed = line_prefix.trim_start();
    if trimmed.starts_with("import ") || trimmed == "import" || trimmed.starts_with("from ") {
        return COOL_MODULES
            .iter()
            .map(|m| json!({"label": m, "kind": 9}))
            .collect();
    }
    let mut items = Vec::new();
    for kw in COOL_KEYWORDS {
        items.push(json!({"label": kw, "kind": 14, "detail": "keyword"}));
    }
    for b in COOL_BUILTINS {
        items.push(json!({"label": b, "kind": 3, "detail": "builtin"}));
    }
    let report = tooling::inspect_source(content, uri);
    for f in &report.functions {
        items.push(json!({
            "label": f.name,
            "kind": 3,
            "detail": format!("def {}({})", f.name, fmt_params(&f.params))
        }));
    }
    for c in &report.classes {
        items.push(json!({"label": c.name, "kind": 7, "detail": format!("class {}", c.name)}));
    }
    for s in &report.structs {
        items.push(json!({"label": s.name, "kind": 22, "detail": format!("struct {}", s.name)}));
    }
    for a in &report.assignments {
        if matches!(a.kind, "assign" | "unpack") {
            for name in &a.names {
                items.push(json!({"label": name, "kind": 6}));
            }
        }
    }
    items
}

// ── Text helpers ──────────────────────────────────────────────────────────────

fn get_line_prefix(source: &str, line: usize, character: usize) -> String {
    source
        .lines()
        .nth(line)
        .map(|l| l.chars().take(character).collect())
        .unwrap_or_default()
}

fn word_at_position(source: &str, line: usize, character: usize) -> Option<String> {
    let text_line = source.lines().nth(line)?;
    let chars: Vec<char> = text_line.chars().collect();
    if character > chars.len() {
        return None;
    }
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let start = chars[..character]
        .iter()
        .rposition(|c| !is_word(*c))
        .map(|p| p + 1)
        .unwrap_or(0);
    let end = chars[character..]
        .iter()
        .position(|c| !is_word(*c))
        .map(|p| p + character)
        .unwrap_or(chars.len());
    if start >= end {
        return None;
    }
    Some(chars[start..end].iter().collect())
}

// ── URI / path helpers ────────────────────────────────────────────────────────

fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let path_str = uri.strip_prefix("file://")?;
    Some(PathBuf::from(percent_decode(path_str)))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut result = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    result.push(byte as char);
                    i += 3;
                    continue;
                }
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

fn extract_uri(params: &Value, key: &str) -> Option<String> {
    params[key]["uri"].as_str().map(String::from)
}

// ── Param formatting ──────────────────────────────────────────────────────────

fn fmt_params(params: &[InspectParam]) -> String {
    params
        .iter()
        .map(|p| {
            let mut s = if p.is_vararg {
                format!("*{}", p.name)
            } else if p.is_kwarg {
                format!("**{}", p.name)
            } else {
                p.name.clone()
            };
            if p.has_default {
                s.push_str("=...");
            }
            s
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn fmt_params_slice(params: &[&InspectParam]) -> String {
    params
        .iter()
        .map(|p| {
            let mut s = if p.is_vararg {
                format!("*{}", p.name)
            } else if p.is_kwarg {
                format!("**{}", p.name)
            } else {
                p.name.clone()
            };
            if p.has_default {
                s.push_str("=...");
            }
            s
        })
        .collect::<Vec<_>>()
        .join(", ")
}

// ── Public entry point ────────────────────────────────────────────────────────

pub fn run_server() -> Result<(), String> {
    let stdin = std::io::stdin();
    let mut reader = std::io::BufReader::new(stdin.lock());
    let stdout = std::io::stdout();
    let mut writer = BufWriter::new(stdout.lock());
    let mut server = LspServer::new();

    loop {
        match read_message(&mut reader) {
            Ok(Some(msg)) => {
                let responses = server.handle(&msg);
                for response in responses {
                    if let Err(e) = write_message(&mut writer, &response) {
                        eprintln!("cool lsp: write error: {e}");
                        return Ok(());
                    }
                }
            }
            Ok(None) => break,
            Err(e) => {
                eprintln!("cool lsp: {e}");
                break;
            }
        }
        if server.should_exit {
            break;
        }
    }

    Ok(())
}
