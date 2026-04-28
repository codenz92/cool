use crate::ast::{ExceptHandler, Expr, MatchArm, Pattern, Program, Stmt, TraitMethod, TypeParam, Visibility};
use crate::lexer::Lexer;
use crate::module_exports;
use crate::parser::Parser;
use crate::project::{CoolProject, ModuleResolver};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
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
    diags.extend(type_check_program(&program, path, false, None));
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
pub struct DocReport {
    pub entry: String,
    pub modules: Vec<DocModule>,
}

#[derive(Clone, Serialize)]
pub struct DocModule {
    pub name: String,
    pub path: String,
    pub is_entry: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    pub functions: Vec<DocFunction>,
    pub classes: Vec<DocClass>,
    pub types: Vec<DocType>,
    pub bindings: Vec<DocBinding>,
}

#[derive(Clone, Serialize)]
pub struct DocFunction {
    pub line: Option<usize>,
    pub name: String,
    pub kind: &'static str,
    pub params: Vec<InspectParam>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<Visibility>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extern_metadata: Option<DocExternMetadata>,
}

#[derive(Clone, Serialize)]
pub struct DocExternMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callconv: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_kind: Option<String>,
    pub weak: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ownership: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifetime: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct DocClass {
    pub line: Option<usize>,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<Visibility>,
    pub methods: Vec<DocFunction>,
    pub class_bindings: Vec<DocBinding>,
}

#[derive(Clone, Serialize)]
pub struct DocType {
    pub line: Option<usize>,
    pub name: String,
    pub kind: &'static str,
    pub is_packed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<Visibility>,
    pub fields: Vec<InspectStructField>,
}

