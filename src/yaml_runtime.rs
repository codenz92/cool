#[derive(Debug, Clone, PartialEq)]
pub enum YamlData {
    Nil,
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    List(Vec<YamlData>),
    Dict(Vec<(String, YamlData)>),
}

#[derive(Debug, Clone)]
struct Line {
    indent: usize,
    text: String,
}

pub fn loads(src: &str) -> Result<YamlData, String> {
    let lines = preprocess_lines(src)?;
    if lines.is_empty() {
        return Ok(YamlData::Dict(Vec::new()));
    }
    let mut parser = Parser { lines: &lines, idx: 0 };
    let root_indent = parser.lines[0].indent;
    let value = parser.parse_block(root_indent)?;
    if parser.idx != parser.lines.len() {
        return Err("yaml.loads() could not consume the full document".to_string());
    }
    Ok(value)
}

pub fn dumps(data: &YamlData) -> Result<String, String> {
    let mut out = String::new();
    dump_block(&mut out, data, 0)?;
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

fn preprocess_lines(src: &str) -> Result<Vec<Line>, String> {
    let mut out = Vec::new();
    for raw in src.lines() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let indent = count_indent(line)?;
        let mut text = line[indent..].to_string();
        strip_comment_in_place(&mut text);
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        out.push(Line {
            indent,
            text: trimmed.to_string(),
        });
    }
    Ok(out)
}

fn count_indent(line: &str) -> Result<usize, String> {
    let mut indent = 0;
    for ch in line.chars() {
        match ch {
            ' ' => indent += 1,
            '\t' => return Err("yaml.loads() does not support tab indentation".to_string()),
            _ => break,
        }
    }
    Ok(indent)
}

fn strip_comment_in_place(text: &mut String) {
    let mut in_double = false;
    let mut in_single = false;
    let mut escaped = false;
    let mut cut = None;
    for (idx, ch) in text.char_indices() {
        if in_double {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_double = false;
            }
            continue;
        }
        if in_single {
            if ch == '\'' {
                in_single = false;
            }
            continue;
        }
        match ch {
            '"' => in_double = true,
            '\'' => in_single = true,
            '#' => {
                cut = Some(idx);
                break;
            }
            _ => {}
        }
    }
    if let Some(idx) = cut {
        text.truncate(idx);
    }
}

fn find_top_level_char(text: &str, target: char) -> Option<usize> {
    let mut in_double = false;
    let mut in_single = false;
    let mut escaped = false;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;
    for (idx, ch) in text.char_indices() {
        if in_double {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_double = false;
            }
            continue;
        }
        if in_single {
            if ch == '\'' {
                in_single = false;
            }
            continue;
        }
        match ch {
            '"' => in_double = true,
            '\'' => in_single = true,
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            _ if ch == target && bracket_depth == 0 && brace_depth == 0 => return Some(idx),
            _ => {}
        }
    }
    None
}

