use crate::ast::{Program, Stmt, Visibility};

pub fn stmt_declared_name(stmt: &Stmt) -> Option<&str> {
    match stmt_unwrap_visibility(stmt).0 {
        Stmt::Assign { name, .. } => Some(name),
        Stmt::VarDecl { name, .. } => Some(name),
        Stmt::FnDef { name, .. } => Some(name),
        Stmt::ExternFn { name, .. } => Some(name),
        Stmt::Data { name, .. } => Some(name),
        Stmt::Class { name, .. } => Some(name),
        Stmt::Struct { name, .. } => Some(name),
        Stmt::Union { name, .. } => Some(name),
        Stmt::Enum { name, .. } => Some(name),
        Stmt::Trait { name, .. } => Some(name),
        _ => None,
    }
}

pub fn stmt_visibility(stmt: &Stmt) -> Option<Visibility> {
    stmt_unwrap_visibility(stmt).1
}

pub fn stmt_is_public_export(stmt: &Stmt) -> bool {
    let Some(name) = stmt_declared_name(stmt) else {
        return false;
    };
    match stmt_visibility(stmt) {
        Some(Visibility::Public) => true,
        Some(Visibility::Private) => false,
        None => !name.starts_with('_'),
    }
}

pub fn exported_names(program: &Program) -> Vec<String> {
    let mut exports = Vec::new();
    for stmt in program {
        if stmt_is_public_export(stmt) {
            if let Some(name) = stmt_declared_name(stmt) {
                exports.push(name.to_string());
            }
        }
    }
    exports
}

pub fn stmt_unwrap_visibility(stmt: &Stmt) -> (&Stmt, Option<Visibility>) {
    match stmt {
        Stmt::Visibility { visibility, stmt } => {
            let (inner, inner_visibility) = stmt_unwrap_visibility(stmt);
            (inner, inner_visibility.or(Some(*visibility)))
        }
        other => (other, None),
    }
}
