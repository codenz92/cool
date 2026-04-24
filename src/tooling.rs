use crate::ast::{ExceptHandler, Program, Stmt};
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::project::ModuleResolver;
use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

const BUILTIN_MODULES: &[&str] = &[
    "argparse",
    "collections",
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

#[derive(Serialize)]
pub struct AstDump {
    pub path: String,
    pub ast: Program,
}

#[derive(Serialize)]
pub struct ModuleGraph {
    pub entry: String,
    pub modules: Vec<ModuleGraphModule>,
}

#[derive(Serialize)]
pub struct ModuleGraphModule {
    pub path: String,
    pub imports: Vec<ModuleGraphImport>,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModuleImportKind {
    File,
    Module,
    Builtin,
}

#[derive(Serialize)]
pub struct ModuleGraphImport {
    pub line: Option<usize>,
    pub kind: ModuleImportKind,
    pub specifier: String,
    pub resolved: Option<String>,
}

struct ResolvedImport {
    line: Option<usize>,
    kind: ModuleImportKind,
    specifier: String,
    resolved: Option<PathBuf>,
}

impl ResolvedImport {
    fn export(&self) -> ModuleGraphImport {
        ModuleGraphImport {
            line: self.line,
            kind: self.kind,
            specifier: self.specifier.clone(),
            resolved: self.resolved.as_ref().map(|path| path_string(path)),
        }
    }
}

pub fn build_ast_dump(path: &Path, include_line_markers: bool) -> Result<AstDump, String> {
    let canonical = canonical_existing(path)?;
    let program = parse_file(&canonical)?;
    let ast = if include_line_markers {
        program
    } else {
        strip_line_markers(&program)
    };
    Ok(AstDump {
        path: path_string(&canonical),
        ast,
    })
}

pub fn build_module_graph(path: &Path) -> Result<ModuleGraph, String> {
    let canonical = canonical_existing(path)?;
    let source_dir = canonical
        .parent()
        .ok_or_else(|| format!("modulegraph: '{}' has no parent directory", canonical.display()))?;
    let resolver = ModuleResolver::discover_for_script(source_dir)?;
    let mut visited = HashSet::new();
    let mut modules = Vec::new();
    walk_module(&canonical, &resolver, &mut visited, &mut modules)?;
    Ok(ModuleGraph {
        entry: path_string(&canonical),
        modules,
    })
}

fn walk_module(
    path: &Path,
    resolver: &ModuleResolver,
    visited: &mut HashSet<PathBuf>,
    modules: &mut Vec<ModuleGraphModule>,
) -> Result<(), String> {
    let canonical = canonical_existing(path)?;
    if !visited.insert(canonical.clone()) {
        return Ok(());
    }

    let program = parse_file(&canonical)?;
    let current_source_dir = canonical
        .parent()
        .ok_or_else(|| format!("modulegraph: '{}' has no parent directory", canonical.display()))?;
    let imports = collect_imports(&program, current_source_dir, resolver)?;

    let mut child_paths: Vec<PathBuf> = imports.iter().filter_map(|import| import.resolved.clone()).collect();
    child_paths.sort();
    child_paths.dedup();

    modules.push(ModuleGraphModule {
        path: path_string(&canonical),
        imports: imports.iter().map(ResolvedImport::export).collect(),
    });

    for child in child_paths {
        walk_module(&child, resolver, visited, modules)?;
    }

    Ok(())
}

fn collect_imports(
    stmts: &[Stmt],
    current_source_dir: &Path,
    resolver: &ModuleResolver,
) -> Result<Vec<ResolvedImport>, String> {
    let mut imports = Vec::new();
    collect_imports_from_block(stmts, current_source_dir, resolver, &mut imports)?;
    Ok(imports)
}

fn collect_imports_from_block(
    stmts: &[Stmt],
    current_source_dir: &Path,
    resolver: &ModuleResolver,
    imports: &mut Vec<ResolvedImport>,
) -> Result<(), String> {
    let mut current_line = None;
    for stmt in stmts {
        match stmt {
            Stmt::SetLine(line) => current_line = Some(*line),
            Stmt::Import(specifier) => {
                let resolved = resolve_file_import(current_source_dir, specifier)?;
                imports.push(ResolvedImport {
                    line: current_line,
                    kind: ModuleImportKind::File,
                    specifier: specifier.clone(),
                    resolved: Some(resolved),
                });
            }
            Stmt::ImportModule(name) => {
                let resolution = if is_builtin_module(name) {
                    ResolvedImport {
                        line: current_line,
                        kind: ModuleImportKind::Builtin,
                        specifier: name.clone(),
                        resolved: None,
                    }
                } else if let Some(resolved) = resolver.resolve_module(current_source_dir, name) {
                    ResolvedImport {
                        line: current_line,
                        kind: ModuleImportKind::Module,
                        specifier: name.clone(),
                        resolved: Some(resolved),
                    }
                } else {
                    return Err(format!(
                        "modulegraph: unresolved module import '{}' from '{}'",
                        name,
                        current_source_dir.display()
                    ));
                };
                imports.push(resolution);
            }
            Stmt::If {
                then_body,
                elif_clauses,
                else_body,
                ..
            } => {
                collect_imports_from_block(then_body, current_source_dir, resolver, imports)?;
                for (_, body) in elif_clauses {
                    collect_imports_from_block(body, current_source_dir, resolver, imports)?;
                }
                if let Some(body) = else_body {
                    collect_imports_from_block(body, current_source_dir, resolver, imports)?;
                }
            }
            Stmt::While { body, .. }
            | Stmt::For { body, .. }
            | Stmt::FnDef { body, .. }
            | Stmt::Class { body, .. }
            | Stmt::With { body, .. } => {
                collect_imports_from_block(body, current_source_dir, resolver, imports)?;
            }
            Stmt::Try {
                body,
                handlers,
                else_body,
                finally_body,
            } => {
                collect_imports_from_block(body, current_source_dir, resolver, imports)?;
                for handler in handlers {
                    collect_imports_from_block(&handler.body, current_source_dir, resolver, imports)?;
                }
                if let Some(body) = else_body {
                    collect_imports_from_block(body, current_source_dir, resolver, imports)?;
                }
                if let Some(body) = finally_body {
                    collect_imports_from_block(body, current_source_dir, resolver, imports)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn resolve_file_import(current_source_dir: &Path, specifier: &str) -> Result<PathBuf, String> {
    let full_path = if Path::new(specifier).is_absolute() {
        PathBuf::from(specifier)
    } else {
        current_source_dir.join(specifier)
    };
    if full_path.exists() {
        Ok(full_path.canonicalize().unwrap_or(full_path))
    } else {
        Err(format!(
            "modulegraph: unresolved file import '{}' from '{}'",
            specifier,
            current_source_dir.display()
        ))
    }
}

fn parse_file(path: &Path) -> Result<Program, String> {
    let source = fs::read_to_string(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    let mut lexer = Lexer::new(&source);
    let tokens = lexer.tokenize().map_err(|e| format!("{}: {}", path.display(), e))?;
    let mut parser = Parser::new(tokens);
    parser.parse_program().map_err(|e| format!("{}: {}", path.display(), e))
}

fn canonical_existing(path: &Path) -> Result<PathBuf, String> {
    if !path.exists() {
        return Err(format!("file not found: {}", path.display()));
    }
    Ok(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()))
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn is_builtin_module(name: &str) -> bool {
    BUILTIN_MODULES.contains(&name)
}

fn strip_line_markers(program: &Program) -> Program {
    program.iter().filter_map(strip_stmt).collect()
}

fn strip_stmt(stmt: &Stmt) -> Option<Stmt> {
    match stmt {
        Stmt::SetLine(_) => None,
        Stmt::If {
            condition,
            then_body,
            elif_clauses,
            else_body,
        } => Some(Stmt::If {
            condition: condition.clone(),
            then_body: strip_line_markers(then_body),
            elif_clauses: elif_clauses
                .iter()
                .map(|(expr, body)| (expr.clone(), strip_line_markers(body)))
                .collect(),
            else_body: else_body.as_ref().map(strip_line_markers),
        }),
        Stmt::While { condition, body } => Some(Stmt::While {
            condition: condition.clone(),
            body: strip_line_markers(body),
        }),
        Stmt::For { var, iter, body } => Some(Stmt::For {
            var: var.clone(),
            iter: iter.clone(),
            body: strip_line_markers(body),
        }),
        Stmt::FnDef { name, params, body } => Some(Stmt::FnDef {
            name: name.clone(),
            params: params.clone(),
            body: strip_line_markers(body),
        }),
        Stmt::Class { name, parent, body } => Some(Stmt::Class {
            name: name.clone(),
            parent: parent.clone(),
            body: strip_line_markers(body),
        }),
        Stmt::Try {
            body,
            handlers,
            else_body,
            finally_body,
        } => Some(Stmt::Try {
            body: strip_line_markers(body),
            handlers: handlers
                .iter()
                .map(|handler| ExceptHandler {
                    exc_type: handler.exc_type.clone(),
                    as_name: handler.as_name.clone(),
                    body: strip_line_markers(&handler.body),
                })
                .collect(),
            else_body: else_body.as_ref().map(strip_line_markers),
            finally_body: finally_body.as_ref().map(strip_line_markers),
        }),
        Stmt::With { expr, as_name, body } => Some(Stmt::With {
            expr: expr.clone(),
            as_name: as_name.clone(),
            body: strip_line_markers(body),
        }),
        _ => Some(stmt.clone()),
    }
}