fn split_top_level(text: &str, sep: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut in_double = false;
    let mut in_single = false;
    let mut escaped = false;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;
    for (idx, ch) in text.char_indices() {
        if in_double {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_double = false;
            }
            continue;
        }
        if in_single {
            if ch == '\'' {
                in_single = false;
            }
            continue;
        }
        match ch {
            '"' => in_double = true,
            '\'' => in_single = true,
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            _ if ch == sep && bracket_depth == 0 && brace_depth == 0 => {
                parts.push(text[start..idx].trim().to_string());
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(text[start..].trim().to_string());
    parts
}

fn parse_quoted_string(text: &str) -> Result<(String, usize), String> {
    let quote = text
        .chars()
        .next()
        .ok_or_else(|| "yaml string cannot be empty".to_string())?;
    let mut out = String::new();
    let mut escaped = false;
    for (offset, ch) in text[quote.len_utf8()..].char_indices() {
        let idx = quote.len_utf8() + offset;
        if quote == '"' {
            if escaped {
                match ch {
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    other => out.push(other),
                }
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
        }
        if ch == quote {
            return Ok((out, idx + ch.len_utf8()));
        }
        out.push(ch);
    }
    Err("yaml string is unterminated".to_string())
}

fn parse_key(text: &str) -> Result<String, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("yaml mapping key cannot be empty".to_string());
    }
    if matches!(trimmed.chars().next(), Some('"') | Some('\'')) {
        let (value, consumed) = parse_quoted_string(trimmed)?;
        if trimmed[consumed..].trim().is_empty() {
            Ok(value)
        } else {
            Err("yaml mapping key has trailing characters".to_string())
        }
    } else {
        Ok(trimmed.to_string())
    }
}

fn parse_inline_value(text: &str) -> Result<YamlData, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("yaml value cannot be empty".to_string());
    }
    if matches!(trimmed.chars().next(), Some('"') | Some('\'')) {
        let (value, consumed) = parse_quoted_string(trimmed)?;
        if !trimmed[consumed..].trim().is_empty() {
            return Err("yaml string has trailing characters".to_string());
        }
        return Ok(YamlData::Str(value));
    }
    if trimmed.starts_with('[') {
        return parse_inline_list(trimmed);
    }
    if trimmed.starts_with('{') {
        return parse_inline_dict(trimmed);
    }
    if matches!(trimmed, "null" | "nil" | "~") {
        return Ok(YamlData::Nil);
    }
    if trimmed == "true" {
        return Ok(YamlData::Bool(true));
    }
    if trimmed == "false" {
        return Ok(YamlData::Bool(false));
    }
    if let Ok(n) = trimmed.parse::<i64>() {
        return Ok(YamlData::Int(n));
    }
    if let Ok(f) = trimmed.parse::<f64>() {
        return Ok(YamlData::Float(f));
    }
    Ok(YamlData::Str(trimmed.to_string()))
}

fn parse_inline_list(text: &str) -> Result<YamlData, String> {
    if !text.ends_with(']') {
        return Err("yaml inline list must end with ']'".to_string());
    }
    let inner = &text[1..text.len() - 1];
    if inner.trim().is_empty() {
        return Ok(YamlData::List(Vec::new()));
    }
    let mut out = Vec::new();
    for part in split_top_level(inner, ',') {
        if part.is_empty() {
            continue;
        }
        out.push(parse_inline_value(&part)?);
    }
    Ok(YamlData::List(out))
}

fn parse_inline_dict(text: &str) -> Result<YamlData, String> {
    if !text.ends_with('}') {
        return Err("yaml inline map must end with '}'".to_string());
    }
    let inner = &text[1..text.len() - 1];
    if inner.trim().is_empty() {
        return Ok(YamlData::Dict(Vec::new()));
    }
    let mut out = Vec::new();
    for part in split_top_level(inner, ',') {
        if part.is_empty() {
            continue;
        }
        let colon =
            find_top_level_char(&part, ':').ok_or_else(|| "yaml inline map entries must be key: value".to_string())?;
        let key = parse_key(&part[..colon])?;
        let value = parse_inline_value(&part[colon + 1..])?;
        out.push((key, value));
    }
    Ok(YamlData::Dict(out))
}

fn is_sequence_item(text: &str) -> bool {
    match text.strip_prefix('-') {
        Some(rest) => rest.is_empty() || rest.chars().next().is_some_and(char::is_whitespace),
        None => false,
    }
}

struct Parser<'a> {
    lines: &'a [Line],
    idx: usize,
}

impl<'a> Parser<'a> {
    fn parse_block(&mut self, indent: usize) -> Result<YamlData, String> {
        let line = self
            .lines
            .get(self.idx)
            .ok_or_else(|| "yaml document ended unexpectedly".to_string())?;
        if line.indent != indent {
            return Err(format!(
                "yaml indentation error: expected indent {}, got {}",
                indent, line.indent
            ));
        }
        if is_sequence_item(&line.text) {
            self.parse_list(indent)
        } else {
            self.parse_dict(indent)
        }
    }

