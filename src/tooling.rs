use crate::ast::{ExceptHandler, Program, Stmt};
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::project::ModuleResolver;
use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

pub fn diagnose_source(source: &str, path: &str) -> Vec<ToolingDiagnostic> {
    let mut diags = Vec::new();
    let mut lexer = Lexer::new(source);
    let tokens = match lexer.tokenize() {
        Err(msg) => {
            diags.push(ToolingDiagnostic {
                severity: DiagnosticSeverity::Error,
                code: "lex_error",
                path: path.to_string(),
                line: extract_error_line(&msg),
                message: msg,
            });
            return diags;
        }
        Ok(t) => t,
    };
    let mut parser = Parser::new(tokens);
    let program = match parser.parse_program() {
        Err(msg) => {
            diags.push(ToolingDiagnostic {
                severity: DiagnosticSeverity::Error,
                code: "parse_error",
                path: path.to_string(),
                line: extract_error_line(&msg),
                message: msg,
            });
            return diags;
        }
        Ok(p) => p,
    };
    let report = inspect_program(path.to_string(), &program, vec![]);
    diags.extend(check_report_warnings(&report));
    diags
}

pub fn inspect_source(source: &str, path: &str) -> InspectReport {
    let source_dir = Path::new(path).parent().unwrap_or(Path::new("."));
    let resolver = ModuleResolver::local_only(source_dir.to_path_buf());
    let program = match parse_source_str(source) {
        Ok(p) => p,
        Err(_) => {
            return InspectReport {
                path: path.to_string(),
                imports: vec![],
                functions: vec![],
                classes: vec![],
                structs: vec![],
                assignments: vec![],
            }
        }
    };
    let imports = collect_imports(&program, source_dir, &resolver, "lsp")
        .unwrap_or_default()
        .iter()
        .map(ResolvedImport::export)
        .collect();
    inspect_program(path.to_string(), &program, imports)
}

fn parse_source_str(source: &str) -> Result<Program, String> {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize()?;
    let mut parser = Parser::new(tokens);
    parser.parse_program()
}

fn extract_error_line(msg: &str) -> Option<usize> {
    let prefix = "line ";
    let start = msg.find(prefix)?;
    let rest = &msg[start + prefix.len()..];
    let end = rest.find(':')?;
    rest[..end].trim().parse().ok()
}

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

#[derive(Serialize)]
pub struct AstDump {
    pub path: String,
    pub ast: Program,
}

#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Clone, Serialize)]
pub struct ToolingDiagnostic {
    pub severity: DiagnosticSeverity,
    pub code: &'static str,
    pub path: String,
    pub line: Option<usize>,
    pub message: String,
}

#[derive(Serialize)]
pub struct CheckReport {
    pub entry: String,
    pub modules_checked: usize,
    pub diagnostics: Vec<ToolingDiagnostic>,
}

#[derive(Serialize)]
pub struct SymbolIndexReport {
    pub entry: String,
    pub modules_indexed: usize,
    pub symbols: Vec<SymbolLocation>,
    pub diagnostics: Vec<ToolingDiagnostic>,
}

#[derive(Clone, Serialize)]
pub struct InspectReport {
    pub path: String,
    pub imports: Vec<ModuleGraphImport>,
    pub functions: Vec<InspectFunction>,
    pub classes: Vec<InspectClass>,
    pub structs: Vec<InspectStruct>,
    pub assignments: Vec<InspectAssignment>,
}

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct InspectFunction {
    pub line: Option<usize>,
    pub name: String,
    pub params: Vec<InspectParam>,
}

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct InspectClass {
    pub line: Option<usize>,
    pub name: String,
    pub parent: Option<String>,
    pub methods: Vec<InspectFunction>,
    pub class_assignments: Vec<InspectAssignment>,
}

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct InspectStruct {
    pub line: Option<usize>,
    pub name: String,
    pub is_packed: bool,
    pub fields: Vec<InspectStructField>,
}

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct InspectStructField {
    pub name: String,
    pub type_name: String,
}

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct InspectAssignment {
    pub line: Option<usize>,
    pub kind: &'static str,
    pub names: Vec<String>,
}

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct InspectParam {
    pub name: String,
    pub has_default: bool,
    pub is_vararg: bool,
    pub is_kwarg: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Import,
    Function,
    Class,
    Method,
    Struct,
    Assignment,
    ClassAssignment,
}

