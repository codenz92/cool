use crate::module_exports;
use crate::tooling::{self, DiagnosticSeverity, InspectAssignment, InspectParam};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufWriter, Write};
use std::path::PathBuf;

const COOL_KEYWORDS: &[&str] = &[
    "def", "data", "extern", "class", "struct", "packed", "union", "if", "elif", "else", "while", "for", "in", "not",
    "and", "or", "return", "break", "continue", "pass", "import", "from", "as", "try", "except", "finally", "raise",
    "with", "lambda", "assert", "global", "nonlocal", "const", "public", "private", "True", "False", "None",
];

const COOL_BUILTINS: &[&str] = &[
    "print",
    "len",
    "range",
    "input",
    "str",
    "int",
    "float",
    "bool",
    "list",
    "dict",
    "tuple",
    "set",
    "min",
    "max",
    "sum",
    "abs",
    "round",
    "sorted",
    "reversed",
    "enumerate",
    "zip",
    "map",
    "filter",
    "type",
    "isinstance",
    "hasattr",
    "getattr",
    "open",
    "repr",
    "ord",
    "chr",
    "hex",
    "bin",
    "oct",
    "any",
    "all",
    "callable",
    "i8",
    "u8",
    "i16",
    "u16",
    "i32",
    "u32",
    "i64",
    "isize",
    "usize",
    "word_bits",
    "word_bytes",
    "asm",
    "malloc",
    "free",
    "read_byte",
    "write_byte",
    "read_i8",
    "write_i8",
    "read_u8",
    "write_u8",
    "read_i16",
    "write_i16",
    "read_u16",
    "write_u16",
    "read_i32",
    "write_i32",
    "read_u32",
    "write_u32",
    "read_i64",
    "write_i64",
    "read_f64",
    "write_f64",
    "read_str",
    "write_str",
    "read_byte_volatile",
    "write_byte_volatile",
    "read_i8_volatile",
    "write_i8_volatile",
    "read_u8_volatile",
    "write_u8_volatile",
    "read_i16_volatile",
    "write_i16_volatile",
    "read_u16_volatile",
    "write_u16_volatile",
    "read_i32_volatile",
    "write_i32_volatile",
    "read_u32_volatile",
    "write_u32_volatile",
    "read_i64_volatile",
    "write_i64_volatile",
    "read_f64_volatile",
    "write_f64_volatile",
    "outb",
    "inb",
    "write_serial_byte",
];

const COOL_MODULES: &[&str] = &[
    "argparse",
    "collections",
    "core",
    "csv",
    "datetime",
    "ffi",
    "hashlib",
    "http",
    "json",
    "list",
    "logging",
    "math",
    "os",
    "path",
    "platform",
    "random",
    "re",
    "socket",
    "sqlite",
    "string",
    "subprocess",
    "sys",
    "term",
    "test",
    "time",
    "toml",
    "yaml",
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
                hover_markdown(&word, &report).map(|md| json!({"contents": {"kind": "markdown", "value": md}}))
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
                    self.open_files
                        .iter()
                        .filter(|(u, _)| u.as_str() != uri)
                        .find_map(|(other_uri, other_content)| {
                            let r = tooling::inspect_source(other_content, other_uri);
                            find_definition_in_report(&word, &r, other_uri)
                        })
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
        if matches!(a.kind, "assign" | "unpack" | "var_decl" | "const") {
            for name in &a.names {
                symbols.push(json!({"name": name, "kind": if a.is_const { 14 } else { 13 }, "location": loc(a.line)}));
            }
        }
    }
    symbols
}