    fn parse_list(&mut self, indent: usize) -> Result<YamlData, String> {
        let mut items = Vec::new();
        while let Some(line) = self.lines.get(self.idx) {
            if line.indent < indent {
                break;
            }
            if line.indent > indent {
                return Err(format!(
                    "yaml indentation error: unexpected nested indent {}",
                    line.indent
                ));
            }
            if !is_sequence_item(&line.text) {
                return Err("yaml cannot mix mapping entries into a sequence block".to_string());
            }
            let content = line.text[1..].trim();
            self.idx += 1;
            let value = if content.is_empty() {
                if let Some(next) = self.lines.get(self.idx) {
                    if next.indent > indent {
                        self.parse_block(next.indent)?
                    } else {
                        YamlData::Nil
                    }
                } else {
                    YamlData::Nil
                }
            } else {
                parse_inline_value(content)?
            };
            items.push(value);
        }
        Ok(YamlData::List(items))
    }

    fn parse_dict(&mut self, indent: usize) -> Result<YamlData, String> {
        let mut entries = Vec::new();
        while let Some(line) = self.lines.get(self.idx) {
            if line.indent < indent {
                break;
            }
            if line.indent > indent {
                return Err(format!(
                    "yaml indentation error: unexpected nested indent {}",
                    line.indent
                ));
            }
            if is_sequence_item(&line.text) {
                return Err("yaml cannot mix sequence items into a mapping block".to_string());
            }
            let colon = find_top_level_char(&line.text, ':')
                .ok_or_else(|| "yaml mapping entries must contain ':'".to_string())?;
            let key = parse_key(&line.text[..colon])?;
            let rest = line.text[colon + 1..].trim();
            self.idx += 1;
            let value = if rest.is_empty() {
                if let Some(next) = self.lines.get(self.idx) {
                    if next.indent > indent {
                        self.parse_block(next.indent)?
                    } else {
                        YamlData::Nil
                    }
                } else {
                    YamlData::Nil
                }
            } else {
                parse_inline_value(rest)?
            };
            entries.push((key, value));
        }
        Ok(YamlData::Dict(entries))
    }
}

fn is_inline_value(data: &YamlData) -> bool {
    matches!(
        data,
        YamlData::Nil | YamlData::Int(_) | YamlData::Float(_) | YamlData::Str(_) | YamlData::Bool(_)
    ) || matches!(data, YamlData::List(items) if items.is_empty())
        || matches!(data, YamlData::Dict(items) if items.is_empty())
}

fn string_needs_quotes(text: &str) -> bool {
    if text.is_empty() || text.trim() != text {
        return true;
    }
    if matches!(text, "true" | "false" | "null" | "nil" | "~") {
        return true;
    }
    if text.parse::<i64>().is_ok() || text.parse::<f64>().is_ok() {
        return true;
    }
    if matches!(text.chars().next(), Some('-' | '?' | '!' | '&' | '*' | '@' | '`')) {
        return true;
    }
    text.chars().any(|ch| {
        matches!(
            ch,
            ':' | '#' | '{' | '}' | '[' | ']' | ',' | '"' | '\'' | '\n' | '\r' | '\t'
        )
    })
}