#[derive(Clone, Serialize)]
pub struct SymbolLocation {
    pub path: String,
    pub line: Option<usize>,
    pub name: String,
    pub kind: SymbolKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub import_kind: Option<ModuleImportKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub import_specifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub import_resolved: Option<String>,
}

#[derive(Serialize)]
pub struct InspectDiffReport {
    pub before: String,
    pub after: String,
    pub imports: DiffSetWithChanges<ModuleGraphImport>,
    pub functions: DiffSetWithChanges<InspectFunction>,
    pub classes: DiffSetWithChanges<InspectClass>,
    pub structs: DiffSetWithChanges<InspectStruct>,
    pub assignments: DiffSet<InspectAssignment>,
}

#[derive(Serialize)]
pub struct DiffSet<T> {
    pub added: Vec<T>,
    pub removed: Vec<T>,
}

#[derive(Serialize)]
pub struct DiffSetWithChanges<T> {
    pub added: Vec<T>,
    pub removed: Vec<T>,
    pub changed: Vec<DiffChange<T>>,
}

#[derive(Serialize)]
pub struct DiffChange<T> {
    pub before: T,
    pub after: T,
}

#[derive(Serialize)]
pub struct ModuleGraph {
    pub entry: String,
    pub modules: Vec<ModuleGraphModule>,
    pub diagnostics: Vec<ToolingDiagnostic>,
}

