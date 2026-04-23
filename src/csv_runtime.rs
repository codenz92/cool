pub fn parse_rows(input: &str) -> Result<Vec<Vec<String>>, String> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut chars = input.chars().peekable();
    let mut in_quotes = false;
    let mut just_closed_quote = false;
    let mut just_finished_row = false;
    let mut saw_any = false;

    while let Some(ch) = chars.next() {
        saw_any = true;
        if in_quotes {
            match ch {
                '"' => {
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        field.push('"');
                    } else {
                        in_quotes = false;
                        just_closed_quote = true;
                    }
                }
                other => field.push(other),
            }
            continue;
        }

        match ch {
            '"' => {
                if !field.is_empty() || just_closed_quote {
                    return Err("csv parse error: quote must start at beginning of field".into());
                }
                in_quotes = true;
                just_finished_row = false;
            }
            ',' => {
                row.push(std::mem::take(&mut field));
                just_closed_quote = false;
                just_finished_row = false;
            }
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
                just_closed_quote = false;
                just_finished_row = true;
            }
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
                just_closed_quote = false;
                just_finished_row = true;
            }
            other => {
                if just_closed_quote {
                    return Err("csv parse error: unexpected character after closing quote".into());
                }
                field.push(other);
                just_finished_row = false;
            }
        }
    }

    if in_quotes {
        return Err("csv parse error: unterminated quoted field".into());
    }

    if saw_any && (!just_finished_row || !field.is_empty() || !row.is_empty()) {
        row.push(field);
        rows.push(row);
    }

    Ok(rows)
}

pub fn parse_dicts(input: &str) -> Result<Vec<Vec<(String, String)>>, String> {
    let rows = parse_rows(input)?;
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let headers = rows[0].clone();
    let mut out = Vec::with_capacity(rows.len().saturating_sub(1));
    for row in rows.into_iter().skip(1) {
        let mut items = Vec::with_capacity(headers.len());
        for (idx, header) in headers.iter().enumerate() {
            items.push((header.clone(), row.get(idx).cloned().unwrap_or_default()));
        }
        out.push(items);
    }
    Ok(out)
}

pub fn write_rows(rows: &[Vec<String>]) -> String {
    let mut out = String::new();
    for (row_idx, row) in rows.iter().enumerate() {
        if row_idx > 0 {
            out.push('\n');
        }
        for (col_idx, field) in row.iter().enumerate() {
            if col_idx > 0 {
                out.push(',');
            }
            out.push_str(&escape_field(field));
        }
    }
    out
}

fn escape_field(field: &str) -> String {
    let needs_quotes = field.contains([',', '"', '\n', '\r'])
        || field.chars().next().map(char::is_whitespace).unwrap_or(false)
        || field.chars().next_back().map(char::is_whitespace).unwrap_or(false);
    if !needs_quotes {
        return field.to_string();
    }
    let escaped = field.replace('"', "\"\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::{parse_dicts, parse_rows, write_rows};

    #[test]
    fn parse_rows_handles_quotes_and_escapes() {
        let rows = parse_rows("name,quote\nAlice,\"She said \"\"hi\"\"\"").unwrap();
        assert_eq!(
            rows,
            vec![
                vec!["name".to_string(), "quote".to_string()],
                vec!["Alice".to_string(), "She said \"hi\"".to_string()],
            ]
        );
    }

    #[test]
    fn parse_dicts_uses_header_row() {
        let rows = parse_dicts("name,city\nAlice,Paris\nBob,London").unwrap();
        assert_eq!(
            rows,
            vec![
                vec![
                    ("name".to_string(), "Alice".to_string()),
                    ("city".to_string(), "Paris".to_string()),
                ],
                vec![
                    ("name".to_string(), "Bob".to_string()),
                    ("city".to_string(), "London".to_string()),
                ],
            ]
        );
    }

    #[test]
    fn write_rows_quotes_special_fields() {
        let rendered = write_rows(&[
            vec!["name".into(), "quote".into()],
            vec!["Alice".into(), "She said \"hi\"".into()],
            vec!["Bob".into(), "New York, NY".into()],
        ]);
        assert_eq!(
            rendered,
            "name,quote\nAlice,\"She said \"\"hi\"\"\"\nBob,\"New York, NY\""
        );
    }
}