fn dump_string(text: &str) -> String {
    if !string_needs_quotes(text) {
        return text.to_string();
    }
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn dump_inline(data: &YamlData) -> Result<String, String> {
    match data {
        YamlData::Nil => Ok("null".to_string()),
        YamlData::Int(n) => Ok(n.to_string()),
        YamlData::Float(f) => {
            if !f.is_finite() {
                return Err("yaml.dumps() does not support NaN or infinite floats".to_string());
            }
            Ok(f.to_string())
        }
        YamlData::Str(s) => Ok(dump_string(s)),
        YamlData::Bool(b) => Ok(if *b { "true" } else { "false" }.to_string()),
        YamlData::List(items) => {
            let rendered: Result<Vec<_>, _> = items.iter().map(dump_inline).collect();
            Ok(format!("[{}]", rendered?.join(", ")))
        }
        YamlData::Dict(items) => {
            let mut rendered = Vec::with_capacity(items.len());
            for (key, value) in items {
                rendered.push(format!("{}: {}", dump_string(key), dump_inline(value)?));
            }
            Ok(format!("{{{}}}", rendered.join(", ")))
        }
    }
}

fn push_indent(out: &mut String, indent: usize) {
    for _ in 0..indent {
        out.push(' ');
    }
}

fn dump_block(out: &mut String, data: &YamlData, indent: usize) -> Result<(), String> {
    match data {
        YamlData::List(items) if items.is_empty() => {
            push_indent(out, indent);
            out.push_str("[]\n");
        }
        YamlData::Dict(items) if items.is_empty() => {
            push_indent(out, indent);
            out.push_str("{}\n");
        }
        YamlData::List(items) => {
            for item in items {
                push_indent(out, indent);
                if is_inline_value(item) {
                    out.push_str("- ");
                    out.push_str(&dump_inline(item)?);
                    out.push('\n');
                } else {
                    out.push_str("-\n");
                    dump_block(out, item, indent + 2)?;
                }
            }
        }
        YamlData::Dict(items) => {
            for (key, value) in items {
                push_indent(out, indent);
                out.push_str(&dump_string(key));
                if is_inline_value(value) {
                    out.push_str(": ");
                    out.push_str(&dump_inline(value)?);
                    out.push('\n');
                } else {
                    out.push_str(":\n");
                    dump_block(out, value, indent + 2)?;
                }
            }
        }
        scalar => {
            push_indent(out, indent);
            out.push_str(&dump_inline(scalar)?);
            out.push('\n');
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{dumps, loads, YamlData};

    #[test]
    fn loads_nested_map_and_list() {
        let parsed = loads(
            r#"
name: cool
enabled: true
ports:
  - 8000
  - 8001
service:
  host: 127.0.0.1
  retries: 3
note: null
"#,
        )
        .unwrap();

        assert_eq!(
            parsed,
            YamlData::Dict(vec![
                ("name".into(), YamlData::Str("cool".into())),
                ("enabled".into(), YamlData::Bool(true)),
                (
                    "ports".into(),
                    YamlData::List(vec![YamlData::Int(8000), YamlData::Int(8001)])
                ),
                (
                    "service".into(),
                    YamlData::Dict(vec![
                        ("host".into(), YamlData::Str("127.0.0.1".into())),
                        ("retries".into(), YamlData::Int(3)),
                    ])
                ),
                ("note".into(), YamlData::Nil),
            ])
        );
    }

    #[test]
    fn loads_inline_collections() {
        let parsed = loads(
            r#"items: [1, "two", false]
meta: {name: cool, count: 2}"#,
        )
        .unwrap();

        assert_eq!(
            parsed,
            YamlData::Dict(vec![
                (
                    "items".into(),
                    YamlData::List(vec![
                        YamlData::Int(1),
                        YamlData::Str("two".into()),
                        YamlData::Bool(false),
                    ])
                ),
                (
                    "meta".into(),
                    YamlData::Dict(vec![
                        ("name".into(), YamlData::Str("cool".into())),
                        ("count".into(), YamlData::Int(2)),
                    ])
                ),
            ])
        );
    }

    #[test]
    fn dumps_nested_values() {
        let text = dumps(&YamlData::Dict(vec![
            ("name".into(), YamlData::Str("cool".into())),
            (
                "ports".into(),
                YamlData::List(vec![YamlData::Int(8000), YamlData::Int(8001)]),
            ),
            (
                "service".into(),
                YamlData::Dict(vec![
                    ("host".into(), YamlData::Str("127.0.0.1".into())),
                    ("enabled".into(), YamlData::Bool(true)),
                ]),
            ),
            ("note".into(), YamlData::Nil),
        ]))
        .unwrap();

        assert!(text.contains("name: cool"));
        assert!(text.contains("ports:\n  - 8000\n  - 8001"));
        assert!(text.contains("service:\n  host: 127.0.0.1\n  enabled: true"));
        assert!(text.contains("note: null"));
    }
}
