use rusqlite::types::{ToSql, ToSqlOutput, Value as SqlPrimitive, ValueRef};
use rusqlite::{params_from_iter, Connection};

#[derive(Debug, Clone, PartialEq)]
pub enum SqlData {
    Nil,
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
}

impl ToSql for SqlData {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        let value = match self {
            SqlData::Nil => SqlPrimitive::Null,
            SqlData::Int(n) => SqlPrimitive::Integer(*n),
            SqlData::Float(f) => SqlPrimitive::Real(*f),
            SqlData::Str(s) => SqlPrimitive::Text(s.clone()),
            SqlData::Bool(b) => SqlPrimitive::Integer(if *b { 1 } else { 0 }),
        };
        Ok(ToSqlOutput::Owned(value))
    }
}

pub fn execute(path: &str, sql: &str, params: &[SqlData]) -> Result<i64, String> {
    let conn = Connection::open(path).map_err(|e| format!("sqlite.open() error: {e}"))?;
    let changed = conn
        .execute(sql, params_from_iter(params.iter()))
        .map_err(|e| format!("sqlite.execute() error: {e}"))?;
    Ok(changed as i64)
}

pub fn query(path: &str, sql: &str, params: &[SqlData]) -> Result<Vec<Vec<(String, SqlData)>>, String> {
    let conn = Connection::open(path).map_err(|e| format!("sqlite.open() error: {e}"))?;
    let mut stmt = conn.prepare(sql).map_err(|e| format!("sqlite.query() error: {e}"))?;
    let column_names: Vec<String> = stmt.column_names().iter().map(|name| (*name).to_string()).collect();
    let mut rows = stmt
        .query(params_from_iter(params.iter()))
        .map_err(|e| format!("sqlite.query() error: {e}"))?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().map_err(|e| format!("sqlite.query() error: {e}"))? {
        let mut dict = Vec::with_capacity(column_names.len());
        for (idx, name) in column_names.iter().enumerate() {
            let value = sql_value_from_ref(row.get_ref(idx).map_err(|e| format!("sqlite.query() error: {e}"))?)?;
            dict.push((name.clone(), value));
        }
        out.push(dict);
    }
    Ok(out)
}

pub fn scalar(path: &str, sql: &str, params: &[SqlData]) -> Result<SqlData, String> {
    let rows = query(path, sql, params)?;
    Ok(rows
        .into_iter()
        .next()
        .and_then(|row| row.into_iter().next().map(|(_, value)| value))
        .unwrap_or(SqlData::Nil))
}

fn sql_value_from_ref(value: ValueRef<'_>) -> Result<SqlData, String> {
    match value {
        ValueRef::Null => Ok(SqlData::Nil),
        ValueRef::Integer(n) => Ok(SqlData::Int(n)),
        ValueRef::Real(f) => Ok(SqlData::Float(f)),
        ValueRef::Text(bytes) => Ok(SqlData::Str(String::from_utf8_lossy(bytes).to_string())),
        ValueRef::Blob(_) => Err("sqlite.query() does not support blob columns yet".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{execute, query, scalar, SqlData};
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static SQLITE_TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_db_path() -> std::path::PathBuf {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let seq = SQLITE_TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("cool_sqlite_runtime_{nonce}_{seq}.db"))
    }

    #[test]
    fn executes_queries_and_scalars() {
        let db = temp_db_path();
        let db_str = db.to_string_lossy().to_string();

        execute(
            &db_str,
            "create table items (id integer primary key, name text, score real, active integer)",
            &[],
        )
        .unwrap();
        execute(
            &db_str,
            "insert into items (name, score, active) values (?, ?, ?)",
            &[SqlData::Str("alpha".into()), SqlData::Float(1.5), SqlData::Bool(true)],
        )
        .unwrap();
        execute(
            &db_str,
            "insert into items (name, score, active) values (?, ?, ?)",
            &[SqlData::Str("beta".into()), SqlData::Float(2.25), SqlData::Bool(false)],
        )
        .unwrap();

        let rows = query(
            &db_str,
            "select name, score, active from items where score >= ? order by id",
            &[SqlData::Float(1.5)],
        )
        .unwrap();
        assert_eq!(
            rows,
            vec![
                vec![
                    ("name".into(), SqlData::Str("alpha".into())),
                    ("score".into(), SqlData::Float(1.5)),
                    ("active".into(), SqlData::Int(1)),
                ],
                vec![
                    ("name".into(), SqlData::Str("beta".into())),
                    ("score".into(), SqlData::Float(2.25)),
                    ("active".into(), SqlData::Int(0)),
                ],
            ]
        );

        let value = scalar(
            &db_str,
            "select name from items where active = ? order by id limit 1",
            &[SqlData::Int(1)],
        )
        .unwrap();
        assert_eq!(value, SqlData::Str("alpha".into()));

        let _ = fs::remove_file(&db);
    }
}
