#[derive(Debug, Clone, PartialEq)]
pub enum TomlData {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    List(Vec<TomlData>),
    Dict(Vec<(String, TomlData)>),
}

pub fn loads(src: &str) -> Result<TomlData, String> {
    let value: toml::Value = src
        .parse()
        .map_err(|e: toml::de::Error| format!("toml.loads() error: {}", e.message()))?;
    value_to_data(&value)
}

pub fn dumps(data: &TomlData) -> Result<String, String> {
    let value = data_to_value(data)?;
    match value {
        toml::Value::Table(_) => toml::to_string(&value).map_err(|e| format!("toml.dumps() error: {e}")),
        _ => Err("toml.dumps() requires a dict/table at the root".to_string()),
    }
}

fn value_to_data(value: &toml::Value) -> Result<TomlData, String> {
    match value {
        toml::Value::String(s) => Ok(TomlData::Str(s.clone())),
        toml::Value::Integer(i) => Ok(TomlData::Int(*i)),
        toml::Value::Float(f) => Ok(TomlData::Float(*f)),
        toml::Value::Boolean(b) => Ok(TomlData::Bool(*b)),
        toml::Value::Datetime(dt) => Ok(TomlData::Str(dt.to_string())),
        toml::Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(value_to_data(item)?);
            }
            Ok(TomlData::List(out))
        }
        toml::Value::Table(table) => {
            let mut out = Vec::with_capacity(table.len());
            for (key, value) in table {
                out.push((key.clone(), value_to_data(value)?));
            }
            Ok(TomlData::Dict(out))
        }
    }
}

fn data_to_value(data: &TomlData) -> Result<toml::Value, String> {
    match data {
        TomlData::Int(n) => Ok(toml::Value::Integer(*n)),
        TomlData::Float(f) => {
            if !f.is_finite() {
                return Err("toml.dumps() does not support NaN or infinite floats".to_string());
            }
            Ok(toml::Value::Float(*f))
        }
        TomlData::Str(s) => Ok(toml::Value::String(s.clone())),
        TomlData::Bool(b) => Ok(toml::Value::Boolean(*b)),
        TomlData::List(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(data_to_value(item)?);
            }
            Ok(toml::Value::Array(out))
        }
        TomlData::Dict(items) => {
            let mut out = toml::map::Map::new();
            for (key, value) in items {
                out.insert(key.clone(), data_to_value(value)?);
            }
            Ok(toml::Value::Table(out))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{dumps, loads, TomlData};

    #[test]
    fn loads_nested_tables_and_arrays() {
        let parsed = loads(
            r#"
name = "cool"
ports = [8000, 8001]

[server]
host = "127.0.0.1"
debug = true
"#,
        )
        .unwrap();

        assert_eq!(
            parsed,
            TomlData::Dict(vec![
                ("name".into(), TomlData::Str("cool".into())),
                (
                    "ports".into(),
                    TomlData::List(vec![TomlData::Int(8000), TomlData::Int(8001)])
                ),
                (
                    "server".into(),
                    TomlData::Dict(vec![
                        ("debug".into(), TomlData::Bool(true)),
                        ("host".into(), TomlData::Str("127.0.0.1".into())),
                    ])
                ),
            ])
        );
    }

    #[test]
    fn dumps_root_tables() {
        let text = dumps(&TomlData::Dict(vec![
            ("name".into(), TomlData::Str("cool".into())),
            (
                "server".into(),
                TomlData::Dict(vec![
                    ("host".into(), TomlData::Str("127.0.0.1".into())),
                    ("debug".into(), TomlData::Bool(true)),
                ]),
            ),
        ]))
        .unwrap();

        assert!(text.contains("name = \"cool\""));
        assert!(text.contains("[server]"));
        assert!(text.contains("host = \"127.0.0.1\""));
        assert!(text.contains("debug = true"));
    }

    #[test]
    fn dumps_rejects_non_table_roots() {
        let err = dumps(&TomlData::Str("cool".into())).unwrap_err();
        assert!(err.contains("dict/table"));
    }
}