#[derive(Serialize)]
pub struct ModuleGraphModule {
    pub path: String,
    pub imports: Vec<ModuleGraphImport>,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModuleImportKind {
    File,
    Module,
    Builtin,
}

#[derive(Clone, PartialEq, Eq, Serialize)]
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

struct ModuleGraphState {
    visited: HashSet<PathBuf>,
    active: Vec<PathBuf>,
    modules: Vec<ModuleGraphModule>,
    diagnostics: Vec<ToolingDiagnostic>,
    cycle_keys: HashSet<String>,
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

impl ModuleImportKind {
    fn as_key(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Module => "module",
            Self::Builtin => "builtin",
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

pub fn build_inspect_report(path: &Path) -> Result<InspectReport, String> {
    let canonical = canonical_existing(path)?;
    let source_dir = canonical
        .parent()
        .ok_or_else(|| format!("inspect: '{}' has no parent directory", canonical.display()))?;
    let resolver = ModuleResolver::discover_for_script(source_dir)?;
    let program = parse_file(&canonical)?;
    let imports = collect_imports(&program, source_dir, &resolver, "inspect")?
        .iter()
        .map(ResolvedImport::export)
        .collect();
    Ok(inspect_program(path_string(&canonical), &program, imports))
}

pub fn build_inspect_diff(before_path: &Path, after_path: &Path) -> Result<InspectDiffReport, String> {
    let before = build_inspect_report(before_path)?;
    let after = build_inspect_report(after_path)?;

    Ok(InspectDiffReport {
        before: before.path.clone(),
        after: after.path.clone(),
        imports: diff_keyed(&before.imports, &after.imports, |item| {
            format!("{}:{}", item.kind.as_key(), item.specifier)
        }),
        functions: diff_keyed(&before.functions, &after.functions, |item| item.name.clone()),
        classes: diff_keyed(&before.classes, &after.classes, |item| item.name.clone()),
        structs: diff_keyed(&before.structs, &after.structs, |item| item.name.clone()),
        assignments: diff_by_identity(&before.assignments, &after.assignments, assignment_identity_key),
    })
}

pub fn build_symbol_index(path: &Path) -> Result<SymbolIndexReport, String> {
    let graph = build_module_graph(path)?;
    let mut symbols = Vec::new();
    for module in &graph.modules {
        let program = parse_file(Path::new(&module.path))?;
        let report = inspect_program(module.path.clone(), &program, module.imports.clone());
        symbols.extend(symbols_from_report(&report));
    }
    symbols.sort_by_key(symbol_sort_key);
    Ok(SymbolIndexReport {
        entry: graph.entry.clone(),
        modules_indexed: graph.modules.len(),
        symbols,
        diagnostics: graph.diagnostics.clone(),
    })
}

pub fn build_check_report(path: &Path) -> Result<CheckReport, String> {
    let graph = build_module_graph(path)?;
    let mut diagnostics = graph.diagnostics.clone();
    for module in &graph.modules {
        let program = parse_file(Path::new(&module.path))?;
        let report = inspect_program(module.path.clone(), &program, module.imports.clone());
        diagnostics.extend(check_report_warnings(&report));
    }
    diagnostics.sort_by_key(diagnostic_sort_key);
    Ok(CheckReport {
        entry: graph.entry.clone(),
        modules_checked: graph.modules.len(),
        diagnostics,
    })
}

pub fn build_module_graph(path: &Path) -> Result<ModuleGraph, String> {
    let canonical = canonical_existing(path)?;
    let source_dir = canonical
        .parent()
        .ok_or_else(|| format!("modulegraph: '{}' has no parent directory", canonical.display()))?;
    let resolver = ModuleResolver::discover_for_script(source_dir)?;
    let mut state = ModuleGraphState {
        visited: HashSet::new(),
        active: Vec::new(),
        modules: Vec::new(),
        diagnostics: Vec::new(),
        cycle_keys: HashSet::new(),
    };
    walk_module(&canonical, &resolver, &mut state)?;
    Ok(ModuleGraph {
        entry: path_string(&canonical),
        modules: state.modules,
        diagnostics: state.diagnostics,
    })
}

fn walk_module(path: &Path, resolver: &ModuleResolver, state: &mut ModuleGraphState) -> Result<(), String> {
    let canonical = canonical_existing(path)?;
    if state.visited.contains(&canonical) {
        return Ok(());
    }
    state.visited.insert(canonical.clone());
    state.active.push(canonical.clone());

    let program = match parse_file(&canonical) {
        Ok(program) => program,
        Err(message) => {
            state.diagnostics.push(ToolingDiagnostic {
                severity: DiagnosticSeverity::Error,
                code: "parse_error",
                path: path_string(&canonical),
                line: None,
                message,
            });
            state.active.pop();
            return Ok(());
        }
    };
    let current_source_dir = canonical
        .parent()
        .ok_or_else(|| format!("modulegraph: '{}' has no parent directory", canonical.display()))?;
    let imports = collect_graph_imports(&program, current_source_dir, resolver, &canonical, state, "modulegraph");

    let mut child_paths: Vec<PathBuf> = imports.iter().filter_map(|import| import.resolved.clone()).collect();
    child_paths.sort();
    child_paths.dedup();

    state.modules.push(ModuleGraphModule {
        path: path_string(&canonical),
        imports: imports.iter().map(ResolvedImport::export).collect(),
    });

    for child in child_paths {
        if let Some(cycle_start) = state.active.iter().position(|path| path == &child) {
            let cycle_paths: Vec<String> = state.active[cycle_start..]
                .iter()
                .chain(std::iter::once(&child))
                .map(|path| path_string(path))
                .collect();
            let cycle_key = cycle_paths.join(" -> ");
            if state.cycle_keys.insert(cycle_key.clone()) {
                state.diagnostics.push(ToolingDiagnostic {
                    severity: DiagnosticSeverity::Error,
                    code: "import_cycle",
                    path: path_string(&canonical),
                    line: import_line_for_child(&imports, &child),
                    message: format!("import cycle detected: {}", cycle_key),
                });
            }
            continue;
        }
        walk_module(&child, resolver, state)?;
    }

    state.active.pop();
    Ok(())
}

fn collect_imports(
    stmts: &[Stmt],
    current_source_dir: &Path,
    resolver: &ModuleResolver,
    context: &str,
) -> Result<Vec<ResolvedImport>, String> {
    let mut imports = Vec::new();
    collect_imports_from_block(stmts, current_source_dir, resolver, &mut imports, context)?;
    Ok(imports)
}

fn collect_graph_imports(
    stmts: &[Stmt],
    current_source_dir: &Path,
    resolver: &ModuleResolver,
    current_module_path: &Path,
    state: &mut ModuleGraphState,
    context: &str,
) -> Vec<ResolvedImport> {
    let mut imports = Vec::new();
    collect_graph_imports_from_block(
        stmts,
        current_source_dir,
        resolver,
        current_module_path,
        state,
        &mut imports,
        context,
    );
    imports
}

fn collect_imports_from_block(
    stmts: &[Stmt],
    current_source_dir: &Path,
    resolver: &ModuleResolver,
    imports: &mut Vec<ResolvedImport>,
    context: &str,
) -> Result<(), String> {
    let mut current_line = None;
    for stmt in stmts {
        match stmt {
            Stmt::SetLine(line) => current_line = Some(*line),
            Stmt::Import(specifier) => {
                let resolved = resolve_file_import(current_source_dir, specifier, context)?;
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
                        "{context}: unresolved module import '{}' from '{}'",
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
                collect_imports_from_block(then_body, current_source_dir, resolver, imports, context)?;
                for (_, body) in elif_clauses {
                    collect_imports_from_block(body, current_source_dir, resolver, imports, context)?;
                }
                if let Some(body) = else_body {
                    collect_imports_from_block(body, current_source_dir, resolver, imports, context)?;
                }
            }
            Stmt::While { body, .. }
            | Stmt::For { body, .. }
            | Stmt::FnDef { body, .. }
            | Stmt::Class { body, .. }
            | Stmt::With { body, .. } => {
                collect_imports_from_block(body, current_source_dir, resolver, imports, context)?;
            }
            Stmt::Try {
                body,
                handlers,
                else_body,
                finally_body,
            } => {
                collect_imports_from_block(body, current_source_dir, resolver, imports, context)?;
                for handler in handlers {
                    collect_imports_from_block(&handler.body, current_source_dir, resolver, imports, context)?;
                }
                if let Some(body) = else_body {
                    collect_imports_from_block(body, current_source_dir, resolver, imports, context)?;
                }
                if let Some(body) = finally_body {
                    collect_imports_from_block(body, current_source_dir, resolver, imports, context)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn collect_graph_imports_from_block(
    stmts: &[Stmt],
    current_source_dir: &Path,
    resolver: &ModuleResolver,
    current_module_path: &Path,
    state: &mut ModuleGraphState,
    imports: &mut Vec<ResolvedImport>,
    context: &str,
) {
    let mut current_line = None;
    for stmt in stmts {
        match stmt {
            Stmt::SetLine(line) => current_line = Some(*line),
            Stmt::Import(specifier) => match resolve_file_import(current_source_dir, specifier, context) {
                Ok(resolved) => imports.push(ResolvedImport {
                    line: current_line,
                    kind: ModuleImportKind::File,
                    specifier: specifier.clone(),
                    resolved: Some(resolved),
                }),
                Err(_) => {
                    imports.push(ResolvedImport {
                        line: current_line,
                        kind: ModuleImportKind::File,
                        specifier: specifier.clone(),
                        resolved: None,
                    });
                    state.diagnostics.push(ToolingDiagnostic {
                        severity: DiagnosticSeverity::Error,
                        code: "unresolved_import",
                        path: path_string(current_module_path),
                        line: current_line,
                        message: format!("unresolved file import '{}'", specifier),
                    });
                }
            },
            Stmt::ImportModule(name) => {
                if is_builtin_module(name) {
                    imports.push(ResolvedImport {
                        line: current_line,
                        kind: ModuleImportKind::Builtin,
                        specifier: name.clone(),
                        resolved: None,
                    });
                } else if let Some(resolved) = resolver.resolve_module(current_source_dir, name) {
                    imports.push(ResolvedImport {
                        line: current_line,
                        kind: ModuleImportKind::Module,
                        specifier: name.clone(),
                        resolved: Some(resolved),
                    });
                } else {
                    imports.push(ResolvedImport {
                        line: current_line,
                        kind: ModuleImportKind::Module,
                        specifier: name.clone(),
                        resolved: None,
                    });
                    state.diagnostics.push(ToolingDiagnostic {
                        severity: DiagnosticSeverity::Error,
                        code: "unresolved_import",
                        path: path_string(current_module_path),
                        line: current_line,
                        message: format!("unresolved module import '{}'", name),
                    });
                }
            }
            Stmt::If {
                then_body,
                elif_clauses,
                else_body,
                ..
            } => {
                collect_graph_imports_from_block(
                    then_body,
                    current_source_dir,
                    resolver,
                    current_module_path,
                    state,
                    imports,
                    context,
                );
                for (_, body) in elif_clauses {
                    collect_graph_imports_from_block(
                        body,
                        current_source_dir,
                        resolver,
                        current_module_path,
                        state,
                        imports,
                        context,
                    );
                }
                if let Some(body) = else_body {
                    collect_graph_imports_from_block(
                        body,
                        current_source_dir,
                        resolver,
                        current_module_path,
                        state,
                        imports,
                        context,
                    );
                }
            }
            Stmt::While { body, .. }
            | Stmt::For { body, .. }
            | Stmt::FnDef { body, .. }
            | Stmt::Class { body, .. }
            | Stmt::With { body, .. } => {
                collect_graph_imports_from_block(
                    body,
                    current_source_dir,
                    resolver,
                    current_module_path,
                    state,
                    imports,
                    context,
                );
            }
            Stmt::Try {
                body,
                handlers,
                else_body,
                finally_body,
            } => {
                collect_graph_imports_from_block(
                    body,
                    current_source_dir,
                    resolver,
                    current_module_path,
                    state,
                    imports,
                    context,
                );
                for handler in handlers {
                    collect_graph_imports_from_block(
                        &handler.body,
                        current_source_dir,
                        resolver,
                        current_module_path,
                        state,
                        imports,
                        context,
                    );
                }
                if let Some(body) = else_body {
                    collect_graph_imports_from_block(
                        body,
                        current_source_dir,
                        resolver,
                        current_module_path,
                        state,
                        imports,
                        context,
                    );
                }
                if let Some(body) = finally_body {
                    collect_graph_imports_from_block(
                        body,
                        current_source_dir,
                        resolver,
                        current_module_path,
                        state,
                        imports,
                        context,
                    );
                }
            }
            _ => {}
        }
    }
}

fn resolve_file_import(current_source_dir: &Path, specifier: &str, context: &str) -> Result<PathBuf, String> {
    let full_path = if Path::new(specifier).is_absolute() {
        PathBuf::from(specifier)
    } else {
        current_source_dir.join(specifier)
    };
    if full_path.exists() {
        Ok(full_path.canonicalize().unwrap_or(full_path))
    } else {
        Err(format!(
            "{context}: unresolved file import '{}' from '{}'",
            specifier,
            current_source_dir.display()
        ))
    }
}

fn import_line_for_child(imports: &[ResolvedImport], child: &Path) -> Option<usize> {
    imports.iter().find_map(|import| match &import.resolved {
        Some(resolved) if resolved == child => import.line,
        _ => None,
    })
}

fn check_report_warnings(report: &InspectReport) -> Vec<ToolingDiagnostic> {
    let mut diagnostics = Vec::new();
    diagnostics.extend(top_level_duplicate_warnings(report));
    for class in &report.classes {
        diagnostics.extend(class_duplicate_warnings(&report.path, class));
    }
    diagnostics
}

fn symbols_from_report(report: &InspectReport) -> Vec<SymbolLocation> {
    let mut symbols = Vec::new();

    for import in &report.imports {
        if matches!(import.kind, ModuleImportKind::File) {
            continue;
        }
        symbols.push(SymbolLocation {
            path: report.path.clone(),
            line: import.line,
            name: import_binding_name(&import.specifier),
            kind: SymbolKind::Import,
            container: None,
            import_kind: Some(import.kind),
            import_specifier: Some(import.specifier.clone()),
            import_resolved: import.resolved.clone(),
        });
    }

    for function in &report.functions {
        symbols.push(SymbolLocation {
            path: report.path.clone(),
            line: function.line,
            name: function.name.clone(),
            kind: SymbolKind::Function,
            container: None,
            import_kind: None,
            import_specifier: None,
            import_resolved: None,
        });
    }

    for class in &report.classes {
        symbols.push(SymbolLocation {
            path: report.path.clone(),
            line: class.line,
            name: class.name.clone(),
            kind: SymbolKind::Class,
            container: None,
            import_kind: None,
            import_specifier: None,
            import_resolved: None,
        });
        for method in &class.methods {
            symbols.push(SymbolLocation {
                path: report.path.clone(),
                line: method.line,
                name: method.name.clone(),
                kind: SymbolKind::Method,
                container: Some(class.name.clone()),
                import_kind: None,
                import_specifier: None,
                import_resolved: None,
            });
        }
        for assignment in &class.class_assignments {
            if !assignment_defines_symbol(assignment) {
                continue;
            }
            for name in &assignment.names {
                symbols.push(SymbolLocation {
                    path: report.path.clone(),
                    line: assignment.line,
                    name: name.clone(),
                    kind: SymbolKind::ClassAssignment,
                    container: Some(class.name.clone()),
                    import_kind: None,
                    import_specifier: None,
                    import_resolved: None,
                });
            }
        }
    }

    for structure in &report.structs {
        symbols.push(SymbolLocation {
            path: report.path.clone(),
            line: structure.line,
            name: structure.name.clone(),
            kind: SymbolKind::Struct,
            container: None,
            import_kind: None,
            import_specifier: None,
            import_resolved: None,
        });
    }

    for assignment in &report.assignments {
        if !assignment_defines_symbol(assignment) {
            continue;
        }
        for name in &assignment.names {
            symbols.push(SymbolLocation {
                path: report.path.clone(),
                line: assignment.line,
                name: name.clone(),
                kind: SymbolKind::Assignment,
                container: None,
                import_kind: None,
                import_specifier: None,
                import_resolved: None,
            });
        }
    }

    symbols
}

fn inspect_program(path: String, program: &Program, imports: Vec<ModuleGraphImport>) -> InspectReport {
    let mut functions = Vec::new();
    let mut classes = Vec::new();
    let mut structs = Vec::new();
    let mut assignments = Vec::new();
    let mut current_line = None;

    for stmt in program {
        match stmt {
            Stmt::SetLine(line) => current_line = Some(*line),
            Stmt::FnDef { name, params, .. } => functions.push(InspectFunction {
                line: current_line,
                name: name.clone(),
                params: inspect_params(params),
            }),
            Stmt::ExternFn { name, params, .. } => functions.push(InspectFunction {
                line: current_line,
                name: name.clone(),
                params: inspect_extern_params(params),
            }),
            Stmt::Class { name, parent, body } => {
                let (methods, class_assignments) = inspect_class_body(body);
                classes.push(InspectClass {
                    line: current_line,
                    name: name.clone(),
                    parent: parent.clone(),
                    methods,
                    class_assignments,
                });
            }
            Stmt::Struct {
                name,
                fields,
                is_packed,
            } => structs.push(InspectStruct {
                line: current_line,
                name: name.clone(),
                is_packed: *is_packed,
                fields: fields
                    .iter()
                    .map(|(field_name, type_name)| InspectStructField {
                        name: field_name.clone(),
                        type_name: type_name.clone(),
                    })
                    .collect(),
            }),
            Stmt::Union { name, fields } => structs.push(InspectStruct {
                line: current_line,
                name: name.clone(),
                is_packed: false,
                fields: fields
                    .iter()
                    .map(|(field_name, type_name)| InspectStructField {
                        name: field_name.clone(),
                        type_name: type_name.clone(),
                    })
                    .collect(),
            }),
            _ => {
                if let Some(assignment) = inspect_assignment(stmt, current_line) {
                    assignments.push(assignment);
                }
            }
        }
    }

    InspectReport {
        path,
        imports,
        functions,
        classes,
        structs,
        assignments,
    }
}

fn top_level_duplicate_warnings(report: &InspectReport) -> Vec<ToolingDiagnostic> {
    #[derive(Clone)]
    struct Symbol {
        line: Option<usize>,
        kind: &'static str,
    }

    let mut diagnostics = Vec::new();
    let mut seen = std::collections::BTreeMap::<String, Symbol>::new();

    for import in &report.imports {
        if matches!(import.kind, ModuleImportKind::File) {
            continue;
        }
        let binding = import.specifier.rsplit('.').next().unwrap_or(&import.specifier);
        let symbol = Symbol {
            line: import.line,
            kind: "import",
        };
        if let Some(previous) = seen.get(binding) {
            diagnostics.push(ToolingDiagnostic {
                severity: DiagnosticSeverity::Warning,
                code: "duplicate_symbol",
                path: report.path.clone(),
                line: import.line,
                message: format!(
                    "top-level symbol '{}' ({}) duplicates earlier {} at line {}",
                    binding,
                    symbol.kind,
                    previous.kind,
                    previous.line.unwrap_or(0)
                ),
            });
        } else {
            seen.insert(binding.to_string(), symbol);
        }
    }

    for function in &report.functions {
        let symbol = Symbol {
            line: function.line,
            kind: "function",
        };
        if let Some(previous) = seen.get(&function.name) {
            diagnostics.push(ToolingDiagnostic {
                severity: DiagnosticSeverity::Warning,
                code: "duplicate_symbol",
                path: report.path.clone(),
                line: function.line,
                message: format!(
                    "top-level symbol '{}' ({}) duplicates earlier {} at line {}",
                    function.name,
                    symbol.kind,
                    previous.kind,
                    previous.line.unwrap_or(0)
                ),
            });
        } else {
            seen.insert(function.name.clone(), symbol);
        }
    }

    for class in &report.classes {
        let symbol = Symbol {
            line: class.line,
            kind: "class",
        };
        if let Some(previous) = seen.get(&class.name) {
            diagnostics.push(ToolingDiagnostic {
                severity: DiagnosticSeverity::Warning,
                code: "duplicate_symbol",
                path: report.path.clone(),
                line: class.line,
                message: format!(
                    "top-level symbol '{}' ({}) duplicates earlier {} at line {}",
                    class.name,
                    symbol.kind,
                    previous.kind,
                    previous.line.unwrap_or(0)
                ),
            });
        } else {
            seen.insert(class.name.clone(), symbol);
        }
    }

    for structure in &report.structs {
        let symbol = Symbol {
            line: structure.line,
            kind: "struct",
        };
        if let Some(previous) = seen.get(&structure.name) {
            diagnostics.push(ToolingDiagnostic {
                severity: DiagnosticSeverity::Warning,
                code: "duplicate_symbol",
                path: report.path.clone(),
                line: structure.line,
                message: format!(
                    "top-level symbol '{}' ({}) duplicates earlier {} at line {}",
                    structure.name,
                    symbol.kind,
                    previous.kind,
                    previous.line.unwrap_or(0)
                ),
            });
        } else {
            seen.insert(structure.name.clone(), symbol);
        }
    }

    diagnostics
}

fn class_duplicate_warnings(path: &str, class: &InspectClass) -> Vec<ToolingDiagnostic> {
    #[derive(Clone)]
    struct Member {
        line: Option<usize>,
        kind: &'static str,
    }

    let mut diagnostics = Vec::new();
    let mut seen = std::collections::BTreeMap::<String, Member>::new();

    for assignment in &class.class_assignments {
        for name in &assignment.names {
            let member = Member {
                line: assignment.line,
                kind: "attribute",
            };
            if let Some(previous) = seen.get(name) {
                diagnostics.push(ToolingDiagnostic {
                    severity: DiagnosticSeverity::Warning,
                    code: "duplicate_member",
                    path: path.to_string(),
                    line: assignment.line,
                    message: format!(
                        "class '{}' member '{}' ({}) duplicates earlier {} at line {}",
                        class.name,
                        name,
                        member.kind,
                        previous.kind,
                        previous.line.unwrap_or(0)
                    ),
                });
            } else {
                seen.insert(name.clone(), member);
            }
        }
    }

    for method in &class.methods {
        let member = Member {
            line: method.line,
            kind: "method",
        };
        if let Some(previous) = seen.get(&method.name) {
            diagnostics.push(ToolingDiagnostic {
                severity: DiagnosticSeverity::Warning,
                code: "duplicate_member",
                path: path.to_string(),
                line: method.line,
                message: format!(
                    "class '{}' member '{}' ({}) duplicates earlier {} at line {}",
                    class.name,
                    method.name,
                    member.kind,
                    previous.kind,
                    previous.line.unwrap_or(0)
                ),
            });
        } else {
            seen.insert(method.name.clone(), member);
        }
    }

    diagnostics
}

fn diagnostic_sort_key(diagnostic: &ToolingDiagnostic) -> (u8, String, Option<usize>, &'static str, String) {
    (
        match diagnostic.severity {
            DiagnosticSeverity::Error => 0,
            DiagnosticSeverity::Warning => 1,
        },
        diagnostic.path.clone(),
        diagnostic.line,
        diagnostic.code,
        diagnostic.message.clone(),
    )
}

fn symbol_sort_key(symbol: &SymbolLocation) -> (String, Option<usize>, u8, Option<String>, String, Option<String>) {
    (
        symbol.path.clone(),
        symbol.line,
        match symbol.kind {
            SymbolKind::Import => 0,
            SymbolKind::Assignment => 1,
            SymbolKind::Function => 2,
            SymbolKind::Class => 3,
            SymbolKind::ClassAssignment => 4,
            SymbolKind::Method => 5,
            SymbolKind::Struct => 6,
        },
        symbol.container.clone(),
        symbol.name.clone(),
        symbol.import_specifier.clone(),
    )
}

fn diff_keyed<T, F>(before: &[T], after: &[T], key_fn: F) -> DiffSetWithChanges<T>
where
    T: Clone + PartialEq,
    F: Fn(&T) -> String,
{
    let before_map: std::collections::BTreeMap<String, T> =
        before.iter().cloned().map(|item| (key_fn(&item), item)).collect();
    let after_map: std::collections::BTreeMap<String, T> =
        after.iter().cloned().map(|item| (key_fn(&item), item)).collect();

    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();

    for (key, before_item) in &before_map {
        match after_map.get(key) {
            Some(after_item) if after_item != before_item => changed.push(DiffChange {
                before: before_item.clone(),
                after: after_item.clone(),
            }),
            Some(_) => {}
            None => removed.push(before_item.clone()),
        }
    }

    for (key, after_item) in &after_map {
        if !before_map.contains_key(key) {
            added.push(after_item.clone());
        }
    }

    DiffSetWithChanges {
        added,
        removed,
        changed,
    }
}

fn diff_by_identity<T, F>(before: &[T], after: &[T], key_fn: F) -> DiffSet<T>
where
    T: Clone,
    F: Fn(&T) -> String,
{
    let before_map: std::collections::BTreeMap<String, T> =
        before.iter().cloned().map(|item| (key_fn(&item), item)).collect();
    let after_map: std::collections::BTreeMap<String, T> =
        after.iter().cloned().map(|item| (key_fn(&item), item)).collect();

    let removed = before_map
        .iter()
        .filter(|(key, _)| !after_map.contains_key(*key))
        .map(|(_, item)| item.clone())
        .collect();
    let added = after_map
        .iter()
        .filter(|(key, _)| !before_map.contains_key(*key))
        .map(|(_, item)| item.clone())
        .collect();

    DiffSet { added, removed }
}

fn assignment_identity_key(item: &InspectAssignment) -> String {
    format!("{}:{}:{}", item.line.unwrap_or(0), item.kind, item.names.join(","))
}

fn assignment_defines_symbol(item: &InspectAssignment) -> bool {
    matches!(item.kind, "assign" | "data" | "aug_assign" | "unpack")
}

fn import_binding_name(specifier: &str) -> String {
    specifier.rsplit('.').next().unwrap_or(specifier).to_string()
}

fn inspect_params(params: &[crate::ast::Param]) -> Vec<InspectParam> {
    params
        .iter()
        .map(|param| InspectParam {
            name: param.name.clone(),
            has_default: param.default.is_some(),
            is_vararg: param.is_vararg,
            is_kwarg: param.is_kwarg,
        })
        .collect()
}

fn inspect_extern_params(params: &[crate::ast::ExternParam]) -> Vec<InspectParam> {
    params
        .iter()
        .map(|param| InspectParam {
            name: param.name.clone(),
            has_default: false,
            is_vararg: false,
            is_kwarg: false,
        })
        .collect()
}

fn inspect_class_body(body: &[Stmt]) -> (Vec<InspectFunction>, Vec<InspectAssignment>) {
    let mut methods = Vec::new();
    let mut assignments = Vec::new();
    let mut current_line = None;

    for stmt in body {
        match stmt {
            Stmt::SetLine(line) => current_line = Some(*line),
            Stmt::FnDef { name, params, .. } => methods.push(InspectFunction {
                line: current_line,
                name: name.clone(),
                params: inspect_params(params),
            }),
            _ => {
                if let Some(assignment) = inspect_assignment(stmt, current_line) {
                    assignments.push(assignment);
                }
            }
        }
    }

    (methods, assignments)
}

fn inspect_assignment(stmt: &Stmt, line: Option<usize>) -> Option<InspectAssignment> {
    let (kind, names) = match stmt {
        Stmt::Assign { name, .. } => ("assign", vec![name.clone()]),
        Stmt::Data { name, .. } => ("data", vec![name.clone()]),
        Stmt::AugAssign { name, .. } => ("aug_assign", vec![name.clone()]),
        Stmt::Unpack { names, .. } => ("unpack", names.clone()),
        Stmt::Global(names) => ("global", names.clone()),
        Stmt::Nonlocal(names) => ("nonlocal", names.clone()),
        _ => return None,
    };

    Some(InspectAssignment { line, kind, names })
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
        Stmt::FnDef {
            name,
            params,
            section,
            entry,
            body,
        } => Some(Stmt::FnDef {
            name: name.clone(),
            params: params.clone(),
            section: section.clone(),
            entry: entry.clone(),
            body: strip_line_markers(body),
        }),
        Stmt::ExternFn {
            name,
            params,
            return_type,
            symbol,
            callconv,
            section,
        } => Some(Stmt::ExternFn {
            name: name.clone(),
            params: params.clone(),
            return_type: return_type.clone(),
            symbol: symbol.clone(),
            callconv: callconv.clone(),
            section: section.clone(),
        }),
        Stmt::Data {
            name,
            type_name,
            value,
            section,
        } => Some(Stmt::Data {
            name: name.clone(),
            type_name: type_name.clone(),
            value: value.clone(),
            section: section.clone(),
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