#[derive(Clone, Serialize)]
pub struct DocBinding {
    pub line: Option<usize>,
    pub name: String,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
    #[serde(skip_serializing_if = "is_false")]
    pub is_const: bool,
    pub visibility: Visibility,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_type: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
    #[serde(skip_serializing_if = "is_false")]
    pub is_const: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<Visibility>,
}

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct InspectParam {
    pub name: String,
    pub has_default: bool,
    pub is_vararg: bool,
    pub is_kwarg: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
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

#[derive(Clone, Serialize)]
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

pub fn build_check_report(path: &Path, strict: bool) -> Result<CheckReport, String> {
    let graph = build_module_graph(path)?;
    let mut diagnostics = graph.diagnostics.clone();
    let mut parsed_modules = Vec::new();
    for module in &graph.modules {
        let program = parse_file(Path::new(&module.path))?;
        let report = inspect_program(module.path.clone(), &program, module.imports.clone());
        parsed_modules.push((module.clone(), program, report));
    }
    let exports_by_path: HashMap<String, HashSet<String>> = parsed_modules
        .iter()
        .map(|(_, program, report)| {
            (
                report.path.clone(),
                module_exports::exported_names(program).into_iter().collect(),
            )
        })
        .collect();
    for (module, program, report) in parsed_modules {
        diagnostics.extend(check_report_warnings(&report));
        diagnostics.extend(type_check_program(
            &program,
            &module.path,
            strict,
            Some(ModuleCheckContext {
                imports: module.imports.clone(),
                exports_by_path: exports_by_path.clone(),
            }),
        ));
    }
    diagnostics.sort_by_key(diagnostic_sort_key);
    Ok(CheckReport {
        entry: graph.entry.clone(),
        modules_checked: graph.modules.len(),
        diagnostics,
    })
}

pub fn build_doc_report(path: &Path, include_private: bool) -> Result<DocReport, String> {
    let graph = build_module_graph(path)?;
    let errors: Vec<&ToolingDiagnostic> = graph
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
        .collect();
    if !errors.is_empty() {
        let details = errors
            .into_iter()
            .map(format_tooling_diagnostic)
            .collect::<Vec<_>>()
            .join("\n");
        return Err(format!("doc: cannot generate docs due to source errors\n{details}"));
    }

    let name_map = infer_module_doc_names(&graph, Path::new(&graph.entry))?;
    let modules = graph
        .modules
        .iter()
        .map(|module| build_doc_module(module, &graph.entry, &name_map, include_private))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(DocReport {
        entry: graph.entry,
        modules,
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

fn infer_module_doc_names(graph: &ModuleGraph, entry_path: &Path) -> Result<HashMap<String, String>, String> {
    let mut names = HashMap::new();
    for module in &graph.modules {
        for import in &module.imports {
            let Some(resolved) = &import.resolved else {
                continue;
            };
            names
                .entry(resolved.clone())
                .or_insert_with(|| import.specifier.clone());
        }
    }

    let project = entry_path
        .parent()
        .and_then(|dir| CoolProject::discover(dir).ok().flatten());
    let project_roots = if let Some(project) = &project {
        Some(project.local_module_roots()?)
    } else {
        None
    };

    for module in &graph.modules {
        names.entry(module.path.clone()).or_insert_with(|| {
            derive_module_doc_name(Path::new(&module.path), project.as_ref(), project_roots.as_deref())
        });
    }

    Ok(names)
}

fn derive_module_doc_name(path: &Path, project: Option<&CoolProject>, project_roots: Option<&[PathBuf]>) -> String {
    if let Some(roots) = project_roots {
        for root in roots {
            if let Ok(relative) = path.strip_prefix(root) {
                return module_name_from_relative(relative);
            }
        }
    }

    if let Some(project) = project {
        if let Ok(relative) = path.strip_prefix(&project.root) {
            return module_name_from_relative(relative);
        }
    }

    module_name_from_relative(path.file_name().map(Path::new).unwrap_or(path))
}

fn module_name_from_relative(relative: &Path) -> String {
    let mut stem_path = relative.to_path_buf();
    if stem_path.extension().and_then(|ext| ext.to_str()) == Some("cool") {
        stem_path.set_extension("");
    }
    let mut parts: Vec<String> = stem_path
        .iter()
        .filter_map(|part| part.to_str())
        .map(|part| part.to_string())
        .collect();
    if parts.last().map(|part| part.as_str()) == Some("__init__") {
        parts.pop();
    }
    if parts.is_empty() {
        "main".to_string()
    } else {
        parts.join(".")
    }
}

fn build_doc_module(
    module: &ModuleGraphModule,
    entry: &str,
    names: &HashMap<String, String>,
    include_private: bool,
) -> Result<DocModule, String> {
    let program = parse_file(Path::new(&module.path))?;
    let name = names.get(&module.path).cloned().unwrap_or_else(|| module.path.clone());
    let mut functions = Vec::new();
    let mut classes = Vec::new();
    let mut types = Vec::new();
    let mut bindings = Vec::new();
    let mut current_line = None;

    for raw_stmt in &program {
        let effective_visibility = effective_visibility(raw_stmt);
        let is_public = module_exports::stmt_is_public_export(raw_stmt);
        if !include_private && !is_public {
            match module_exports::stmt_unwrap_visibility(raw_stmt).0 {
                Stmt::SetLine(_) | Stmt::Expr(Expr::Str(_)) => {}
                _ => continue,
            }
        }

        let (stmt, _) = module_exports::stmt_unwrap_visibility(raw_stmt);
        match stmt {
            Stmt::SetLine(line) => current_line = Some(*line),
            Stmt::FnDef {
                name,
                params,
                return_type,
                body,
                ..
            } => functions.push(DocFunction {
                line: current_line,
                name: name.clone(),
                kind: "function",
                params: inspect_params(params),
                return_type: return_type.clone(),
                doc: extract_docstring(body),
                visibility: Some(effective_visibility),
                extern_metadata: None,
            }),
            Stmt::ExternFn {
                name,
                params,
                return_type,
                symbol,
                callconv,
                section,
                library,
                link_kind,
                weak,
                ownership,
                lifetime,
            } => functions.push(DocFunction {
                line: current_line,
                name: name.clone(),
                kind: "extern",
                params: inspect_extern_params(params),
                return_type: Some(return_type.clone()),
                doc: None,
                visibility: Some(effective_visibility),
                extern_metadata: Some(DocExternMetadata {
                    symbol: symbol.clone(),
                    callconv: callconv.clone(),
                    section: section.clone(),
                    library: library.clone(),
                    link_kind: link_kind.clone(),
                    weak: *weak,
                    ownership: ownership.clone(),
                    lifetime: lifetime.clone(),
                }),
            }),
            Stmt::Class { name, parent, body, .. } => {
                let (methods, class_bindings) = doc_class_body(body, include_private);
                classes.push(DocClass {
                    line: current_line,
                    name: name.clone(),
                    parent: parent.clone(),
                    doc: extract_docstring(body),
                    visibility: Some(effective_visibility),
                    methods,
                    class_bindings,
                });
            }
            Stmt::Trait { name, methods, .. } => classes.push(DocClass {
                line: current_line,
                name: name.clone(),
                parent: None,
                doc: None,
                visibility: Some(effective_visibility),
                methods: methods
                    .iter()
                    .map(|method| DocFunction {
                        line: current_line,
                        name: method.name.clone(),
                        kind: "trait_method",
                        params: inspect_params(&method.params),
                        return_type: method.return_type.clone(),
                        doc: None,
                        visibility: Some(effective_visibility),
                        extern_metadata: None,
                    })
                    .collect(),
                class_bindings: Vec::new(),
            }),
            Stmt::Struct {
                name,
                fields,
                is_packed,
                ..
            } => types.push(DocType {
                line: current_line,
                name: name.clone(),
                kind: "struct",
                is_packed: *is_packed,
                doc: None,
                visibility: Some(effective_visibility),
                fields: fields
                    .iter()
                    .map(|(field_name, type_name)| InspectStructField {
                        name: field_name.clone(),
                        type_name: type_name.clone(),
                    })
                    .collect(),
            }),
            Stmt::Union { name, fields, .. } => types.push(DocType {
                line: current_line,
                name: name.clone(),
                kind: "union",
                is_packed: false,
                doc: None,
                visibility: Some(effective_visibility),
                fields: fields
                    .iter()
                    .map(|(field_name, type_name)| InspectStructField {
                        name: field_name.clone(),
                        type_name: type_name.clone(),
                    })
                    .collect(),
            }),
            Stmt::Enum { name, variants, .. } => types.push(DocType {
                line: current_line,
                name: name.clone(),
                kind: "enum",
                is_packed: false,
                doc: None,
                visibility: Some(effective_visibility),
                fields: variants
                    .iter()
                    .map(|variant| InspectStructField {
                        name: variant.name.clone(),
                        type_name: if variant.fields.is_empty() {
                            "unit".to_string()
                        } else {
                            variant
                                .fields
                                .iter()
                                .map(|(field_name, type_name)| format!("{field_name}: {type_name}"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        },
                    })
                    .collect(),
            }),
            _ => {
                if let Some(binding) = doc_binding(raw_stmt, current_line) {
                    bindings.extend(binding);
                }
            }
        }
    }

    Ok(DocModule {
        name,
        path: module.path.clone(),
        is_entry: module.path == entry,
        doc: extract_docstring(&program),
        functions,
        classes,
        types,
        bindings,
    })
}

fn doc_class_body(body: &[Stmt], include_private: bool) -> (Vec<DocFunction>, Vec<DocBinding>) {
    let mut methods = Vec::new();
    let mut bindings = Vec::new();
    let mut current_line = None;

    for raw_stmt in body {
        let visibility = effective_visibility(raw_stmt);
        let is_public = module_exports::stmt_is_public_export(raw_stmt);
        if !include_private && !is_public {
            match module_exports::stmt_unwrap_visibility(raw_stmt).0 {
                Stmt::SetLine(_) | Stmt::Expr(Expr::Str(_)) => {}
                _ => continue,
            }
        }

        match module_exports::stmt_unwrap_visibility(raw_stmt).0 {
            Stmt::SetLine(line) => current_line = Some(*line),
            Stmt::FnDef {
                name,
                params,
                return_type,
                body,
                ..
            } => methods.push(DocFunction {
                line: current_line,
                name: name.clone(),
                kind: "method",
                params: inspect_params(params),
                return_type: return_type.clone(),
                doc: extract_docstring(body),
                visibility: Some(visibility),
                extern_metadata: None,
            }),
            _ => {
                if let Some(items) = doc_binding(raw_stmt, current_line) {
                    bindings.extend(items);
                }
            }
        }
    }

    (methods, bindings)
}

fn doc_binding(raw_stmt: &Stmt, line: Option<usize>) -> Option<Vec<DocBinding>> {
    let visibility = effective_visibility(raw_stmt);
    let (stmt, _) = module_exports::stmt_unwrap_visibility(raw_stmt);
    let (kind, names, type_name, is_const) = match stmt {
        Stmt::Assign { name, .. } => ("assign", vec![name.clone()], None, false),
        Stmt::VarDecl {
            name,
            type_name,
            is_const,
            ..
        } => (
            if *is_const { "const" } else { "var" },
            vec![name.clone()],
            type_name.clone(),
            *is_const,
        ),
        Stmt::Data { name, type_name, .. } => ("data", vec![name.clone()], Some(type_name.clone()), false),
        _ => return None,
    };

    Some(
        names
            .into_iter()
            .map(|name| DocBinding {
                line,
                name,
                kind,
                type_name: type_name.clone(),
                is_const,
                visibility,
            })
            .collect(),
    )
}

fn extract_docstring(stmts: &[Stmt]) -> Option<String> {
    for stmt in stmts {
        match stmt {
            Stmt::SetLine(_) => continue,
            Stmt::Expr(Expr::Str(text)) => {
                let doc = text.trim().to_string();
                return if doc.is_empty() { None } else { Some(doc) };
            }
            Stmt::Visibility { stmt, .. } => match stmt.as_ref() {
                Stmt::Expr(Expr::Str(text)) => {
                    let doc = text.trim().to_string();
                    return if doc.is_empty() { None } else { Some(doc) };
                }
                _ => return None,
            },
            _ => return None,
        }
    }
    None
}

fn effective_visibility(stmt: &Stmt) -> Visibility {
    if module_exports::stmt_is_public_export(stmt) {
        Visibility::Public
    } else {
        Visibility::Private
    }
}

fn render_function_signature(function: &DocFunction) -> String {
    let params = function
        .params
        .iter()
        .map(render_param_signature)
        .collect::<Vec<_>>()
        .join(", ");
    let prefix = match function.kind {
        "extern" => "extern def",
        _ => "def",
    };
    match &function.return_type {
        Some(return_type) => format!("{prefix} {}({}) -> {}", function.name, params, return_type),
        None => format!("{prefix} {}({})", function.name, params),
    }
}

fn render_param_signature(param: &InspectParam) -> String {
    let mut name = String::new();
    if param.is_kwarg {
        name.push_str("**");
    } else if param.is_vararg {
        name.push('*');
    }
    name.push_str(&param.name);
    if let Some(type_name) = &param.type_name {
        name.push_str(": ");
        name.push_str(type_name);
    }
    if param.has_default {
        name.push_str(" = ...");
    }
    name
}

fn render_binding_signature(binding: &DocBinding) -> String {
    let prefix = if binding.is_const {
        "const"
    } else {
        match binding.kind {
            "data" => "data",
            "var" => "var",
            _ => "let",
        }
    };
    match &binding.type_name {
        Some(type_name) => format!("{prefix} {}: {}", binding.name, type_name),
        None => format!("{prefix} {}", binding.name),
    }
}

fn render_extern_metadata_markdown(metadata: &DocExternMetadata) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(symbol) = &metadata.symbol {
        parts.push(format!("symbol=`{symbol}`"));
    }
    if let Some(callconv) = &metadata.callconv {
        parts.push(format!("cc=`{callconv}`"));
    }
    if let Some(section) = &metadata.section {
        parts.push(format!("section=`{section}`"));
    }
    if let Some(library) = &metadata.library {
        parts.push(format!("library=`{library}`"));
    }
    if let Some(link_kind) = &metadata.link_kind {
        parts.push(format!("link_kind=`{link_kind}`"));
    }
    if metadata.weak {
        parts.push("weak=`true`".to_string());
    }
    if let Some(ownership) = &metadata.ownership {
        parts.push(format!("ownership=`{ownership}`"));
    }
    if let Some(lifetime) = &metadata.lifetime {
        parts.push(format!("lifetime=`{lifetime}`"));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

fn render_extern_metadata_html(metadata: &DocExternMetadata) -> Option<String> {
    render_extern_metadata_markdown(metadata).map(|text| {
        format!(
            "<p><strong>Extern metadata:</strong> <code>{}</code></p>",
            escape_html(&text)
        )
    })
}

fn render_markdown_visibility(visibility: Option<Visibility>) -> &'static str {
    match visibility {
        Some(Visibility::Private) => " _(private)_",
        _ => "",
    }
}

fn render_html_visibility(visibility: Option<Visibility>) -> &'static str {
    match visibility {
        Some(Visibility::Private) => " <span class=\"visibility\">private</span>",
        _ => "",
    }
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn render_doc_html_text(text: &str) -> String {
    let paragraphs: Vec<String> = text
        .split("\n\n")
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| format!("<p>{}</p>", escape_html(part).replace('\n', "<br>")))
        .collect();
    paragraphs.join("")
}

pub fn render_doc_markdown(report: &DocReport) -> String {
    let mut out = String::new();
    out.push_str("# Cool API Docs\n\n");
    out.push_str(&format!("Entry: `{}`\n\n", report.entry));

    for module in &report.modules {
        out.push_str(&format!("## Module `{}`\n\n", module.name));
        out.push_str(&format!("Path: `{}`\n\n", module.path));
        if let Some(doc) = &module.doc {
            out.push_str(doc.trim());
            out.push_str("\n\n");
        }

        if !module.functions.is_empty() {
            out.push_str("### Functions\n\n");
            for function in &module.functions {
                out.push_str(&format!(
                    "#### `{}`{}\n\n",
                    render_function_signature(function),
                    render_markdown_visibility(function.visibility)
                ));
                if let Some(doc) = &function.doc {
                    out.push_str(doc.trim());
                    out.push_str("\n\n");
                }
                if let Some(metadata) = &function.extern_metadata {
                    if let Some(summary) = render_extern_metadata_markdown(metadata) {
                        out.push_str(&format!("Extern metadata: {summary}\n\n"));
                    }
                }
            }
        }

        if !module.classes.is_empty() {
            out.push_str("### Classes\n\n");
            for class in &module.classes {
                let header = match &class.parent {
                    Some(parent) => format!("class {}({})", class.name, parent),
                    None => format!("class {}", class.name),
                };
                out.push_str(&format!(
                    "#### `{header}`{}\n\n",
                    render_markdown_visibility(class.visibility)
                ));
                if let Some(doc) = &class.doc {
                    out.push_str(doc.trim());
                    out.push_str("\n\n");
                }
                if !class.methods.is_empty() {
                    out.push_str("##### Methods\n\n");
                    for method in &class.methods {
                        out.push_str(&format!(
                            "###### `{}`{}\n\n",
                            render_function_signature(method),
                            render_markdown_visibility(method.visibility)
                        ));
                        if let Some(doc) = &method.doc {
                            out.push_str(doc.trim());
                            out.push_str("\n\n");
                        }
                    }
                }
                if !class.class_bindings.is_empty() {
                    out.push_str("##### Bindings\n\n");
                    for binding in &class.class_bindings {
                        out.push_str(&format!(
                            "- `{}`{}\n",
                            render_binding_signature(binding),
                            render_markdown_visibility(Some(binding.visibility))
                        ));
                    }
                    out.push('\n');
                }
            }
        }

        if !module.types.is_empty() {
            out.push_str("### Types\n\n");
            for item in &module.types {
                let header = match item.kind {
                    "union" => format!("union {}", item.name),
                    _ if item.is_packed => format!("packed struct {}", item.name),
                    _ => format!("struct {}", item.name),
                };
                out.push_str(&format!(
                    "#### `{header}`{}\n\n",
                    render_markdown_visibility(item.visibility)
                ));
                if let Some(doc) = &item.doc {
                    out.push_str(doc.trim());
                    out.push_str("\n\n");
                }
                if !item.fields.is_empty() {
                    out.push_str("Fields:\n");
                    for field in &item.fields {
                        out.push_str(&format!("- `{}`: `{}`\n", field.name, field.type_name));
                    }
                    out.push('\n');
                }
            }
        }

        if !module.bindings.is_empty() {
            out.push_str("### Bindings\n\n");
            for binding in &module.bindings {
                out.push_str(&format!(
                    "- `{}`{}\n",
                    render_binding_signature(binding),
                    render_markdown_visibility(Some(binding.visibility))
                ));
            }
            out.push('\n');
        }
    }

    out
}

pub fn render_doc_html(report: &DocReport) -> String {
    let mut out = String::new();
    out.push_str("<!doctype html><html><head><meta charset=\"utf-8\"><title>Cool API Docs</title>");
    out.push_str(
        "<style>body{font-family:ui-sans-serif,system-ui,sans-serif;max-width:980px;margin:40px auto;padding:0 20px;line-height:1.55}code{background:#f4f4f4;padding:0.1em 0.3em;border-radius:4px}pre{background:#f4f4f4;padding:12px;border-radius:8px;overflow:auto}h1,h2,h3,h4,h5{line-height:1.2}.visibility{color:#666;font-size:0.8em;text-transform:uppercase;letter-spacing:0.04em}ul{padding-left:20px}</style>",
    );
    out.push_str("</head><body>");
    out.push_str("<h1>Cool API Docs</h1>");
    out.push_str(&format!("<p>Entry: <code>{}</code></p>", escape_html(&report.entry)));

    for module in &report.modules {
        out.push_str(&format!("<h2>Module <code>{}</code></h2>", escape_html(&module.name)));
        out.push_str(&format!("<p>Path: <code>{}</code></p>", escape_html(&module.path)));
        if let Some(doc) = &module.doc {
            out.push_str(&render_doc_html_text(doc));
        }

        if !module.functions.is_empty() {
            out.push_str("<h3>Functions</h3>");
            for function in &module.functions {
                out.push_str(&format!(
                    "<h4><code>{}</code>{}</h4>",
                    escape_html(&render_function_signature(function)),
                    render_html_visibility(function.visibility)
                ));
                if let Some(doc) = &function.doc {
                    out.push_str(&render_doc_html_text(doc));
                }
                if let Some(metadata) = &function.extern_metadata {
                    if let Some(summary) = render_extern_metadata_html(metadata) {
                        out.push_str(&summary);
                    }
                }
            }
        }

        if !module.classes.is_empty() {
            out.push_str("<h3>Classes</h3>");
            for class in &module.classes {
                let header = match &class.parent {
                    Some(parent) => format!("class {}({})", class.name, parent),
                    None => format!("class {}", class.name),
                };
                out.push_str(&format!(
                    "<h4><code>{}</code>{}</h4>",
                    escape_html(&header),
                    render_html_visibility(class.visibility)
                ));
                if let Some(doc) = &class.doc {
                    out.push_str(&render_doc_html_text(doc));
                }
                if !class.methods.is_empty() {
                    out.push_str("<h5>Methods</h5>");
                    for method in &class.methods {
                        out.push_str(&format!(
                            "<p><code>{}</code>{}</p>",
                            escape_html(&render_function_signature(method)),
                            render_html_visibility(method.visibility)
                        ));
                        if let Some(doc) = &method.doc {
                            out.push_str(&render_doc_html_text(doc));
                        }
                    }
                }
                if !class.class_bindings.is_empty() {
                    out.push_str("<p>Bindings:</p><ul>");
                    for binding in &class.class_bindings {
                        out.push_str(&format!(
                            "<li><code>{}</code>{}</li>",
                            escape_html(&render_binding_signature(binding)),
                            render_html_visibility(Some(binding.visibility))
                        ));
                    }
                    out.push_str("</ul>");
                }
            }
        }

        if !module.types.is_empty() {
            out.push_str("<h3>Types</h3>");
            for item in &module.types {
                let header = match item.kind {
                    "union" => format!("union {}", item.name),
                    _ if item.is_packed => format!("packed struct {}", item.name),
                    _ => format!("struct {}", item.name),
                };
                out.push_str(&format!(
                    "<h4><code>{}</code>{}</h4>",
                    escape_html(&header),
                    render_html_visibility(item.visibility)
                ));
                if let Some(doc) = &item.doc {
                    out.push_str(&render_doc_html_text(doc));
                }
                if !item.fields.is_empty() {
                    out.push_str("<p>Fields:</p><ul>");
                    for field in &item.fields {
                        out.push_str(&format!(
                            "<li><code>{}</code>: <code>{}</code></li>",
                            escape_html(&field.name),
                            escape_html(&field.type_name)
                        ));
                    }
                    out.push_str("</ul>");
                }
            }
        }

        if !module.bindings.is_empty() {
            out.push_str("<h3>Bindings</h3><ul>");
            for binding in &module.bindings {
                out.push_str(&format!(
                    "<li><code>{}</code>{}</li>",
                    escape_html(&render_binding_signature(binding)),
                    render_html_visibility(Some(binding.visibility))
                ));
            }
            out.push_str("</ul>");
        }
    }

    out.push_str("</body></html>");
    out
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
            Stmt::Visibility { stmt, .. } => {
                collect_imports_from_block(
                    std::slice::from_ref(stmt.as_ref()),
                    current_source_dir,
                    resolver,
                    imports,
                    context,
                )?;
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
            Stmt::Visibility { stmt, .. } => {
                collect_graph_imports_from_block(
                    std::slice::from_ref(stmt.as_ref()),
                    current_source_dir,
                    resolver,
                    current_module_path,
                    state,
                    imports,
                    context,
                );
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
        match module_exports::stmt_unwrap_visibility(stmt).0 {
            Stmt::SetLine(line) => current_line = Some(*line),
            Stmt::FnDef {
                name,
                params,
                return_type,
                ..
            } => functions.push(InspectFunction {
                line: current_line,
                name: name.clone(),
                params: inspect_params(params),
                return_type: return_type.clone(),
            }),
            Stmt::ExternFn {
                name,
                params,
                return_type,
                ..
            } => functions.push(InspectFunction {
                line: current_line,
                name: name.clone(),
                params: inspect_extern_params(params),
                return_type: Some(return_type.clone()),
            }),
            Stmt::Class { name, parent, body, .. } => {
                let (methods, class_assignments) = inspect_class_body(body);
                classes.push(InspectClass {
                    line: current_line,
                    name: name.clone(),
                    parent: parent.clone(),
                    methods,
                    class_assignments,
                });
            }
            Stmt::Trait { name, methods, .. } => classes.push(InspectClass {
                line: current_line,
                name: name.clone(),
                parent: None,
                methods: methods
                    .iter()
                    .map(|method| InspectFunction {
                        line: current_line,
                        name: method.name.clone(),
                        params: inspect_params(&method.params),
                        return_type: method.return_type.clone(),
                    })
                    .collect(),
                class_assignments: Vec::new(),
            }),
            Stmt::Struct {
                name,
                fields,
                is_packed,
                ..
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
            Stmt::Union { name, fields, .. } => structs.push(InspectStruct {
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
            Stmt::Enum { name, variants, .. } => structs.push(InspectStruct {
                line: current_line,
                name: name.clone(),
                is_packed: false,
                fields: variants
                    .iter()
                    .map(|variant| InspectStructField {
                        name: variant.name.clone(),
                        type_name: if variant.fields.is_empty() {
                            "unit".to_string()
                        } else {
                            variant
                                .fields
                                .iter()
                                .map(|(field_name, type_name)| format!("{field_name}: {type_name}"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        },
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

    for assignment in &report.assignments {
        if !assignment_defines_symbol(assignment) {
            continue;
        }
        for name in &assignment.names {
            let symbol = Symbol {
                line: assignment.line,
                kind: assignment.kind,
            };
            if let Some(previous) = seen.get(name) {
                diagnostics.push(ToolingDiagnostic {
                    severity: DiagnosticSeverity::Warning,
                    code: "duplicate_symbol",
                    path: report.path.clone(),
                    line: assignment.line,
                    message: format!(
                        "top-level symbol '{}' ({}) duplicates earlier {} at line {}",
                        name,
                        symbol.kind,
                        previous.kind,
                        previous.line.unwrap_or(0)
                    ),
                });
            } else {
                seen.insert(name.clone(), symbol);
            }
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

fn format_tooling_diagnostic(diagnostic: &ToolingDiagnostic) -> String {
    let severity = match diagnostic.severity {
        DiagnosticSeverity::Error => "error",
        DiagnosticSeverity::Warning => "warning",
    };
    match diagnostic.line {
        Some(line) => format!(
            "{severity}[{}] {}:{}: {}",
            diagnostic.code, diagnostic.path, line, diagnostic.message
        ),
        None => format!(
            "{severity}[{}] {}: {}",
            diagnostic.code, diagnostic.path, diagnostic.message
        ),
    }
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
    matches!(
        item.kind,
        "assign" | "var_decl" | "const" | "data" | "aug_assign" | "unpack"
    )
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
            type_name: param.type_name.clone(),
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
            type_name: Some(param.type_name.clone()),
        })
        .collect()
}

fn inspect_class_body(body: &[Stmt]) -> (Vec<InspectFunction>, Vec<InspectAssignment>) {
    let mut methods = Vec::new();
    let mut assignments = Vec::new();
    let mut current_line = None;

    for stmt in body {
        match module_exports::stmt_unwrap_visibility(stmt).0 {
            Stmt::SetLine(line) => current_line = Some(*line),
            Stmt::FnDef {
                name,
                params,
                return_type,
                ..
            } => methods.push(InspectFunction {
                line: current_line,
                name: name.clone(),
                params: inspect_params(params),
                return_type: return_type.clone(),
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
    let (stmt, visibility) = module_exports::stmt_unwrap_visibility(stmt);
    let (kind, names, type_name, is_const) = match stmt {
        Stmt::Assign { name, .. } => ("assign", vec![name.clone()], None, false),
        Stmt::VarDecl {
            name,
            type_name,
            is_const,
            ..
        } => (
            if *is_const { "const" } else { "var_decl" },
            vec![name.clone()],
            type_name.clone(),
            *is_const,
        ),
        Stmt::Data { name, .. } => ("data", vec![name.clone()], None, false),
        Stmt::AugAssign { name, .. } => ("aug_assign", vec![name.clone()], None, false),
        Stmt::Unpack { names, .. } => ("unpack", names.clone(), None, false),
        Stmt::Global(names) => ("global", names.clone(), None, false),
        Stmt::Nonlocal(names) => ("nonlocal", names.clone(), None, false),
        _ => return None,
    };

    Some(InspectAssignment {
        line,
        kind,
        names,
        type_name,
        is_const,
        visibility,
    })
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
        Stmt::VarDecl {
            name,
            type_name,
            value,
            is_const,
        } => Some(Stmt::VarDecl {
            name: name.clone(),
            type_name: type_name.clone(),
            value: value.clone(),
            is_const: *is_const,
        }),
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
            type_params,
            params,
            return_type,
            section,
            entry,
            body,
        } => Some(Stmt::FnDef {
            name: name.clone(),
            type_params: type_params.clone(),
            params: params.clone(),
            return_type: return_type.clone(),
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
            library,
            link_kind,
            weak,
            ownership,
            lifetime,
        } => Some(Stmt::ExternFn {
            name: name.clone(),
            params: params.clone(),
            return_type: return_type.clone(),
            symbol: symbol.clone(),
            callconv: callconv.clone(),
            section: section.clone(),
            library: library.clone(),
            link_kind: link_kind.clone(),
            weak: *weak,
            ownership: ownership.clone(),
            lifetime: lifetime.clone(),
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
        Stmt::Class {
            name,
            parent,
            implements,
            body,
        } => Some(Stmt::Class {
            name: name.clone(),
            parent: parent.clone(),
            implements: implements.clone(),
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
        Stmt::Visibility { visibility, stmt } => Some(Stmt::Visibility {
            visibility: *visibility,
            stmt: Box::new(strip_stmt(stmt).unwrap_or_else(|| stmt.as_ref().clone())),
        }),
        _ => Some(stmt.clone()),
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

const BUILTIN_NAMES: &[&str] = &[
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
    "runfile",
    "super",
    "eval",
    "Exception",
    "ValueError",
    "TypeError",
    "RuntimeError",
    "IndexError",
    "KeyError",
    "AttributeError",
    "NameError",
];

#[derive(Clone)]
struct ModuleCheckContext {
    imports: Vec<ModuleGraphImport>,
    exports_by_path: HashMap<String, HashSet<String>>,
}

#[derive(Clone, Default)]
struct SymbolScope {
    names: HashSet<String>,
    consts: HashSet<String>,
    module_bindings: HashMap<String, String>,
    builtin_module_bindings: HashSet<String>,
    globals_declared: HashSet<String>,
    nonlocals_declared: HashSet<String>,
}

#[derive(Clone)]
struct EnumTypeInfo {
    type_params: Vec<TypeParam>,
    variants: HashMap<String, Vec<(String, String)>>,
}

#[derive(Clone)]
struct TraitTypeInfo {
    methods: HashMap<String, (Vec<Option<String>>, Option<String>)>,
}

#[derive(Clone, Default)]
struct ClassTypeInfo {
    implements: Vec<String>,
    methods: HashMap<String, (Vec<Option<String>>, Option<String>)>,
}

#[derive(Clone)]
struct StructTypeInfo {
    type_params: Vec<TypeParam>,
    fields: Vec<(String, String)>,
}

fn type_check_program(
    program: &[Stmt],
    path: &str,
    strict: bool,
    context: Option<ModuleCheckContext>,
) -> Vec<ToolingDiagnostic> {
    let mut checker = TypeChecker {
        path: path.to_string(),
        context,
        typed_fns: HashMap::new(),
        fn_type_params: HashMap::new(),
        enum_types: HashMap::new(),
        trait_types: HashMap::new(),
        class_types: HashMap::new(),
        struct_types: HashMap::new(),
        type_env: HashMap::new(),
        annotated_env: HashMap::new(),
        symbol_scopes: Vec::new(),
        current_type_params: HashMap::new(),
        current_return_type: None,
        current_line: 1,
        diagnostics: Vec::new(),
    };
    checker.collect_type_metadata(program);
    checker.symbol_scopes.push(checker.collect_scope(program, true));
    if strict {
        checker.check_annotation_coverage(program);
    }
    checker.check_stmts(program, 0);
    checker.validate_trait_impls();
    checker.diagnostics
}

struct TypeChecker {
    path: String,
    context: Option<ModuleCheckContext>,
    // fn name → (param types per position, return type)
    typed_fns: HashMap<String, (Vec<Option<String>>, Option<String>)>,
    fn_type_params: HashMap<String, Vec<TypeParam>>,
    enum_types: HashMap<String, EnumTypeInfo>,
    trait_types: HashMap<String, TraitTypeInfo>,
    class_types: HashMap<String, ClassTypeInfo>,
    struct_types: HashMap<String, StructTypeInfo>,
    // Variable → currently-known type (inferred or annotated).
    type_env: HashMap<String, String>,
    // Variable → explicit declared/annotated type.
    annotated_env: HashMap<String, String>,
    symbol_scopes: Vec<SymbolScope>,
    current_type_params: HashMap<String, TypeParam>,
    current_return_type: Option<String>,
    current_line: usize,
    diagnostics: Vec<ToolingDiagnostic>,
}

impl TypeChecker {
    fn collect_type_metadata(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            match module_exports::stmt_unwrap_visibility(stmt).0 {
                Stmt::FnDef {
                    name,
                    type_params,
                    params,
                    return_type,
                    ..
                } => {
                    let param_types: Vec<Option<String>> = params.iter().map(|p| p.type_name.clone()).collect();
                    let has_types = param_types.iter().any(|t| t.is_some()) || return_type.is_some();
                    if has_types {
                        self.typed_fns.insert(name.clone(), (param_types, return_type.clone()));
                    }
                    self.fn_type_params.insert(name.clone(), type_params.clone());
                }
                Stmt::Class {
                    name, implements, body, ..
                } => {
                    let mut methods = HashMap::new();
                    for stmt in body {
                        if let Stmt::FnDef {
                            name,
                            params,
                            return_type,
                            ..
                        } = module_exports::stmt_unwrap_visibility(stmt).0
                        {
                            methods.insert(
                                name.clone(),
                                (
                                    params.iter().map(|param| param.type_name.clone()).collect(),
                                    return_type.clone(),
                                ),
                            );
                        }
                    }
                    self.class_types.insert(
                        name.clone(),
                        ClassTypeInfo {
                            implements: implements.clone(),
                            methods,
                        },
                    );
                    self.collect_type_metadata(body);
                }
                Stmt::Struct {
                    name,
                    type_params,
                    fields,
                    ..
                }
                | Stmt::Union {
                    name,
                    type_params,
                    fields,
                } => {
                    self.struct_types.insert(
                        name.clone(),
                        StructTypeInfo {
                            type_params: type_params.clone(),
                            fields: fields.clone(),
                        },
                    );
                }
                Stmt::Enum {
                    name,
                    type_params,
                    variants,
                } => {
                    self.enum_types.insert(
                        name.clone(),
                        EnumTypeInfo {
                            type_params: type_params.clone(),
                            variants: variants
                                .iter()
                                .map(|variant| (variant.name.clone(), variant.fields.clone()))
                                .collect(),
                        },
                    );
                }
                Stmt::Trait { name, methods, .. } => {
                    self.trait_types.insert(
                        name.clone(),
                        TraitTypeInfo {
                            methods: methods.iter().map(|method| trait_method_signature(method)).collect(),
                        },
                    );
                }
                _ => {}
            }
        }
    }

    fn check_annotation_coverage(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            match module_exports::stmt_unwrap_visibility(stmt).0 {
                Stmt::SetLine(n) => self.current_line = *n,
                Stmt::FnDef {
                    name,
                    params,
                    return_type,
                    ..
                } => {
                    if name.starts_with("__") && name.ends_with("__") {
                        continue;
                    }
                    for param in params {
                        if param.is_vararg || param.is_kwarg || param.name == "self" {
                            continue;
                        }
                        if param.type_name.is_none() {
                            self.emit(
                                "unannotated_param",
                                &format!(
                                    "strict: parameter '{}' of '{}' has no type annotation",
                                    param.name, name
                                ),
                            );
                        }
                    }
                    if return_type.is_none() {
                        self.emit(
                            "unannotated_return",
                            &format!("strict: function '{}' has no return type annotation", name),
                        );
                    }
                }
                _ => {}
            }
        }
    }

    fn check_stmts(&mut self, stmts: &[Stmt], nesting: usize) {
        for stmt in stmts {
            self.check_stmt(stmt, nesting);
        }
    }

    fn check_stmt(&mut self, raw_stmt: &Stmt, nesting: usize) {
        if let Stmt::Visibility { stmt, .. } = raw_stmt {
            if nesting > 0 {
                self.emit(
                    "invalid_visibility",
                    "public/private visibility is only valid at module scope",
                );
            }
            self.check_stmt(stmt, nesting);
            return;
        }

        match raw_stmt {
            Stmt::SetLine(n) => self.current_line = *n,

            Stmt::FnDef {
                name,
                params,
                body,
                return_type,
                ..
            } => {
                for param in params {
                    if let Some(default) = &param.default {
                        self.check_expr(default);
                    }
                }

                let saved_ret = self.current_return_type.clone();
                let saved_env = self.type_env.clone();
                let saved_annotated = self.annotated_env.clone();
                let saved_type_params = self.current_type_params.clone();
                let mut scope = self.collect_scope(body, false);
                for param in params {
                    scope.names.insert(param.name.clone());
                    if let Some(type_name) = &param.type_name {
                        self.type_env.insert(param.name.clone(), type_name.clone());
                        self.annotated_env.insert(param.name.clone(), type_name.clone());
                    }
                }
                self.symbol_scopes.push(scope);
                self.current_type_params = self
                    .fn_type_params
                    .get(name)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|param| (param.name.clone(), param))
                    .collect();
                self.current_return_type = if self.typed_fns.contains_key(name) {
                    return_type.clone()
                } else {
                    None
                };
                self.check_stmts(body, nesting + 1);
                if let Some(ret_type) = &return_type {
                    if ret_type != "void" && !self.block_guarantees_return(body) {
                        self.emit(
                            "missing_return",
                            &format!("function '{}' may exit without returning '{}'", name, ret_type),
                        );
                    }
                }
                self.current_return_type = saved_ret;
                self.type_env = saved_env;
                self.annotated_env = saved_annotated;
                self.current_type_params = saved_type_params;
                self.symbol_scopes.pop();
            }

            Stmt::Class { body, .. } => {
                let saved_env = self.type_env.clone();
                let saved_annotated = self.annotated_env.clone();
                self.symbol_scopes.push(self.collect_scope(body, false));
                self.check_stmts(body, nesting + 1);
                self.symbol_scopes.pop();
                self.type_env = saved_env;
                self.annotated_env = saved_annotated;
            }

            Stmt::Match { value, arms } => {
                self.check_expr(value);
                self.check_match(value, arms, nesting);
            }

            Stmt::Return(opt_expr) => {
                if let Some(ret_type) = &self.current_return_type.clone() {
                    match opt_expr {
                        Some(expr) => {
                            self.check_expr(expr);
                            if let Some(actual) = self.infer_type(expr) {
                                if !self.types_compatible_name(&actual, ret_type) {
                                    let msg = type_mismatch_msg(&normalize_to_kind(&actual), ret_type)
                                        .unwrap_or_else(|| format!("expected '{}', got '{}'", ret_type, actual));
                                    self.emit("type_error", &format!("return type mismatch: {msg}"));
                                }
                            }
                        }
                        None if ret_type != "void" => {
                            self.emit(
                                "type_error",
                                &format!("return type mismatch: expected '{}', got nil", ret_type),
                            );
                        }
                        None => {}
                    }
                } else if let Some(expr) = opt_expr {
                    self.check_expr(expr);
                }
            }

            Stmt::Expr(e) => self.check_expr(e),

            Stmt::Assign { name, value } => {
                self.check_expr(value);
                if self.is_const_binding(name) {
                    self.emit(
                        "immutable_reassign",
                        &format!("cannot reassign immutable binding '{}'", name),
                    );
                }
                if let Some(expected) = self.annotated_env.get(name).cloned() {
                    if let Some(actual) = self.infer_type(value) {
                        if !self.types_compatible_name(&actual, &expected) {
                            let msg = type_mismatch_msg(&normalize_to_kind(&actual), &expected)
                                .unwrap_or_else(|| format!("expected '{}', got '{}'", expected, actual));
                            self.emit("type_error", &format!("assignment to '{}': {msg}", name));
                        }
                    }
                    self.type_env.insert(name.clone(), expected);
                } else if let Some(inferred) = self.infer_type(value) {
                    self.type_env.insert(name.clone(), inferred);
                }
            }

            Stmt::VarDecl {
                name, type_name, value, ..
            } => {
                self.check_expr(value);
                if let Some(expected) = type_name {
                    if let Some(actual) = self.infer_type(value) {
                        if !self.types_compatible_name(&actual, expected) {
                            let msg = type_mismatch_msg(&normalize_to_kind(&actual), expected)
                                .unwrap_or_else(|| format!("expected '{}', got '{}'", expected, actual));
                            self.emit("type_error", &format!("binding '{}': {msg}", name));
                        }
                    }
                    self.type_env.insert(name.clone(), expected.clone());
                    self.annotated_env.insert(name.clone(), expected.clone());
                } else if let Some(inferred) = self.infer_type(value) {
                    self.type_env.insert(name.clone(), inferred);
                }
            }

            Stmt::AugAssign { name, value, .. } => {
                if self.is_const_binding(name) {
                    self.emit(
                        "immutable_reassign",
                        &format!("cannot reassign immutable binding '{}'", name),
                    );
                }
                if !self.lookup_symbol(name) {
                    self.emit("undefined_symbol", &format!("unknown symbol '{}'", name));
                }
                self.check_expr(value);
            }

            Stmt::SetItem { object, index, value } => {
                self.check_expr(object);
                self.check_expr(index);
                self.check_expr(value);
            }
            Stmt::SetAttr { object, value, .. } => {
                self.check_expr(object);
                self.check_expr(value);
            }
            Stmt::Unpack { names, value } => {
                self.check_expr(value);
                for name in names {
                    if self.is_const_binding(name) {
                        self.emit(
                            "immutable_reassign",
                            &format!("cannot reassign immutable binding '{}'", name),
                        );
                    }
                }
            }
            Stmt::UnpackTargets { targets, value } => {
                for target in targets {
                    if let Expr::Ident(name) = target {
                        if self.is_const_binding(name) {
                            self.emit(
                                "immutable_reassign",
                                &format!("cannot reassign immutable binding '{}'", name),
                            );
                        }
                    } else {
                        self.check_expr(target);
                    }
                }
                self.check_expr(value);
            }

            Stmt::If {
                condition,
                then_body,
                elif_clauses,
                else_body,
            } => {
                self.check_expr(condition);
                self.check_stmts(then_body, nesting);
                for (cond, blk) in elif_clauses {
                    self.check_expr(cond);
                    self.check_stmts(blk, nesting);
                }
                if let Some(blk) = else_body {
                    self.check_stmts(blk, nesting);
                }
            }
            Stmt::While { condition, body } => {
                self.check_expr(condition);
                self.check_stmts(body, nesting);
            }
            Stmt::For { var, iter, body } => {
                self.check_expr(iter);
                if !self.lookup_symbol(var) {
                    if let Some(scope) = self.symbol_scopes.last_mut() {
                        scope.names.insert(var.clone());
                    }
                }
                self.check_stmts(body, nesting);
            }
            Stmt::Try {
                body,
                handlers,
                else_body,
                finally_body,
            } => {
                self.check_stmts(body, nesting);
                for h in handlers {
                    self.check_stmts(&h.body, nesting);
                }
                if let Some(b) = else_body {
                    self.check_stmts(b, nesting);
                }
                if let Some(b) = finally_body {
                    self.check_stmts(b, nesting);
                }
            }
            Stmt::With { expr, body, .. } => {
                self.check_expr(expr);
                self.check_stmts(body, nesting);
            }
            Stmt::Raise(Some(e)) => self.check_expr(e),
            Stmt::Enum { .. } | Stmt::Trait { .. } => {}
            _ => {}
        }
    }

    fn check_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Ident(name) => {
                if !self.lookup_symbol(name) {
                    self.emit("undefined_symbol", &format!("unknown symbol '{}'", name));
                }
            }
            Expr::Call { callee, args, kwargs } => {
                if let Expr::Ident(fn_name) = callee.as_ref() {
                    if let Some((param_types, _)) = self.typed_fns.get(fn_name).cloned() {
                        let fn_type_params = self.fn_type_params.get(fn_name).cloned().unwrap_or_default();
                        let mut bindings = HashMap::new();
                        for (i, (arg, param_type)) in args.iter().zip(param_types.iter()).enumerate() {
                            if let Some(type_name) = param_type {
                                if let Some(actual) = self.infer_type(arg) {
                                    if !self.bind_expected_type(&actual, &type_name, &fn_type_params, &mut bindings) {
                                        let msg = type_mismatch_msg(&normalize_to_kind(&actual), &type_name)
                                            .unwrap_or_else(|| format!("expected '{}', got '{}'", type_name, actual));
                                        self.emit("type_error", &format!("argument {} to '{}': {msg}", i + 1, fn_name));
                                    }
                                }
                            }
                        }
                        self.check_bound_bindings(fn_name, &fn_type_params, &bindings);
                    }
                    if let Some(struct_info) = self.struct_types.get(fn_name) {
                        if struct_info.fields.len() != args.len() {
                            self.emit(
                                "type_error",
                                &format!(
                                    "constructor '{}' expects {} argument(s), got {}",
                                    fn_name,
                                    struct_info.fields.len(),
                                    args.len()
                                ),
                            );
                        }
                    }
                } else if let Expr::Attr { object, name } = callee.as_ref() {
                    if let Expr::Ident(enum_name) = object.as_ref() {
                        if let Some(enum_info) = self.enum_types.get(enum_name) {
                            if let Some(fields) = enum_info.variants.get(name) {
                                if fields.len() != args.len() {
                                    self.emit(
                                        "type_error",
                                        &format!(
                                            "variant '{}.{}' expects {} argument(s), got {}",
                                            enum_name,
                                            name,
                                            fields.len(),
                                            args.len()
                                        ),
                                    );
                                }
                            }
                        }
                    }
                }
                self.check_expr(callee);
                for a in args {
                    self.check_expr(a);
                }
                for (_, v) in kwargs {
                    self.check_expr(v);
                }
            }
            Expr::BinOp { left, right, .. } => {
                self.check_expr(left);
                self.check_expr(right);
            }
            Expr::UnaryOp { expr, .. } => self.check_expr(expr),
            Expr::Index { object, index } => {
                self.check_expr(object);
                self.check_expr(index);
            }
            Expr::Slice { object, start, stop } => {
                self.check_expr(object);
                if let Some(e) = start {
                    self.check_expr(e);
                }
                if let Some(e) = stop {
                    self.check_expr(e);
                }
            }
            Expr::Attr { object, name } => {
                if let Expr::Ident(binding) = object.as_ref() {
                    if let Some(module_path) = self.lookup_module_binding(binding) {
                        let exported = self
                            .context
                            .as_ref()
                            .and_then(|ctx| ctx.exports_by_path.get(module_path));
                        if let Some(exports) = exported {
                            if !exports.contains(name) {
                                self.emit(
                                    "import_validation",
                                    &format!("module '{}' does not export '{}'", binding, name),
                                );
                            }
                        }
                    }
                }
                self.check_expr(object);
            }
            Expr::List(items) | Expr::Tuple(items) => {
                for e in items {
                    self.check_expr(e);
                }
            }
            Expr::Dict(pairs) => {
                for (k, v) in pairs {
                    self.check_expr(k);
                    self.check_expr(v);
                }
            }
            Expr::FString(parts) => {
                for part in parts {
                    if let crate::ast::FStringPart::Expr(expr) = part {
                        self.check_expr(expr);
                    }
                }
            }
            Expr::Lambda { params, body } => {
                let saved_env = self.type_env.clone();
                let saved_annotated = self.annotated_env.clone();
                let mut scope = SymbolScope::default();
                for param in params {
                    scope.names.insert(param.name.clone());
                    if let Some(type_name) = &param.type_name {
                        self.type_env.insert(param.name.clone(), type_name.clone());
                        self.annotated_env.insert(param.name.clone(), type_name.clone());
                    }
                }
                self.symbol_scopes.push(scope);
                self.check_expr(body);
                self.symbol_scopes.pop();
                self.type_env = saved_env;
                self.annotated_env = saved_annotated;
            }
            Expr::Ternary {
                condition,
                then_expr,
                else_expr,
            } => {
                self.check_expr(condition);
                self.check_expr(then_expr);
                self.check_expr(else_expr);
            }
            Expr::ListComp {
                expr,
                var,
                iter,
                condition,
            } => {
                self.check_expr(iter);
                let mut scope = SymbolScope::default();
                scope.names.insert(var.clone());
                self.symbol_scopes.push(scope);
                self.check_expr(expr);
                if let Some(c) = condition {
                    self.check_expr(c);
                }
                self.symbol_scopes.pop();
            }
            _ => {}
        }
    }

    fn infer_type(&self, expr: &Expr) -> Option<String> {
        match expr {
            Expr::Int(_) => Some("int".to_string()),
            Expr::Float(_) => Some("float".to_string()),
            Expr::Str(_) => Some("str".to_string()),
            Expr::Bool(_) => Some("bool".to_string()),
            Expr::Nil => Some("nil".to_string()),
            Expr::Ident(name) => self.type_env.get(name).cloned(),
            Expr::List(items) => {
                let first = self.infer_type(items.first()?)?;
                if items
                    .iter()
                    .skip(1)
                    .all(|item| self.infer_type(item).as_deref() == Some(first.as_str()))
                {
                    Some(format!("list[{first}]"))
                } else {
                    Some("list".to_string())
                }
            }
            Expr::Tuple(items) => Some(format!(
                "tuple[{}]",
                items
                    .iter()
                    .map(|item| self.infer_type(item).unwrap_or_else(|| "any".to_string()))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
            Expr::Dict(items) => {
                if items.is_empty() {
                    return Some("dict".to_string());
                }
                let key = self.infer_type(&items[0].0)?;
                let value = self.infer_type(&items[0].1)?;
                if items.iter().skip(1).all(|(k, v)| {
                    self.infer_type(k).as_deref() == Some(key.as_str())
                        && self.infer_type(v).as_deref() == Some(value.as_str())
                }) {
                    Some(format!("dict[{key}, {value}]"))
                } else {
                    Some("dict".to_string())
                }
            }
            Expr::Call { callee, args, .. } => {
                if let Expr::Ident(fn_name) = callee.as_ref() {
                    if let Some((_, Some(ret))) = self.typed_fns.get(fn_name) {
                        let fn_type_params = self.fn_type_params.get(fn_name).cloned().unwrap_or_default();
                        let mut bindings = HashMap::new();
                        if let Some((param_types, _)) = self.typed_fns.get(fn_name) {
                            for (arg, expected) in args.iter().zip(param_types.iter()) {
                                if let Some(expected) = expected {
                                    if let Some(actual) = self.infer_type(arg) {
                                        let _ =
                                            self.bind_expected_type(&actual, expected, &fn_type_params, &mut bindings);
                                    }
                                }
                            }
                        }
                        return Some(self.substitute_type_name(ret, &bindings));
                    }
                    if let Some(struct_info) = self.struct_types.get(fn_name) {
                        return Some(self.infer_struct_constructor_type(fn_name, struct_info, args));
                    }
                    if self.class_types.contains_key(fn_name) {
                        return Some(fn_name.clone());
                    }
                } else if let Expr::Attr { object, name } = callee.as_ref() {
                    if let Expr::Ident(enum_name) = object.as_ref() {
                        if let Some(inferred) = self.infer_enum_variant_type(enum_name, name, args) {
                            return Some(inferred);
                        }
                    }
                }
                None
            }
            Expr::Attr { object, name } => {
                if let Expr::Ident(enum_name) = object.as_ref() {
                    if let Some(enum_info) = self.enum_types.get(enum_name) {
                        if let Some(fields) = enum_info.variants.get(name) {
                            if fields.is_empty() {
                                return Some(self.instantiate_named_type(
                                    enum_name,
                                    &enum_info.type_params,
                                    &HashMap::new(),
                                ));
                            }
                        }
                    }
                }
                None
            }
            Expr::Ternary {
                then_expr, else_expr, ..
            } => {
                let then_ty = self.infer_type(then_expr)?;
                let else_ty = self.infer_type(else_expr)?;
                if self.types_compatible_name(&then_ty, &else_ty) {
                    Some(then_ty)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn types_compatible_name(&self, actual: &str, expected: &str) -> bool {
        let actual = parse_type_expr(actual);
        let expected = parse_type_expr(expected);
        let mut bindings = HashMap::new();
        let params = self.current_type_params.values().cloned().collect::<Vec<_>>();
        type_matches(&actual, &expected, &params, &mut bindings)
    }

    fn bind_expected_type(
        &self,
        actual: &str,
        expected: &str,
        type_params: &[TypeParam],
        bindings: &mut HashMap<String, TypeExpr>,
    ) -> bool {
        type_matches(
            &parse_type_expr(actual),
            &parse_type_expr(expected),
            type_params,
            bindings,
        )
    }

    fn substitute_type_name(&self, type_name: &str, bindings: &HashMap<String, TypeExpr>) -> String {
        substitute_type_expr(&parse_type_expr(type_name), bindings).render()
    }

    fn instantiate_named_type(
        &self,
        name: &str,
        type_params: &[TypeParam],
        bindings: &HashMap<String, TypeExpr>,
    ) -> String {
        if type_params.is_empty() {
            return name.to_string();
        }
        let args = type_params
            .iter()
            .map(|param| {
                bindings
                    .get(&param.name)
                    .cloned()
                    .unwrap_or_else(|| TypeExpr::named(param.name.clone()))
            })
            .collect();
        TypeExpr::Named {
            name: name.to_string(),
            args,
        }
        .render()
    }

    fn infer_struct_constructor_type(&self, name: &str, info: &StructTypeInfo, args: &[Expr]) -> String {
        let mut bindings = HashMap::new();
        for ((_, expected), arg) in info.fields.iter().zip(args.iter()) {
            if let Some(actual) = self.infer_type(arg) {
                let _ = self.bind_expected_type(&actual, expected, &info.type_params, &mut bindings);
            }
        }
        self.instantiate_named_type(name, &info.type_params, &bindings)
    }

    fn infer_enum_variant_type(&self, enum_name: &str, variant_name: &str, args: &[Expr]) -> Option<String> {
        let info = self.enum_types.get(enum_name)?;
        let fields = info.variants.get(variant_name)?;
        let mut bindings = HashMap::new();
        for ((_, expected), arg) in fields.iter().zip(args.iter()) {
            if let Some(actual) = self.infer_type(arg) {
                let _ = self.bind_expected_type(&actual, expected, &info.type_params, &mut bindings);
            }
        }
        Some(self.instantiate_named_type(enum_name, &info.type_params, &bindings))
    }

    fn check_bound_bindings(&mut self, fn_name: &str, type_params: &[TypeParam], bindings: &HashMap<String, TypeExpr>) {
        for param in type_params {
            let Some(bound) = &param.bound else {
                continue;
            };
            let Some(actual) = bindings.get(&param.name) else {
                continue;
            };
            if !self.type_implements_trait(actual, bound) {
                self.emit(
                    "type_error",
                    &format!(
                        "call to '{}': type '{}' does not satisfy bound '{}: {}'",
                        fn_name,
                        actual.render(),
                        param.name,
                        bound
                    ),
                );
            }
        }
    }

    fn type_implements_trait(&self, actual: &TypeExpr, trait_name: &str) -> bool {
        let actual_name = actual.base_name();
        if actual_name == trait_name {
            return true;
        }
        self.class_types
            .get(actual_name)
            .map(|info| {
                info.implements
                    .iter()
                    .any(|implemented| type_name_base(implemented) == trait_name)
            })
            .unwrap_or(false)
    }

    fn check_match(&mut self, value: &Expr, arms: &[MatchArm], nesting: usize) {
        let inferred = self.infer_type(value);
        if let Some(subject_type) = inferred.as_deref() {
            let subject_base = type_name_base(subject_type);
            if let Some(enum_info) = self.enum_types.get(&subject_base) {
                let covered = arms
                    .iter()
                    .any(|arm| matches!(arm.pattern, Pattern::Wildcard | Pattern::Capture(_)))
                    || enum_info
                        .variants
                        .keys()
                        .all(|variant| arms.iter().any(|arm| pattern_covers_variant(&arm.pattern, variant)));
                if !covered {
                    self.emit(
                        "non_exhaustive_match",
                        &format!(
                            "match on '{}' does not cover every variant of '{}'",
                            subject_type, subject_base
                        ),
                    );
                }
            }
        }

        for arm in arms {
            let saved_env = self.type_env.clone();
            let saved_annotated = self.annotated_env.clone();
            let mut scope = SymbolScope::default();
            if let Some(subject_type) = inferred.as_deref() {
                self.bind_pattern_types(&arm.pattern, subject_type, &mut scope);
            }
            self.symbol_scopes.push(scope);
            self.check_stmts(&arm.body, nesting + 1);
            self.symbol_scopes.pop();
            self.type_env = saved_env;
            self.annotated_env = saved_annotated;
        }
    }

    fn bind_pattern_types(&mut self, pattern: &Pattern, subject_type: &str, scope: &mut SymbolScope) {
        match pattern {
            Pattern::Capture(name) => {
                scope.names.insert(name.clone());
                self.type_env.insert(name.clone(), subject_type.to_string());
            }
            Pattern::Variant {
                enum_name,
                variant,
                fields,
            } => {
                let resolved_enum = enum_name.clone().unwrap_or_else(|| type_name_base(subject_type));
                if let Some(field_types) = self.instantiated_variant_field_types(&resolved_enum, subject_type, variant)
                {
                    for (field_pattern, field_type) in fields.iter().zip(field_types.iter()) {
                        self.bind_pattern_types(field_pattern, field_type, scope);
                    }
                } else {
                    for field_pattern in fields {
                        self.bind_pattern_types(field_pattern, "any", scope);
                    }
                }
            }
            _ => {}
        }
    }

    fn instantiated_variant_field_types(
        &self,
        enum_name: &str,
        subject_type: &str,
        variant: &str,
    ) -> Option<Vec<String>> {
        let info = self.enum_types.get(enum_name)?;
        let fields = info.variants.get(variant)?;
        let subject = parse_type_expr(subject_type);
        let mut bindings = HashMap::new();
        if let TypeExpr::Named { args, .. } = subject {
            for (param, arg) in info.type_params.iter().zip(args.iter()) {
                bindings.insert(param.name.clone(), arg.clone());
            }
        }
        Some(
            fields
                .iter()
                .map(|(_, type_name)| substitute_type_expr(&parse_type_expr(type_name), &bindings).render())
                .collect(),
        )
    }

    fn validate_trait_impls(&mut self) {
        for (class_name, class_info) in self.class_types.clone() {
            for trait_name in class_info.implements {
                let trait_base = type_name_base(&trait_name);
                let Some(trait_info) = self.trait_types.get(&trait_base).cloned() else {
                    self.emit(
                        "type_error",
                        &format!("class '{}' implements unknown trait '{}'", class_name, trait_name),
                    );
                    continue;
                };
                for (method_name, expected_sig) in &trait_info.methods {
                    let Some(actual_sig) = class_info.methods.get(method_name) else {
                        self.emit(
                            "type_error",
                            &format!(
                                "class '{}' does not implement required trait method '{}.{}'",
                                class_name, trait_name, method_name
                            ),
                        );
                        continue;
                    };
                    if actual_sig.0.len() != expected_sig.0.len() {
                        self.emit(
                            "type_error",
                            &format!(
                                "class '{}' method '{}' does not match trait '{}' arity",
                                class_name, method_name, trait_name
                            ),
                        );
                        continue;
                    }
                    for (actual, expected) in actual_sig.0.iter().zip(expected_sig.0.iter()) {
                        if let (Some(actual), Some(expected)) = (actual, expected) {
                            if !self.types_compatible_name(actual, expected) {
                                self.emit(
                                    "type_error",
                                    &format!(
                                        "class '{}' method '{}' parameter type '{}' does not satisfy trait '{}'",
                                        class_name, method_name, actual, trait_name
                                    ),
                                );
                            }
                        }
                    }
                    if let (Some(actual), Some(expected)) = (&actual_sig.1, &expected_sig.1) {
                        if !self.types_compatible_name(actual, expected) {
                            self.emit(
                                "type_error",
                                &format!(
                                    "class '{}' method '{}' return type '{}' does not satisfy trait '{}'",
                                    class_name, method_name, actual, trait_name
                                ),
                            );
                        }
                    }
                }
            }
        }
    }

    fn collect_scope(&self, stmts: &[Stmt], include_builtins: bool) -> SymbolScope {
        let mut scope = SymbolScope::default();
        if include_builtins {
            for builtin in BUILTIN_NAMES {
                scope.names.insert((*builtin).to_string());
            }
        }
        self.collect_scope_directives(stmts, &mut scope);
        let mut current_line = 1usize;
        self.collect_scope_bindings(stmts, &mut scope, &mut current_line);
        scope
    }

    fn collect_scope_directives(&self, stmts: &[Stmt], scope: &mut SymbolScope) {
        for raw_stmt in stmts {
            let stmt = module_exports::stmt_unwrap_visibility(raw_stmt).0;
            match stmt {
                Stmt::Global(names) => {
                    for name in names {
                        scope.globals_declared.insert(name.clone());
                    }
                }
                Stmt::Nonlocal(names) => {
                    for name in names {
                        scope.nonlocals_declared.insert(name.clone());
                    }
                }
                Stmt::If {
                    then_body,
                    elif_clauses,
                    else_body,
                    ..
                } => {
                    self.collect_scope_directives(then_body, scope);
                    for (_, body) in elif_clauses {
                        self.collect_scope_directives(body, scope);
                    }
                    if let Some(body) = else_body {
                        self.collect_scope_directives(body, scope);
                    }
                }
                Stmt::While { body, .. } | Stmt::For { body, .. } | Stmt::With { body, .. } => {
                    self.collect_scope_directives(body, scope);
                }
                Stmt::Try {
                    body,
                    handlers,
                    else_body,
                    finally_body,
                } => {
                    self.collect_scope_directives(body, scope);
                    for handler in handlers {
                        self.collect_scope_directives(&handler.body, scope);
                    }
                    if let Some(body) = else_body {
                        self.collect_scope_directives(body, scope);
                    }
                    if let Some(body) = finally_body {
                        self.collect_scope_directives(body, scope);
                    }
                }
                _ => {}
            }
        }
    }

    fn collect_scope_bindings(&self, stmts: &[Stmt], scope: &mut SymbolScope, current_line: &mut usize) {
        for raw_stmt in stmts {
            let stmt = module_exports::stmt_unwrap_visibility(raw_stmt).0;
            match stmt {
                Stmt::SetLine(line) => *current_line = *line,
                Stmt::Assign { name, .. } => self.add_scope_binding(scope, name, false),
                Stmt::VarDecl { name, is_const, .. } => self.add_scope_binding(scope, name, *is_const),
                Stmt::FnDef { name, .. }
                | Stmt::ExternFn { name, .. }
                | Stmt::Data { name, .. }
                | Stmt::Class { name, .. }
                | Stmt::Struct { name, .. }
                | Stmt::Union { name, .. } => self.add_scope_binding(scope, name, false),
                Stmt::Unpack { names, .. } => {
                    for name in names {
                        self.add_scope_binding(scope, name, false);
                    }
                }
                Stmt::For { var, body, .. } => {
                    self.add_scope_binding(scope, var, false);
                    self.collect_scope_bindings(body, scope, current_line);
                }
                Stmt::With { as_name, body, .. } => {
                    if let Some(name) = as_name {
                        self.add_scope_binding(scope, name, false);
                    }
                    self.collect_scope_bindings(body, scope, current_line);
                }
                Stmt::If {
                    then_body,
                    elif_clauses,
                    else_body,
                    ..
                } => {
                    self.collect_scope_bindings(then_body, scope, current_line);
                    for (_, body) in elif_clauses {
                        self.collect_scope_bindings(body, scope, current_line);
                    }
                    if let Some(body) = else_body {
                        self.collect_scope_bindings(body, scope, current_line);
                    }
                }
                Stmt::While { body, .. } => self.collect_scope_bindings(body, scope, current_line),
                Stmt::Try {
                    body,
                    handlers,
                    else_body,
                    finally_body,
                } => {
                    self.collect_scope_bindings(body, scope, current_line);
                    for handler in handlers {
                        if let Some(name) = &handler.as_name {
                            self.add_scope_binding(scope, name, false);
                        }
                        self.collect_scope_bindings(&handler.body, scope, current_line);
                    }
                    if let Some(body) = else_body {
                        self.collect_scope_bindings(body, scope, current_line);
                    }
                    if let Some(body) = finally_body {
                        self.collect_scope_bindings(body, scope, current_line);
                    }
                }
                Stmt::Import(specifier) => {
                    if let Some(import) = self.find_import(*current_line, ModuleImportKind::File, specifier.as_str()) {
                        if let Some(resolved) = &import.resolved {
                            if let Some(exports) =
                                self.context.as_ref().and_then(|ctx| ctx.exports_by_path.get(resolved))
                            {
                                for name in exports {
                                    self.add_scope_binding(scope, name, false);
                                }
                            }
                        }
                    }
                }
                Stmt::ImportModule(name) => {
                    let binding = import_binding_name(name);
                    self.add_scope_binding(scope, &binding, false);
                    if let Some(import) = self.find_import(*current_line, ModuleImportKind::Module, name.as_str()) {
                        if let Some(resolved) = &import.resolved {
                            scope.module_bindings.insert(binding.clone(), resolved.clone());
                        }
                    } else if self
                        .find_import(*current_line, ModuleImportKind::Builtin, name.as_str())
                        .is_some()
                    {
                        scope.builtin_module_bindings.insert(binding);
                    }
                }
                _ => {}
            }
        }
    }

    fn add_scope_binding(&self, scope: &mut SymbolScope, name: &str, is_const: bool) {
        if scope.globals_declared.contains(name) || scope.nonlocals_declared.contains(name) {
            return;
        }
        scope.names.insert(name.to_string());
        if is_const {
            scope.consts.insert(name.to_string());
        }
    }

    fn lookup_symbol(&self, name: &str) -> bool {
        for scope in self.symbol_scopes.iter().rev() {
            if scope.globals_declared.contains(name) || scope.nonlocals_declared.contains(name) {
                continue;
            }
            if scope.names.contains(name) {
                return true;
            }
        }
        false
    }

    fn is_const_binding(&self, name: &str) -> bool {
        for scope in self.symbol_scopes.iter().rev() {
            if scope.globals_declared.contains(name) || scope.nonlocals_declared.contains(name) {
                continue;
            }
            if scope.names.contains(name) {
                return scope.consts.contains(name);
            }
        }
        false
    }

    fn lookup_module_binding(&self, name: &str) -> Option<&str> {
        for scope in self.symbol_scopes.iter().rev() {
            if scope.globals_declared.contains(name) || scope.nonlocals_declared.contains(name) {
                continue;
            }
            if let Some(path) = scope.module_bindings.get(name) {
                return Some(path);
            }
            if scope.names.contains(name) {
                return None;
            }
        }
        None
    }

    fn find_import(&self, line: usize, kind: ModuleImportKind, specifier: &str) -> Option<&ModuleGraphImport> {
        self.context
            .as_ref()?
            .imports
            .iter()
            .find(|import| import.line == Some(line) && import.kind == kind && import.specifier == specifier)
    }

    fn block_guarantees_return(&self, stmts: &[Stmt]) -> bool {
        for stmt in stmts {
            if self.stmt_guarantees_return(stmt) {
                return true;
            }
        }
        false
    }

    fn stmt_guarantees_return(&self, raw_stmt: &Stmt) -> bool {
        let stmt = module_exports::stmt_unwrap_visibility(raw_stmt).0;
        match stmt {
            Stmt::Return(_) | Stmt::Raise(_) => true,
            Stmt::If {
                then_body,
                elif_clauses,
                else_body,
                ..
            } => {
                let Some(else_body) = else_body else {
                    return false;
                };
                self.block_guarantees_return(then_body)
                    && elif_clauses.iter().all(|(_, body)| self.block_guarantees_return(body))
                    && self.block_guarantees_return(else_body)
            }
            Stmt::While { condition, body } => {
                matches!(condition, Expr::Bool(true)) && self.block_guarantees_return(body)
            }
            Stmt::Try {
                body,
                handlers,
                else_body,
                finally_body,
            } => {
                if let Some(finally_body) = finally_body {
                    if self.block_guarantees_return(finally_body) {
                        return true;
                    }
                }
                self.block_guarantees_return(body)
                    && handlers
                        .iter()
                        .all(|handler| self.block_guarantees_return(&handler.body))
                    && else_body
                        .as_ref()
                        .map(|body| self.block_guarantees_return(body))
                        .unwrap_or(true)
            }
            _ => false,
        }
    }

    fn emit(&mut self, code: &'static str, message: &str) {
        self.diagnostics.push(ToolingDiagnostic {
            severity: DiagnosticSeverity::Error,
            code,
            path: self.path.clone(),
            line: Some(self.current_line),
            message: message.to_string(),
        });
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum TypeExpr {
    Named { name: String, args: Vec<TypeExpr> },
    Union(Vec<TypeExpr>),
}

impl TypeExpr {
    fn named(name: impl Into<String>) -> Self {
        Self::Named {
            name: name.into(),
            args: Vec::new(),
        }
    }

    fn render(&self) -> String {
        match self {
            Self::Named { name, args } if args.is_empty() => name.clone(),
            Self::Named { name, args } => format!(
                "{}[{}]",
                name,
                args.iter().map(TypeExpr::render).collect::<Vec<_>>().join(", ")
            ),
            Self::Union(items) => items.iter().map(TypeExpr::render).collect::<Vec<_>>().join(" | "),
        }
    }

    fn base_name(&self) -> &str {
        match self {
            Self::Named { name, .. } => name,
            Self::Union(items) => items.first().map(TypeExpr::base_name).unwrap_or("any"),
        }
    }
}

fn trait_method_signature(method: &TraitMethod) -> (String, (Vec<Option<String>>, Option<String>)) {
    (
        method.name.clone(),
        (
            method.params.iter().map(|param| param.type_name.clone()).collect(),
            method.return_type.clone(),
        ),
    )
}

fn pattern_covers_variant(pattern: &Pattern, variant: &str) -> bool {
    match pattern {
        Pattern::Variant { variant: name, .. } => name == variant,
        Pattern::Wildcard | Pattern::Capture(_) => true,
        _ => false,
    }
}

fn type_name_base(type_name: &str) -> String {
    parse_type_expr(type_name).base_name().to_string()
}

fn parse_type_expr(text: &str) -> TypeExpr {
    let text = text.trim();
    let union_parts = split_top_level(text, '|');
    if union_parts.len() > 1 {
        return TypeExpr::Union(union_parts.into_iter().map(|part| parse_type_expr(&part)).collect());
    }
    let Some(bracket_start) = find_top_level_char(text, '[') else {
        return TypeExpr::named(text);
    };
    if !text.ends_with(']') {
        return TypeExpr::named(text);
    }
    let name = text[..bracket_start].trim().to_string();
    let inner = &text[bracket_start + 1..text.len() - 1];
    let args = split_top_level(inner, ',')
        .into_iter()
        .filter(|part| !part.is_empty())
        .map(|part| parse_type_expr(&part))
        .collect();
    TypeExpr::Named { name, args }
}

fn split_top_level(text: &str, separator: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (idx, ch) in text.char_indices() {
        match ch {
            '[' | '(' => depth += 1,
            ']' | ')' => depth -= 1,
            _ if ch == separator && depth == 0 => {
                parts.push(text[start..idx].trim().to_string());
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(text[start..].trim().to_string());
    parts
}

fn find_top_level_char(text: &str, target: char) -> Option<usize> {
    let mut depth = 0i32;
    for (idx, ch) in text.char_indices() {
        match ch {
            '[' | '(' => {
                if ch == target && depth == 0 {
                    return Some(idx);
                }
                depth += 1;
            }
            ']' | ')' => depth -= 1,
            _ if ch == target && depth == 0 => return Some(idx),
            _ => {}
        }
    }
    None
}

fn substitute_type_expr(expr: &TypeExpr, bindings: &HashMap<String, TypeExpr>) -> TypeExpr {
    match expr {
        TypeExpr::Named { name, args } if args.is_empty() => {
            bindings.get(name).cloned().unwrap_or_else(|| expr.clone())
        }
        TypeExpr::Named { name, args } => TypeExpr::Named {
            name: name.clone(),
            args: args.iter().map(|arg| substitute_type_expr(arg, bindings)).collect(),
        },
        TypeExpr::Union(items) => {
            TypeExpr::Union(items.iter().map(|item| substitute_type_expr(item, bindings)).collect())
        }
    }
}

fn type_matches(
    actual: &TypeExpr,
    expected: &TypeExpr,
    type_params: &[TypeParam],
    bindings: &mut HashMap<String, TypeExpr>,
) -> bool {
    match expected {
        TypeExpr::Union(items) => items.iter().any(|item| {
            let mut local = bindings.clone();
            let ok = type_matches(actual, item, type_params, &mut local);
            if ok {
                *bindings = local;
            }
            ok
        }),
        TypeExpr::Named { name, args } => {
            if type_params.iter().any(|param| param.name == *name) && args.is_empty() {
                match bindings.get(name) {
                    Some(bound) => bound == actual,
                    None => {
                        bindings.insert(name.clone(), actual.clone());
                        true
                    }
                }
            } else {
                let actual_name = actual.base_name();
                let expected_name = name.as_str();
                let primitive_ok = primitive_types_compatible(actual_name, expected_name);
                if !primitive_ok && actual_name != expected_name {
                    return false;
                }
                match actual {
                    TypeExpr::Named { args: actual_args, .. } => {
                        if !args.is_empty() && args.len() != actual_args.len() {
                            return false;
                        }
                        args.iter().zip(actual_args.iter()).all(|(expected_arg, actual_arg)| {
                            type_matches(actual_arg, expected_arg, type_params, bindings)
                        })
                    }
                    TypeExpr::Union(_) => false,
                }
            }
        }
    }
}

fn primitive_types_compatible(actual: &str, expected: &str) -> bool {
    if actual == expected {
        return true;
    }
    match normalize_to_kind(actual).as_str() {
        "int" => matches!(
            expected,
            "int"
                | "i8"
                | "i16"
                | "i32"
                | "i64"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "isize"
                | "usize"
                | "float"
                | "f32"
                | "f64"
        ),
        "float" => matches!(expected, "float" | "f32" | "f64"),
        "bool" => matches!(expected, "bool"),
        "str" => matches!(expected, "str"),
        "nil" => matches!(expected, "nil"),
        _ => false,
    }
}

/// Normalize an exact type name or kind string to the coarse kind used by type_mismatch_msg.
fn normalize_to_kind(type_str: &str) -> String {
    match parse_type_expr(type_str).base_name() {
        "i8" | "u8" | "i16" | "u16" | "i32" | "u32" | "i64" | "u64" | "isize" | "usize" | "int" => "int".to_string(),
        "f32" | "f64" | "float" => "float".to_string(),
        "str" => "str".to_string(),
        "bool" => "bool".to_string(),
        "nil" => "nil".to_string(),
        other => other.to_string(),
    }
}

/// Returns Some(error message with fix suggestion) when `actual` kind clearly conflicts with `expected` type.
/// Returns None when the combination is compatible or ambiguous.
fn type_mismatch_msg(actual: &str, expected: &str) -> Option<String> {
    let is_int_type = matches!(
        expected,
        "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "isize" | "usize"
    );
    let is_float_type = matches!(expected, "f32" | "f64");

    match actual {
        "int" => {
            if expected == "str" {
                Some(format!(
                    "expected '{expected}', got integer — use str(value) to convert"
                ))
            } else {
                None // int is compatible with all numeric types (may truncate, but not flagged here)
            }
        }
        "float" => {
            if expected == "str" || expected == "bool" {
                Some(format!("expected '{expected}', got float — use str(value) to convert"))
            } else if is_int_type {
                Some(format!(
                    "expected integer type '{expected}', got float — use int() to truncate (precision may be lost)"
                ))
            } else {
                None // float → f32/f64 is fine
            }
        }
        "str" => {
            if expected == "str" {
                None
            } else if is_int_type {
                Some(format!(
                    "expected integer type '{expected}', got str — use int(value) to convert"
                ))
            } else if is_float_type {
                Some(format!(
                    "expected float type '{expected}', got str — use float(value) to convert"
                ))
            } else if expected == "bool" {
                Some(format!("expected '{expected}', got str — use bool(value) to convert"))
            } else {
                None
            }
        }
        "bool" => {
            if expected == "str" {
                Some(format!("expected '{expected}', got bool — use str(value) to convert"))
            } else {
                None // bool coerces to 0/1 for numeric types
            }
        }
        "nil" => {
            if expected == "str" || is_int_type || is_float_type || expected == "bool" {
                Some(format!("expected '{expected}', got nil — check for a missing value"))
            } else {
                None
            }
        }
        _ => None,
    }
}