fn find_definition_in_report(word: &str, report: &tooling::InspectReport, uri: &str) -> Option<Value> {
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
        let sig = fmt_function_signature(f.name.as_str(), &f.params, f.return_type.as_deref());
        return Some(format!("```cool\n{sig}\n```"));
    }
    if let Some(c) = report.classes.iter().find(|c| c.name == word) {
        if let Some(init) = c.methods.iter().find(|m| m.name == "__init__") {
            let non_self: Vec<&InspectParam> = init.params.iter().filter(|p| p.name != "self").collect();
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
    if let Some(assignment) = report
        .assignments
        .iter()
        .find(|assignment| assignment.names.iter().any(|name| name == word))
    {
        return Some(format!("```cool\n{}\n```", fmt_assignment_signature(word, assignment)));
    }
    None
}

// ── Completions ───────────────────────────────────────────────────────────────

fn compute_completions(content: &str, uri: &str, line_prefix: &str) -> Vec<Value> {
    let trimmed = line_prefix.trim_start();
    if trimmed.starts_with("import ") || trimmed == "import" || trimmed.starts_with("from ") {
        return COOL_MODULES.iter().map(|m| json!({"label": m, "kind": 9})).collect();
    }
    if let Some(receiver) = module_completion_receiver(line_prefix) {
        let report = tooling::inspect_source(content, uri);
        if let Some(items) = imported_module_completion_items(&report, &receiver) {
            return items;
        }
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
            "detail": fmt_function_signature(f.name.as_str(), &f.params, f.return_type.as_deref())
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
                items.push(json!({"label": name, "kind": 6, "detail": fmt_assignment_signature(name, a)}));
            }
        } else if matches!(a.kind, "var_decl" | "const") {
            for name in &a.names {
                items.push(json!({
                    "label": name,
                    "kind": if a.is_const { 21 } else { 6 },
                    "detail": fmt_assignment_signature(name, a)
                }));
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
            if let Some(type_name) = &p.type_name {
                s.push_str(": ");
                s.push_str(type_name);
            }
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
            if let Some(type_name) = &p.type_name {
                s.push_str(": ");
                s.push_str(type_name);
            }
            if p.has_default {
                s.push_str("=...");
            }
            s
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn fmt_function_signature(name: &str, params: &[InspectParam], return_type: Option<&str>) -> String {
    let mut sig = format!("def {}({})", name, fmt_params(params));
    if let Some(return_type) = return_type {
        sig.push_str(" -> ");
        sig.push_str(return_type);
    }
    sig
}

fn fmt_assignment_signature(name: &str, assignment: &InspectAssignment) -> String {
    let mut sig = String::new();
    if assignment.is_const {
        sig.push_str("const ");
    }
    sig.push_str(name);
    if let Some(type_name) = &assignment.type_name {
        sig.push_str(": ");
        sig.push_str(type_name);
    }
    sig
}

fn module_completion_receiver(line_prefix: &str) -> Option<String> {
    let trimmed = line_prefix.trim_end();
    if !trimmed.ends_with('.') {
        return None;
    }
    let without_dot = &trimmed[..trimmed.len().saturating_sub(1)];
    let start = without_dot
        .char_indices()
        .rev()
        .find(|(_, ch)| !(ch.is_alphanumeric() || *ch == '_'))
        .map(|(idx, ch)| idx + ch.len_utf8())
        .unwrap_or(0);
    let receiver = &without_dot[start..];
    if receiver.is_empty() {
        None
    } else {
        Some(receiver.to_string())
    }
}

fn imported_module_completion_items(report: &tooling::InspectReport, receiver: &str) -> Option<Vec<Value>> {
    let import = report.imports.iter().find(|import| {
        import.kind == tooling::ModuleImportKind::Module
            && import.specifier.rsplit('.').next().unwrap_or(import.specifier.as_str()) == receiver
            && import.resolved.is_some()
    })?;
    let resolved = import.resolved.as_ref()?;
    let ast = tooling::build_ast_dump(std::path::Path::new(resolved), false).ok()?;
    let exports: HashSet<String> = module_exports::exported_names(&ast.ast).into_iter().collect();
    let imported_report = tooling::build_inspect_report(std::path::Path::new(resolved)).ok()?;
    let mut items = Vec::new();
    for function in &imported_report.functions {
        if exports.contains(&function.name) {
            items.push(json!({
                "label": function.name,
                "kind": 3,
                "detail": fmt_function_signature(function.name.as_str(), &function.params, function.return_type.as_deref())
            }));
        }
    }
    for class in &imported_report.classes {
        if exports.contains(&class.name) {
            items.push(json!({"label": class.name, "kind": 7, "detail": format!("class {}", class.name)}));
        }
    }
    for structure in &imported_report.structs {
        if exports.contains(&structure.name) {
            items.push(json!({"label": structure.name, "kind": 22, "detail": format!("struct {}", structure.name)}));
        }
    }
    for assignment in &imported_report.assignments {
        for name in &assignment.names {
            if exports.contains(name) {
                items.push(json!({
                    "label": name,
                    "kind": if assignment.is_const { 21 } else { 6 },
                    "detail": fmt_assignment_signature(name, assignment)
                }));
            }
        }
    }
    Some(items)
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
