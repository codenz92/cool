mod argparse_runtime;
mod ast;
mod benchmark;
mod bindgen;
mod build_cache;
mod compiler;
mod core_runtime;
mod csv_runtime;
mod datetime_runtime;
mod formatter;
mod hashlib_runtime;
mod http_runtime;
mod interpreter;
mod layout_tool;
mod lexer;
mod llvm_codegen;
mod logging_runtime;
mod lowering;
mod lsp;
mod module_exports;
mod opcode;
mod parser;
mod project;
mod sqlite_runtime;
mod subprocess_runtime;
mod text_runtime;
mod toml_runtime;
mod tooling;
mod vm;
mod yaml_runtime;

use interpreter::Interpreter;
use lexer::Lexer;
use parser::Parser;
use project::{CapabilityPolicy, CoolProject, ModuleResolver};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

// ── Runners ───────────────────────────────────────────────────────────────────

fn run_source(source: &str, source_dir: PathBuf) -> Result<(), String> {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize()?;

    let mut parser = Parser::new(tokens);
    let program = parser.parse_program()?;

    let module_resolver = ModuleResolver::discover_for_script(&source_dir)?;
    let capabilities = capability_policy_for_dir(&source_dir)?;
    let mut interpreter = Interpreter::new(source_dir, source, module_resolver, capabilities);
    interpreter.run(&program)
}

fn run_source_vm(source: &str, source_dir: PathBuf) -> Result<(), String> {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize()?;

    let mut parser = Parser::new(tokens);
    let program = parser.parse_program()?;

    let chunk = compiler::compile(&program)?;
    let module_resolver = ModuleResolver::discover_for_script(&source_dir)?;
    let capabilities = capability_policy_for_dir(&source_dir)?;
    let mut machine = vm::VM::new(source_dir, module_resolver, capabilities);
    machine.run(&chunk)
}

fn compile_to_native(source: &str, output_path: &Path, script_path: &Path) -> Result<(), String> {
    let options = default_native_compile_options(script_path);
    compile_to_native_with_output(source, output_path, script_path, &options)
}

fn compile_to_native_with_output(
    source: &str,
    output_path: &Path,
    script_path: &Path,
    options: &llvm_codegen::NativeCompileOptions,
) -> Result<(), String> {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize()?;

    let mut parser = Parser::new(tokens);
    let program = parser.parse_program()?;

    llvm_codegen::compile_program_with_output(&program, output_path, script_path, options)
}

fn bundled_command_path(file_name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("cmd").join(file_name)
}

fn find_lld(toolchain: &llvm_codegen::NativeToolchainConfig) -> Option<String> {
    if let Some(lld) = toolchain.lld.clone().filter(|value| !value.trim().is_empty()) {
        return Some(lld);
    }
    let candidates = ["ld.lld", "lld", "ld.lld-19", "ld.lld-18", "ld.lld-17", "ld.lld-16"];
    for name in candidates {
        if std::process::Command::new(name)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok()
        {
            return Some(name.to_string());
        }
    }
    None
}

fn link_kernel_image(
    obj_path: &Path,
    script_path: &Path,
    output_path: &Path,
    toolchain: &llvm_codegen::NativeToolchainConfig,
    reproducible: bool,
    entry_symbol: Option<&str>,
) -> Result<(), String> {
    let lld = find_lld(toolchain).ok_or_else(|| {
        "cool build: no LLD linker found — install LLVM/LLD to enable kernel image linking\n  \
         macOS: brew install llvm\n  \
         Debian/Ubuntu: apt install lld"
            .to_string()
    })?;

    let mut command = std::process::Command::new(&lld);
    if reproducible {
        command.env("SOURCE_DATE_EPOCH", "0");
    }
    let status = command
        .arg("-T")
        .arg(script_path)
        .args(
            entry_symbol
                .filter(|value| !value.trim().is_empty())
                .map(|value| vec!["-e", value])
                .unwrap_or_default(),
        )
        .arg("-o")
        .arg(output_path)
        .arg(obj_path)
        .args(if reproducible {
            vec!["--build-id=none"]
        } else {
            Vec::new()
        })
        .status()
        .map_err(|e| format!("cool build: failed to run '{lld}': {e}"))?;

    if !status.success() {
        return Err(format!(
            "cool build: linker '{}' failed (check linker script and entry symbol)",
            lld
        ));
    }
    Ok(())
}

fn task_command_path() -> PathBuf {
    bundled_command_path("task.cool")
}

fn bundle_command_path() -> PathBuf {
    bundled_command_path("bundle.cool")
}

fn release_command_path() -> PathBuf {
    bundled_command_path("release.cool")
}

fn publish_command_path() -> PathBuf {
    bundled_command_path("publish.cool")
}

fn install_command_path() -> PathBuf {
    bundled_command_path("install.cool")
}

fn add_command_path() -> PathBuf {
    bundled_command_path("add.cool")
}

fn pkg_command_path() -> PathBuf {
    bundled_command_path("pkg.cool")
}

fn new_command_path() -> PathBuf {
    bundled_command_path("new.cool")
}

fn run_bundled_app(
    command_name: &str,
    app_path: &Path,
    args: &[&String],
    extra_env: &[(&str, String)],
) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("{command_name}: cannot resolve current executable: {e}"))?;
    if !app_path.exists() {
        return Err(format!(
            "{command_name}: bundled app not found at '{}'",
            app_path.display()
        ));
    }

    let mut cmd = Command::new(&exe);
    cmd.arg(app_path).args(args.iter().map(|arg| arg.as_str()));
    for (key, value) in extra_env {
        cmd.env(key, value);
    }
    cmd.env("COOL_SKIP_PROJECT_CAPABILITIES", "1");

    let status = cmd
        .status()
        .map_err(|e| format!("{command_name}: failed to launch bundled app: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(match status.code() {
            Some(code) => format!("{command_name} failed with exit code {code}"),
            None => format!("{command_name} failed"),
        })
    }
}

fn should_skip_project_capabilities_for(source_dir: &Path) -> bool {
    let source_dir = source_dir.canonicalize().unwrap_or_else(|_| source_dir.to_path_buf());
    let bundled_cmd_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("cmd");
    let bundled_cmd_root = bundled_cmd_root.canonicalize().unwrap_or(bundled_cmd_root);
    match std::env::var("COOL_SKIP_PROJECT_CAPABILITIES") {
        Ok(value) => {
            let trimmed = value.trim();
            !trimmed.is_empty()
                && trimmed != "0"
                && trimmed.to_ascii_lowercase() != "false"
                && source_dir.starts_with(&bundled_cmd_root)
        }
        Err(_) => false,
    }
}

fn capability_policy_for_dir(source_dir: &Path) -> Result<CapabilityPolicy, String> {
    if should_skip_project_capabilities_for(source_dir) {
        return Ok(CapabilityPolicy::allow_all());
    }
    match CoolProject::discover(source_dir)? {
        Some(project) => Ok(project.capabilities),
        None => Ok(CapabilityPolicy::allow_all()),
    }
}

fn current_project(command_name: &str) -> Result<CoolProject, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("{command_name}: cannot read current directory: {e}"))?;
    match CoolProject::discover(&cwd)? {
        Some(project) => Ok(project),
        None => Err(format!(
            "{command_name}: no cool.toml found in this directory or any parent"
        )),
    }
}

fn script_parent_dir(path: &Path) -> PathBuf {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn default_native_compile_options(script_path: &Path) -> llvm_codegen::NativeCompileOptions {
    llvm_codegen::NativeCompileOptions {
        build_mode: llvm_codegen::NativeBuildMode::Hosted,
        artifact_kind: llvm_codegen::NativeArtifactKind::Binary,
        target_triple: None,
        target_cpu: None,
        target_features: None,
        entry_symbol: None,
        debug_info: false,
        reproducible: false,
        no_libc: false,
        capabilities: CapabilityPolicy::allow_all(),
        toolchain: llvm_codegen::NativeToolchainConfig::default(),
        native_links: project::NativeLinkConfig::default(),
        source_root: Some(script_parent_dir(script_path)),
    }
}

fn native_toolchain_config(project: &CoolProject) -> llvm_codegen::NativeToolchainConfig {
    llvm_codegen::NativeToolchainConfig {
        cool: project.toolchain.cool.clone(),
        cc: project.toolchain.cc.clone(),
        ar: project.toolchain.ar.clone(),
        lld: project.toolchain.lld.clone(),
    }
}

fn native_link_config(project: &CoolProject) -> project::NativeLinkConfig {
    let mut config = project.native.clone();
    for path in &mut config.search_paths {
        let candidate = PathBuf::from(path.as_str());
        if candidate.is_relative() {
            *path = project.root.join(candidate).to_string_lossy().into_owned();
        }
    }
    for library in &mut config.libraries {
        let candidate = PathBuf::from(library.name.as_str());
        if candidate.is_relative()
            && (library.name.contains('/')
                || library.name.contains('\\')
                || library.name.ends_with(".a")
                || library.name.ends_with(".so")
                || library.name.ends_with(".dylib")
                || library.name.ends_with(".dll"))
        {
            library.name = project.root.join(candidate).to_string_lossy().into_owned();
        }
    }
    config
}

fn validate_native_toolchain(toolchain: &llvm_codegen::NativeToolchainConfig, context: &str) -> Result<(), String> {
    if let Some(expected) = toolchain.cool.as_deref() {
        let actual = env!("CARGO_PKG_VERSION");
        if expected.trim() != actual {
            return Err(format!(
                "{context}: pinned Cool toolchain version '{expected}' does not match this executable ({actual})"
            ));
        }
    }
    Ok(())
}

fn should_skip_tree_entry(name: &str) -> bool {
    matches!(name, ".git" | ".cool" | "dist" | "target" | "__pycache__")
}

fn collect_tree_files(path: &Path, root: &Path, out: &mut Vec<(PathBuf, PathBuf)>) -> Result<(), String> {
    if path.is_file() {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let relative = canonical.strip_prefix(root).map(Path::to_path_buf).unwrap_or_else(|_| {
            canonical
                .file_name()
                .map(PathBuf::from)
                .unwrap_or_else(|| canonical.clone())
        });
        out.push((canonical, relative));
        return Ok(());
    }

    let mut entries = fs::read_dir(path).map_err(|e| format!("cannot read '{}': {e}", path.display()))?;
    let mut paths = Vec::new();
    while let Some(entry) = entries.next() {
        let entry = entry.map_err(|e| format!("cannot read directory entry in '{}': {e}", path.display()))?;
        let entry_path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if should_skip_tree_entry(&file_name) {
            continue;
        }
        paths.push(entry_path);
    }
    paths.sort();
    for entry_path in paths {
        collect_tree_files(&entry_path, root, out)?;
    }
    Ok(())
}

fn hash_package_path(path: &Path) -> Result<String, String> {
    let root = if path.is_dir() {
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    } else {
        path.parent()
            .unwrap_or(Path::new("."))
            .canonicalize()
            .unwrap_or_else(|_| path.parent().unwrap_or(Path::new(".")).to_path_buf())
    };
    let mut files = Vec::new();
    collect_tree_files(path, &root, &mut files)?;
    files.sort_by(|a, b| a.1.cmp(&b.1));

    let mut hasher = Sha256::new();
    hasher.update(b"cool-package-tree-v1");
    for (absolute, relative) in files {
        hasher.update(relative.to_string_lossy().as_bytes());
        hasher.update([0]);
        let bytes = fs::read(&absolute).map_err(|e| format!("cannot read '{}': {e}", absolute.display()))?;
        hasher.update(bytes.len().to_le_bytes());
        hasher.update(&bytes);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn collect_cool_files(path: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    if path.is_file() {
        if path.extension().and_then(|ext| ext.to_str()) == Some("cool") {
            out.push(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()));
        }
        return Ok(());
    }

    let mut entries = fs::read_dir(path).map_err(|e| format!("cannot read '{}': {e}", path.display()))?;
    let mut paths = Vec::new();
    while let Some(entry) = entries.next() {
        let entry = entry.map_err(|e| format!("cannot read directory entry in '{}': {e}", path.display()))?;
        let entry_path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if entry_path.is_dir() && should_skip_tree_entry(&file_name) {
            continue;
        }
        paths.push(entry_path);
    }
    paths.sort();
    for entry_path in paths {
        collect_cool_files(&entry_path, out)?;
    }
    Ok(())
}

// ── REPL ──────────────────────────────────────────────────────────────────────

fn repl() {
    use std::io::{self, BufRead, Write};
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    println!("Cool 1.0.0 — type 'exit' to quit");
    let stdin = io::stdin();
    loop {
        print!(">>> ");
        io::stdout().flush().ok();

        let mut line = String::new();
        if stdin.lock().read_line(&mut line).is_err() || line.trim() == "exit" {
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        if let Err(e) = run_source(&line, cwd.clone()) {
            eprintln!("Error: {}", e);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TestMode {
    Interpreter,
    Vm,
    Native,
}

impl TestMode {
    fn label(self) -> &'static str {
        match self {
            Self::Interpreter => "interpreter",
            Self::Vm => "bytecode VM",
            Self::Native => "native",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BuildProfile {
    Dev,
    Release,
    Freestanding,
    Strict,
}

impl BuildProfile {
    fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "dev" => Ok(Self::Dev),
            "release" => Ok(Self::Release),
            "freestanding" => Ok(Self::Freestanding),
            "strict" => Ok(Self::Strict),
            _ => Err(format!(
                "unknown build profile '{raw}' (expected dev, release, freestanding, or strict)"
            )),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Dev => "dev",
            Self::Release => "release",
            Self::Freestanding => "freestanding",
            Self::Strict => "strict",
        }
    }

    fn default_build_mode(self) -> llvm_codegen::NativeBuildMode {
        match self {
            Self::Freestanding => llvm_codegen::NativeBuildMode::Freestanding,
            Self::Dev | Self::Release | Self::Strict => llvm_codegen::NativeBuildMode::Hosted,
        }
    }

    fn strict_check(self) -> Option<bool> {
        match self {
            Self::Dev => Some(false),
            Self::Strict => Some(true),
            Self::Release | Self::Freestanding => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BuildEmitKind {
    Binary,
    Object,
    Assembly,
    LlvmIr,
    StaticLib,
    SharedLib,
}

impl BuildEmitKind {
    fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "bin" | "binary" => Ok(Self::Binary),
            "obj" | "object" => Ok(Self::Object),
            "asm" | "assembly" => Ok(Self::Assembly),
            "ir" | "ll" | "llvm-ir" | "llvm_ir" => Ok(Self::LlvmIr),
            "lib" | "library" | "staticlib" | "static-lib" => Ok(Self::StaticLib),
            "shared" | "sharedlib" | "shared-lib" | "dylib" | "so" | "dll" => Ok(Self::SharedLib),
            _ => Err(format!(
                "unknown build emit '{raw}' (expected binary, object, assembly, llvm-ir, staticlib, or sharedlib)"
            )),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Binary => "binary",
            Self::Object => "object",
            Self::Assembly => "assembly",
            Self::LlvmIr => "llvm-ir",
            Self::StaticLib => "staticlib",
            Self::SharedLib => "sharedlib",
        }
    }

    fn native_artifact(self) -> llvm_codegen::NativeArtifactKind {
        match self {
            Self::Binary => llvm_codegen::NativeArtifactKind::Binary,
            Self::Object => llvm_codegen::NativeArtifactKind::Object,
            Self::Assembly => llvm_codegen::NativeArtifactKind::Assembly,
            Self::LlvmIr => llvm_codegen::NativeArtifactKind::LlvmIr,
            Self::StaticLib => llvm_codegen::NativeArtifactKind::StaticLib,
            Self::SharedLib => llvm_codegen::NativeArtifactKind::SharedLib,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BuildFinalArtifact {
    Emit(BuildEmitKind),
    KernelImage,
}

const DEFAULT_KERNEL_IMAGE_TARGET: &str = "x86_64-unknown-linux-gnu";

impl BuildFinalArtifact {
    fn label(self) -> &'static str {
        match self {
            Self::Emit(kind) => kind.label(),
            Self::KernelImage => "kernel image",
        }
    }

    fn output_path(self, base: &Path, target_triple: Option<&str>) -> PathBuf {
        match self {
            Self::Emit(BuildEmitKind::Binary) => base.to_path_buf(),
            Self::Emit(BuildEmitKind::Object) => base.with_extension("o"),
            Self::Emit(BuildEmitKind::Assembly) => base.with_extension("s"),
            Self::Emit(BuildEmitKind::LlvmIr) => base.with_extension("ll"),
            Self::Emit(BuildEmitKind::StaticLib) => static_library_path(base),
            Self::Emit(BuildEmitKind::SharedLib) => shared_library_path(base, target_triple),
            Self::KernelImage => base.with_extension("elf"),
        }
    }
}

fn effective_build_target(requested_target: Option<String>, final_artifact: BuildFinalArtifact) -> Option<String> {
    if final_artifact == BuildFinalArtifact::KernelImage && requested_target.is_none() {
        Some(DEFAULT_KERNEL_IMAGE_TARGET.to_string())
    } else {
        requested_target
    }
}

fn static_library_path(base: &Path) -> PathBuf {
    let file_name = base.file_name().and_then(|name| name.to_str()).unwrap_or("cool");
    let archive_name = format!("lib{file_name}.a");
    match base.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        Some(parent) => parent.join(archive_name),
        None => PathBuf::from(archive_name),
    }
}

fn shared_library_path(base: &Path, target_triple: Option<&str>) -> PathBuf {
    let file_name = base.file_name().and_then(|name| name.to_str()).unwrap_or("cool");
    let ext = if target_triple
        .map(|triple| triple.contains("windows"))
        .unwrap_or(cfg!(target_os = "windows"))
    {
        "dll"
    } else if target_triple
        .map(|triple| triple.contains("darwin") || triple.contains("apple"))
        .unwrap_or(cfg!(target_os = "macos"))
    {
        "dylib"
    } else {
        "so"
    };
    let library_name = format!("lib{file_name}.{ext}");
    match base.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        Some(parent) => parent.join(library_name),
        None => PathBuf::from(library_name),
    }
}

struct TestFailure {
    stdout: String,
    stderr: String,
}

struct BenchConfig {
    runs: usize,
    warmups: usize,
    profile: bool,
}

struct BenchFailure {
    stdout: String,
    stderr: String,
}

struct BenchResult {
    compile_time: std::time::Duration,
    stats: benchmark::BenchStats,
    profile_report: Option<String>,
}

fn unique_temp_executable_path(stem: &str) -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let file_name = if std::env::consts::EXE_EXTENSION.is_empty() {
        format!("{stem}_{pid}_{nonce}")
    } else {
        format!("{stem}_{pid}_{nonce}.{}", std::env::consts::EXE_EXTENSION)
    };
    std::env::temp_dir().join(file_name)
}

fn is_named_test_file(path: &Path) -> bool {
    if path.extension().and_then(|ext| ext.to_str()) != Some("cool") {
        return false;
    }
    let stem = path.file_stem().and_then(|name| name.to_str()).unwrap_or("");
    stem.starts_with("test_") || stem.ends_with("_test")
}

fn is_named_benchmark_file(path: &Path) -> bool {
    if path.extension().and_then(|ext| ext.to_str()) != Some("cool") {
        return false;
    }
    let stem = path.file_stem().and_then(|name| name.to_str()).unwrap_or("");
    stem.starts_with("bench_") || stem.ends_with("_bench")
}

fn collect_test_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("cool test: cannot read '{}': {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("cool test: cannot read '{}': {e}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_test_files(&path, out)?;
        } else if is_named_test_file(&path) {
            out.push(path.canonicalize().unwrap_or(path));
        }
    }
    Ok(())
}

fn collect_benchmark_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("cool bench: cannot read '{}': {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("cool bench: cannot read '{}': {e}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_benchmark_files(&path, out)?;
        } else if is_named_benchmark_file(&path) {
            out.push(path.canonicalize().unwrap_or(path));
        }
    }
    Ok(())
}

fn resolve_test_targets(args: &[&String]) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    if args.is_empty() {
        let tests_dir = Path::new("tests");
        if !tests_dir.exists() {
            return Err(
                "cool test: no tests directory found.\nCreate tests/test_*.cool or pass explicit test files/directories."
                    .to_string(),
            );
        }
        collect_test_files(tests_dir, &mut files)?;
    } else {
        for raw in args {
            let path = Path::new(raw.as_str());
            if !path.exists() {
                return Err(format!("cool test: path not found: {}", raw));
            }
            if path.is_dir() {
                collect_test_files(path, &mut files)?;
            } else {
                if path.extension().and_then(|ext| ext.to_str()) != Some("cool") {
                    return Err(format!("cool test: explicit test files must end in .cool: {}", raw));
                }
                files.push(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()));
            }
        }
    }

    files.sort();
    files.dedup();
    if files.is_empty() {
        return Err(
            "cool test: no test files found.\nExpected files named test_*.cool or *_test.cool, or pass explicit .cool files."
                .to_string(),
        );
    }
    Ok(files)
}

fn resolve_benchmark_targets(args: &[&String], cwd: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    if args.is_empty() {
        let benchmarks_dir = match CoolProject::discover(cwd)? {
            Some(project) => project.root.join("benchmarks"),
            None => cwd.join("benchmarks"),
        };
        if !benchmarks_dir.exists() {
            return Err(
                "cool bench: no benchmarks directory found.\nCreate benchmarks/bench_*.cool or pass explicit benchmark files/directories."
                    .to_string(),
            );
        }
        collect_benchmark_files(&benchmarks_dir, &mut files)?;
    } else {
        for raw in args {
            let path = Path::new(raw.as_str());
            if !path.exists() {
                return Err(format!("cool bench: path not found: {}", raw));
            }
            if path.is_dir() {
                collect_benchmark_files(path, &mut files)?;
            } else {
                if path.extension().and_then(|ext| ext.to_str()) != Some("cool") {
                    return Err(format!(
                        "cool bench: explicit benchmark files must end in .cool: {}",
                        raw
                    ));
                }
                files.push(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()));
            }
        }
    }

    files.sort();
    files.dedup();
    if files.is_empty() {
        return Err(
            "cool bench: no benchmark files found.\nExpected files named bench_*.cool or *_bench.cool, or pass explicit .cool files."
                .to_string(),
        );
    }
    Ok(files)
}

fn display_relative_path(path: &Path, cwd: &Path) -> String {
    path.strip_prefix(cwd).unwrap_or(path).display().to_string()
}

fn run_script_test(path: &Path, mode: TestMode) -> Result<(), TestFailure> {
    let exe = std::env::current_exe().map_err(|e| TestFailure {
        stdout: String::new(),
        stderr: format!("failed to resolve current executable: {e}"),
    })?;
    let mut cmd = Command::new(exe);
    cmd.env_remove("COOL_SCRIPT_PATH");
    cmd.env_remove("COOL_PROGRAM_ARGS");
    if mode == TestMode::Vm {
        cmd.arg("--vm");
    }
    let output = cmd.arg(path).output().map_err(|e| TestFailure {
        stdout: String::new(),
        stderr: format!("failed to launch test: {e}"),
    })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(TestFailure {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }
}

fn run_native_test(path: &Path) -> Result<(), TestFailure> {
    let source = fs::read_to_string(path).map_err(|e| TestFailure {
        stdout: String::new(),
        stderr: format!("failed to read '{}': {e}", path.display()),
    })?;
    let binary_path = unique_temp_executable_path(
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("cool_test_native"),
    );
    let compile_result = compile_to_native(&source, &binary_path, path);
    if let Err(err) = compile_result {
        let _ = fs::remove_file(&binary_path);
        return Err(TestFailure {
            stdout: String::new(),
            stderr: err,
        });
    }

    let output = Command::new(&binary_path)
        .env_remove("COOL_SCRIPT_PATH")
        .env_remove("COOL_PROGRAM_ARGS")
        .output()
        .map_err(|e| TestFailure {
            stdout: String::new(),
            stderr: format!("failed to run compiled test '{}': {e}", path.display()),
        });
    let _ = fs::remove_file(&binary_path);

    match output {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => Err(TestFailure {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        }),
        Err(err) => Err(err),
    }
}

fn run_test_file(path: &Path, mode: TestMode) -> Result<(), TestFailure> {
    match mode {
        TestMode::Interpreter | TestMode::Vm => run_script_test(path, mode),
        TestMode::Native => run_native_test(path),
    }
}

fn cmd_test(args: &[&String]) -> Result<(), String> {
    let mut use_vm = false;
    let mut use_compile = false;
    let mut targets = Vec::new();
    for arg in args {
        match arg.as_str() {
            "--vm" => use_vm = true,
            "--compile" => use_compile = true,
            "--help" | "-h" => {
                println!(
                    "\
Usage: cool test [--vm | --compile] [path ...]

With no path arguments, `cool test` discovers files named `test_*.cool` or `*_test.cool`
under `tests/` recursively.

Examples:
  cool test
  cool test tests/parser_test.cool
  cool test tests/unit tests/integration
  cool test --vm
  cool test --compile"
                );
                return Ok(());
            }
            _ => targets.push(*arg),
        }
    }
    if use_vm && use_compile {
        return Err("cool test: choose either --vm or --compile, not both".to_string());
    }

    let mode = if use_compile {
        TestMode::Native
    } else if use_vm {
        TestMode::Vm
    } else {
        TestMode::Interpreter
    };

    let cwd = std::env::current_dir().map_err(|e| format!("cool test: cannot read current directory: {e}"))?;
    let tests = resolve_test_targets(&targets)?;
    println!("running {} Cool test file(s) with {}", tests.len(), mode.label());

    let mut passed = 0usize;
    let mut failed = 0usize;
    for path in &tests {
        let display = display_relative_path(path, &cwd);
        match run_test_file(path, mode) {
            Ok(()) => {
                println!("ok {}", display);
                passed += 1;
            }
            Err(failure) => {
                println!("FAILED {}", display);
                if !failure.stdout.trim().is_empty() {
                    println!("---- {} stdout ----", display);
                    print!("{}", failure.stdout);
                    if !failure.stdout.ends_with('\n') {
                        println!();
                    }
                }
                if !failure.stderr.trim().is_empty() {
                    println!("---- {} stderr ----", display);
                    eprint!("{}", failure.stderr);
                    if !failure.stderr.ends_with('\n') {
                        eprintln!();
                    }
                }
                failed += 1;
            }
        }
    }

    println!();
    if failed == 0 {
        println!("test result: ok. {} passed; 0 failed", passed);
        Ok(())
    } else {
        println!("test result: FAILED. {} passed; {} failed", passed, failed);
        Err(format!("cool test: {} test file(s) failed", failed))
    }
}

fn run_native_benchmark(path: &Path, current_dir: &Path, config: &BenchConfig) -> Result<BenchResult, BenchFailure> {
    let source = fs::read_to_string(path).map_err(|e| BenchFailure {
        stdout: String::new(),
        stderr: format!("failed to read '{}': {e}", path.display()),
    })?;
    let binary_path = unique_temp_executable_path(
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("cool_bench_native"),
    );

    let compile_time = match benchmark::time_command(|| compile_to_native(&source, &binary_path, path)) {
        Ok(duration) => duration,
        Err(err) => {
            let _ = fs::remove_file(&binary_path);
            return Err(BenchFailure {
                stdout: String::new(),
                stderr: err,
            });
        }
    };

    let benchmark_result = (|| {
        benchmark::capture_binary_output(&binary_path, Some(current_dir))?;
        let samples = benchmark::measure_binary_runs(&binary_path, Some(current_dir), config.warmups, config.runs)?;
        let stats = benchmark::summarize(&samples)?;
        let profile_report = if config.profile {
            Some(capture_profile_report(&binary_path, current_dir)?)
        } else {
            None
        };
        Ok(BenchResult {
            compile_time,
            stats,
            profile_report,
        })
    })();

    let _ = fs::remove_file(&binary_path);

    benchmark_result.map_err(|stderr| BenchFailure {
        stdout: String::new(),
        stderr,
    })
}

fn capture_profile_report(binary_path: &Path, current_dir: &Path) -> Result<String, String> {
    let report_path = unique_temp_path_for_suffix(
        binary_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("cool_profile"),
        "profile.txt",
    );
    let output = Command::new(binary_path)
        .current_dir(current_dir)
        .env("COOL_PROFILE_OUT", &report_path)
        .output()
        .map_err(|e| format!("run {} with profiling: {e}", binary_path.display()))?;
    if !output.status.success() {
        let _ = fs::remove_file(&report_path);
        return Err(benchmark::format_command_failure(
            &format!("run {}", binary_path.display()),
            &output,
        ));
    }
    let report =
        fs::read_to_string(&report_path).map_err(|e| format!("read profile report {}: {e}", report_path.display()))?;
    let _ = fs::remove_file(&report_path);
    Ok(report)
}

fn unique_temp_path_for_suffix(stem: &str, suffix: &str) -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    std::env::temp_dir().join(format!("{stem}_{pid}_{nonce}_{suffix}"))
}

fn run_benchmark_file(path: &Path, current_dir: &Path, config: &BenchConfig) -> Result<BenchResult, BenchFailure> {
    run_native_benchmark(path, current_dir, config)
}

fn cmd_bench(args: &[&String]) -> Result<(), String> {
    let mut config = BenchConfig {
        runs: 5,
        warmups: 1,
        profile: false,
    };
    let mut targets = Vec::new();
    let mut args = args.iter().copied();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--runs" => {
                let value = args
                    .next()
                    .ok_or_else(|| "cool bench: --runs requires a value".to_string())?;
                config.runs = value
                    .parse::<usize>()
                    .map_err(|_| format!("cool bench: invalid --runs value: {value}"))?;
            }
            "--warmups" => {
                let value = args
                    .next()
                    .ok_or_else(|| "cool bench: --warmups requires a value".to_string())?;
                config.warmups = value
                    .parse::<usize>()
                    .map_err(|_| format!("cool bench: invalid --warmups value: {value}"))?;
            }
            "--profile" => config.profile = true,
            "--help" | "-h" => {
                println!(
                    "\
Usage: cool bench [--runs N] [--warmups N] [--profile] [path ...]

Compile and benchmark native Cool programs.

With no path arguments, `cool bench` discovers files named `bench_*.cool` or `*_bench.cool`
under `benchmarks/` recursively. Inside a project, discovery starts from the project root.

Examples:
  cool bench
  cool bench benchmarks/bench_main.cool
  cool bench benchmarks/numeric
  cool bench --runs 10 --warmups 2
  cool bench --profile"
                );
                return Ok(());
            }
            other if other.starts_with('-') => return Err(format!("cool bench: unexpected flag '{other}'")),
            _ => targets.push(arg),
        }
    }

    if config.runs == 0 {
        return Err("cool bench: --runs must be at least 1".to_string());
    }

    let cwd = std::env::current_dir().map_err(|e| format!("cool bench: cannot read current directory: {e}"))?;
    let benchmarks = resolve_benchmark_targets(&targets, &cwd)?;
    println!(
        "running {} Cool benchmark file(s) with native ({} warmup(s), {} measured run(s){})",
        benchmarks.len(),
        config.warmups,
        config.runs,
        if config.profile { ", profiling enabled" } else { "" }
    );

    let mut completed = 0usize;
    let mut failed = 0usize;
    let mut results = Vec::new();

    for path in &benchmarks {
        let display = display_relative_path(path, &cwd);
        match run_benchmark_file(path, &cwd, &config) {
            Ok(result) => {
                println!("ok {}", display);
                println!("  compile {}", benchmark::format_duration(result.compile_time));
                println!(
                    "  mean {}  median {}  min {}",
                    benchmark::format_duration(result.stats.mean),
                    benchmark::format_duration(result.stats.median),
                    benchmark::format_duration(result.stats.min),
                );
                if let Some(report) = &result.profile_report {
                    for line in report.lines() {
                        println!("  {line}");
                    }
                }
                completed += 1;
                results.push((display, result));
            }
            Err(failure) => {
                println!("FAILED {}", display);
                if !failure.stdout.trim().is_empty() {
                    println!("---- {} stdout ----", display);
                    print!("{}", failure.stdout);
                    if !failure.stdout.ends_with('\n') {
                        println!();
                    }
                }
                if !failure.stderr.trim().is_empty() {
                    println!("---- {} stderr ----", display);
                    eprint!("{}", failure.stderr);
                    if !failure.stderr.ends_with('\n') {
                        eprintln!();
                    }
                }
                failed += 1;
            }
        }
    }

    println!();
    if !results.is_empty() {
        println!("summary");
        println!(
            "{:<36} {:>10} {:>10} {:>10} {:>10}",
            "benchmark", "mean", "median", "min", "compile"
        );
        for (display, result) in &results {
            println!(
                "{:<36} {:>10} {:>10} {:>10} {:>10}",
                display,
                benchmark::format_duration(result.stats.mean),
                benchmark::format_duration(result.stats.median),
                benchmark::format_duration(result.stats.min),
                benchmark::format_duration(result.compile_time),
            );
        }
        println!();
    }

    if failed == 0 {
        println!("bench result: ok. {} benchmark(s) measured; 0 failed", completed);
        Ok(())
    } else {
        println!(
            "bench result: FAILED. {} benchmark(s) measured; {} failed",
            completed, failed
        );
        Err(format!("cool bench: {} benchmark file(s) failed", failed))
    }
}

fn resolve_build_profile(cli_profile: Option<&str>, manifest_profile: Option<&str>) -> Result<BuildProfile, String> {
    match cli_profile.or(manifest_profile) {
        Some(raw) => BuildProfile::parse(raw).map_err(|err| format!("cool build: {err}")),
        None => Ok(BuildProfile::Release),
    }
}

fn resolve_build_emit(cli_emit: Option<&str>, manifest_emit: Option<&str>) -> Result<Option<BuildEmitKind>, String> {
    match cli_emit.or(manifest_emit) {
        Some(raw) => BuildEmitKind::parse(raw)
            .map(Some)
            .map_err(|err| format!("cool build: {err}")),
        None => Ok(None),
    }
}

fn resolve_build_target(cli_target: Option<&str>, manifest_target: Option<&str>) -> Result<Option<String>, String> {
    match cli_target.or(manifest_target) {
        Some(raw) => {
            let target = raw.trim();
            if target.is_empty() {
                Err("cool build: target triple must not be empty".to_string())
            } else {
                Ok(Some(target.to_string()))
            }
        }
        None => Ok(None),
    }
}

fn resolve_build_toggle(
    flag_name: &str,
    cli_enable: bool,
    cli_disable: bool,
    manifest_value: Option<bool>,
    default_value: bool,
) -> Result<bool, String> {
    if cli_enable && cli_disable {
        return Err(format!(
            "cool build: choose either --{flag_name} or --no-{flag_name}, not both"
        ));
    }
    if cli_enable {
        Ok(true)
    } else if cli_disable {
        Ok(false)
    } else {
        Ok(manifest_value.unwrap_or(default_value))
    }
}

fn resolve_no_libc_toggle(
    cli_enable: bool,
    cli_disable: bool,
    manifest_value: Option<bool>,
    default_value: bool,
) -> Result<bool, String> {
    if cli_enable && cli_disable {
        return Err("cool build: choose either --no-libc or --with-libc, not both".to_string());
    }
    if cli_enable {
        Ok(true)
    } else if cli_disable {
        Ok(false)
    } else {
        Ok(manifest_value.unwrap_or(default_value))
    }
}

struct BuildRun<'a> {
    source: &'a str,
    script_path: &'a Path,
    manifest_path: Option<PathBuf>,
    cache_root: PathBuf,
    display: String,
    label: String,
    options: llvm_codegen::NativeCompileOptions,
    final_artifact: BuildFinalArtifact,
    final_path: PathBuf,
    compile_output: PathBuf,
    linker_script: Option<PathBuf>,
    incremental: bool,
    extra_inputs: Vec<PathBuf>,
}

fn native_build_identity(options: &llvm_codegen::NativeCompileOptions) -> Vec<(String, String)> {
    let mut identity = vec![
        ("cool".to_string(), env!("CARGO_PKG_VERSION").to_string()),
        ("build_mode".to_string(), format!("{:?}", options.build_mode)),
        ("artifact".to_string(), format!("{:?}", options.artifact_kind)),
        ("debug".to_string(), options.debug_info.to_string()),
        ("reproducible".to_string(), options.reproducible.to_string()),
        ("no_libc".to_string(), options.no_libc.to_string()),
        (
            "target".to_string(),
            options.target_triple.clone().unwrap_or_else(|| "<host>".to_string()),
        ),
        (
            "target_cpu".to_string(),
            options.target_cpu.clone().unwrap_or_else(|| "<default>".to_string()),
        ),
        (
            "target_features".to_string(),
            options
                .target_features
                .clone()
                .unwrap_or_else(|| "<default>".to_string()),
        ),
        ("entry".to_string(), options.entry_symbol.clone().unwrap_or_default()),
        (
            "source_root".to_string(),
            options
                .source_root
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default(),
        ),
        (
            "toolchain.cc".to_string(),
            options.toolchain.cc.clone().unwrap_or_default(),
        ),
        (
            "toolchain.ar".to_string(),
            options.toolchain.ar.clone().unwrap_or_default(),
        ),
        (
            "toolchain.lld".to_string(),
            options.toolchain.lld.clone().unwrap_or_default(),
        ),
        ("native.search".to_string(), options.native_links.search_paths.join(";")),
        ("native.rpath".to_string(), options.native_links.rpaths.join(";")),
        (
            "native.libs".to_string(),
            options
                .native_links
                .libraries
                .iter()
                .map(|library| format!("{:?}:{}", library.kind, library.name))
                .collect::<Vec<_>>()
                .join(";"),
        ),
    ];
    if let Some(cool) = &options.toolchain.cool {
        identity.push(("toolchain.cool".to_string(), cool.clone()));
    }
    identity
}

fn perform_native_build(run: BuildRun<'_>) -> Result<(), String> {
    validate_native_toolchain(&run.options.toolchain, "cool build")?;

    let cache_outputs = if run.final_artifact == BuildFinalArtifact::KernelImage {
        vec![run.compile_output.clone(), run.final_path.clone()]
    } else {
        vec![run.final_path.clone()]
    };

    println!("  Compiling {}{}", run.display, run.label);
    if run.incremental {
        let fingerprint = build_cache::BuildFingerprintInput {
            entry_path: run.script_path.to_path_buf(),
            manifest_path: run.manifest_path.clone(),
            extra_files: run.extra_inputs.clone(),
            identity: native_build_identity(&run.options),
        }
        .compute()?;
        let cache = build_cache::BuildCache::new(run.cache_root.clone(), fingerprint);
        if cache.restore(&cache_outputs)? {
            println!("   Finished from cache → {}", run.final_path.display());
            return Ok(());
        }
    }

    let t0 = std::time::Instant::now();
    match run.final_artifact {
        BuildFinalArtifact::KernelImage => {
            let script = run
                .linker_script
                .as_ref()
                .ok_or_else(|| "cool build: kernel-image output requires a linker script".to_string())?;
            let mut object_options = run.options.clone();
            object_options.artifact_kind = llvm_codegen::NativeArtifactKind::Object;
            compile_to_native_with_output(run.source, &run.compile_output, run.script_path, &object_options)?;
            link_kernel_image(
                &run.compile_output,
                script,
                &run.final_path,
                &run.options.toolchain,
                run.options.reproducible,
                run.options.entry_symbol.as_deref(),
            )?;
        }
        BuildFinalArtifact::Emit(_) => {
            compile_to_native_with_output(run.source, &run.compile_output, run.script_path, &run.options)?;
        }
    }

    if run.incremental {
        let fingerprint = build_cache::BuildFingerprintInput {
            entry_path: run.script_path.to_path_buf(),
            manifest_path: run.manifest_path,
            extra_files: run.extra_inputs,
            identity: native_build_identity(&run.options),
        }
        .compute()?;
        let cache = build_cache::BuildCache::new(run.cache_root, fingerprint);
        cache.store(&cache_outputs)?;
    }

    println!(
        "   Finished in {:.2}s → {}",
        t0.elapsed().as_secs_f64(),
        run.final_path.display()
    );
    Ok(())
}

fn build_label(
    profile: BuildProfile,
    build_mode: llvm_codegen::NativeBuildMode,
    artifact: BuildFinalArtifact,
    target_triple: Option<&str>,
    target_cpu: Option<&str>,
    target_features: Option<&str>,
    entry_symbol: Option<&str>,
    debug_info: bool,
    reproducible: bool,
    incremental: bool,
    no_libc: bool,
) -> String {
    let mut parts = Vec::new();
    if profile != BuildProfile::Release {
        parts.push(profile.label().to_string());
    }
    if build_mode == llvm_codegen::NativeBuildMode::Freestanding && profile != BuildProfile::Freestanding {
        parts.push("freestanding".to_string());
    }
    if artifact != BuildFinalArtifact::Emit(BuildEmitKind::Binary) {
        parts.push(artifact.label().to_string());
    }
    if let Some(target) = target_triple {
        parts.push(format!("target={target}"));
    }
    if let Some(cpu) = target_cpu {
        parts.push(format!("cpu={cpu}"));
    }
    if let Some(features) = target_features.filter(|value| !value.trim().is_empty()) {
        parts.push(format!("features={features}"));
    }
    if let Some(entry) = entry_symbol.filter(|value| !value.trim().is_empty()) {
        parts.push(format!("entry={entry}"));
    }
    if debug_info {
        parts.push("debug".to_string());
    }
    if reproducible {
        parts.push("reproducible".to_string());
    }
    if !incremental {
        parts.push("no-incremental".to_string());
    }
    if no_libc {
        parts.push("no-libc".to_string());
    }

    if parts.is_empty() {
        String::new()
    } else {
        format!(" [{}]", parts.join(", "))
    }
}

fn run_build_profile_checks(target: &Path, profile: BuildProfile, display: &str) -> Result<(), String> {
    let Some(strict) = profile.strict_check() else {
        return Ok(());
    };

    println!("  Checking {display} [{}]", profile.label());
    let report = tooling::build_check_report(target, strict)?;
    let error_count = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == tooling::DiagnosticSeverity::Error)
        .count();
    let warning_count = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == tooling::DiagnosticSeverity::Warning)
        .count();

    for diagnostic in &report.diagnostics {
        eprintln!("{}", format_tooling_diagnostic(diagnostic));
    }

    if error_count == 0 {
        if warning_count == 0 {
            println!("   Checked {} module(s)", report.modules_checked);
        } else {
            println!(
                "   Checked {} module(s); 0 error(s), {} warning(s)",
                report.modules_checked, warning_count
            );
        }
        Ok(())
    } else {
        Err(format!(
            "cool build: {} profile check failed with {} error(s), {} warning(s)",
            profile.label(),
            error_count,
            warning_count
        ))
    }
}

// ── `cool build` ─────────────────────────────────────────────────────────────

/// Build a Cool project or file to a selected native artifact.
///
/// Usage:
///   cool build                 # reads cool.toml in the current directory
///   cool build <file.cool>     # compiles the given file (output = file stem)
///   cool build --emit assembly [file.cool] # emits assembly (.s)
fn cmd_build(args: &[&String]) -> Result<(), String> {
    let mut freestanding = false;
    let mut linker_script_arg: Option<String> = None;
    let mut profile_arg: Option<String> = None;
    let mut emit_arg: Option<String> = None;
    let mut target_arg: Option<String> = None;
    let mut cpu_arg: Option<String> = None;
    let mut cpu_features_arg: Option<String> = None;
    let mut entry_arg: Option<String> = None;
    let mut debug_enabled = false;
    let mut debug_disabled = false;
    let mut reproducible_enabled = false;
    let mut reproducible_disabled = false;
    let mut incremental_enabled = false;
    let mut incremental_disabled = false;
    let mut no_libc_enabled = false;
    let mut no_libc_disabled = false;
    let mut help = false;
    let mut file_arg = None::<&String>;

    let mut args = args.iter().copied();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--freestanding" => freestanding = true,
            "--help" | "-h" => help = true,
            "--debug" => debug_enabled = true,
            "--no-debug" => debug_disabled = true,
            "--reproducible" => reproducible_enabled = true,
            "--no-reproducible" => reproducible_disabled = true,
            "--incremental" => incremental_enabled = true,
            "--no-incremental" => incremental_disabled = true,
            "--profile" => {
                let value = args
                    .next()
                    .ok_or_else(|| "cool build: --profile requires a value".to_string())?;
                profile_arg = Some(value.clone());
            }
            other if other.starts_with("--profile=") => {
                profile_arg = Some(other["--profile=".len()..].to_string());
            }
            "--emit" => {
                let value = args
                    .next()
                    .ok_or_else(|| "cool build: --emit requires a value".to_string())?;
                emit_arg = Some(value.clone());
            }
            other if other.starts_with("--emit=") => {
                emit_arg = Some(other["--emit=".len()..].to_string());
            }
            "--target" => {
                let value = args
                    .next()
                    .ok_or_else(|| "cool build: --target requires a value".to_string())?;
                target_arg = Some(value.clone());
            }
            other if other.starts_with("--target=") => {
                target_arg = Some(other["--target=".len()..].to_string());
            }
            "--cpu" => {
                let value = args
                    .next()
                    .ok_or_else(|| "cool build: --cpu requires a value".to_string())?;
                cpu_arg = Some(value.clone());
            }
            other if other.starts_with("--cpu=") => {
                cpu_arg = Some(other["--cpu=".len()..].to_string());
            }
            "--cpu-features" => {
                let value = args
                    .next()
                    .ok_or_else(|| "cool build: --cpu-features requires a value".to_string())?;
                cpu_features_arg = Some(value.clone());
            }
            other if other.starts_with("--cpu-features=") => {
                cpu_features_arg = Some(other["--cpu-features=".len()..].to_string());
            }
            "--entry" => {
                let value = args
                    .next()
                    .ok_or_else(|| "cool build: --entry requires a value".to_string())?;
                entry_arg = Some(value.clone());
            }
            other if other.starts_with("--entry=") => {
                entry_arg = Some(other["--entry=".len()..].to_string());
            }
            "--no-libc" => no_libc_enabled = true,
            "--with-libc" => no_libc_disabled = true,
            other if other.starts_with("--linker-script=") => {
                linker_script_arg = Some(other["--linker-script=".len()..].to_string());
            }
            other if other.starts_with('-') => return Err(format!("cool build: unexpected flag '{other}'")),
            _ => {
                if file_arg.is_some() {
                    return Err(
                        "Usage: cool build [--profile <name>] [--emit <kind>] [--target <triple>] [--cpu <name>] [--cpu-features <spec>] [--entry <symbol>] [--debug] [--reproducible] [--incremental|--no-incremental] [--no-libc|--with-libc] [--freestanding] [--linker-script=<path>] [file.cool]"
                            .to_string(),
                    );
                }
                file_arg = Some(arg);
            }
        }
    }

    if help {
        println!(
            "\
Usage: cool build [--profile <name>] [--emit <kind>] [--target <triple>] [--cpu <name>] [--cpu-features <spec>] [--entry <symbol>] [--debug|--no-debug] [--reproducible|--no-reproducible] [--incremental|--no-incremental] [--no-libc|--with-libc] [--freestanding] [--linker-script=<path>] [file.cool]

Build a Cool project from cool.toml or compile a single file.

Options:
  --profile <name>      Select a named build profile: dev, release, freestanding, or strict
  --emit <kind>         Select the final artifact: binary, object, assembly, llvm-ir,
                        staticlib, or sharedlib
  --target <triple>     Emit code for an explicit LLVM target triple (for example
                        x86_64-unknown-linux-gnu or i386-unknown-linux-gnu)
  --cpu <name>          Override the LLVM target CPU (for example native, x86-64-v3, cortex-a72)
  --cpu-features <spec> Override LLVM target features (for example +sse4.2,+popcnt)
  --entry <symbol>      Select an explicit linker entry symbol for kernel/no-libc outputs
  --debug               Emit native debug info and line locations
  --no-debug            Override manifest defaults and disable native debug info
  --reproducible        Normalize source paths and deterministic tool output where supported
  --no-reproducible     Override manifest defaults and disable reproducible build settings
  --incremental         Force build-cache reuse when inputs are unchanged
  --no-incremental      Disable the project-local native build cache
  --no-libc             Skip the hosted C runtime and libc assumptions where supported
  --with-libc           Override manifest defaults and keep the hosted runtime/link path
  --freestanding        Compile in freestanding mode without the hosted Cool runtime
  --linker-script=<path>  Link the object file into a kernel image (.elf) using LLD and the
                          given GNU linker script; implies --freestanding unless --emit overrides
                          the final artifact selection

Examples:
  cool build
  cool build --profile dev
  cool build --debug --reproducible
  cool build --emit sharedlib
  cool build --cpu native --cpu-features +sse4.2,+popcnt
  cool build hello.cool
  cool build --target x86_64-unknown-linux-gnu --emit object hello.cool
  cool build --emit object hello.cool
  cool build --emit staticlib
  cool build --no-libc --entry _start hello.cool
  cool build --profile strict hello.cool
  cool build --freestanding
  cool build --freestanding hello.cool
  cool build --linker-script=link.ld hello.cool"
        );
        return Ok(());
    }

    match file_arg {
        // ── cool build  (no extra args) ───────────────────────────────────
        None => {
            let manifest_path = Path::new("cool.toml");
            if !manifest_path.exists() {
                return Err("cool build: no cool.toml found in current directory.\n\
                     Create one or run `cool build <file.cool>` to compile a single file."
                    .to_string());
            }

            let project = CoolProject::from_manifest_path(manifest_path)?;
            let profile = resolve_build_profile(profile_arg.as_deref(), project.build_profile.as_deref())?;
            let requested_emit = resolve_build_emit(emit_arg.as_deref(), project.build_emit.as_deref())?;
            let requested_target = resolve_build_target(target_arg.as_deref(), project.build_target.as_deref())?;
            let requested_cpu = cpu_arg.clone().or_else(|| project.build_cpu.clone());
            let requested_cpu_features = cpu_features_arg.clone().or_else(|| project.build_cpu_features.clone());
            let requested_entry = entry_arg.clone().or_else(|| project.build_entry.clone());
            let debug_info = resolve_build_toggle("debug", debug_enabled, debug_disabled, project.build_debug, false)?;
            let reproducible = resolve_build_toggle(
                "reproducible",
                reproducible_enabled,
                reproducible_disabled,
                project.build_reproducible,
                false,
            )?;
            let incremental = resolve_build_toggle(
                "incremental",
                incremental_enabled,
                incremental_disabled,
                project.build_incremental,
                true,
            )?;
            let no_libc = resolve_no_libc_toggle(no_libc_enabled, no_libc_disabled, project.build_no_libc, false)?;
            let effective_linker_script: Option<PathBuf> = match &linker_script_arg {
                Some(path) => Some(PathBuf::from(path)),
                None => project.linker_script.as_ref().map(|s| project.root.join(s)),
            };
            let freestanding_requested = freestanding
                || no_libc
                || profile.default_build_mode() == llvm_codegen::NativeBuildMode::Freestanding
                || linker_script_arg.is_some();
            let final_artifact = if requested_emit.is_none() && effective_linker_script.is_some() {
                BuildFinalArtifact::KernelImage
            } else if let Some(emit) = requested_emit {
                BuildFinalArtifact::Emit(emit)
            } else if freestanding_requested {
                BuildFinalArtifact::Emit(BuildEmitKind::Object)
            } else {
                BuildFinalArtifact::Emit(BuildEmitKind::Binary)
            };
            let effective_target = effective_build_target(requested_target, final_artifact);
            let build_mode = if final_artifact == BuildFinalArtifact::KernelImage || freestanding_requested {
                llvm_codegen::NativeBuildMode::Freestanding
            } else {
                llvm_codegen::NativeBuildMode::Hosted
            };
            if build_mode == llvm_codegen::NativeBuildMode::Freestanding
                && final_artifact == BuildFinalArtifact::Emit(BuildEmitKind::Binary)
                && !no_libc
            {
                return Err(
                    "cool build: freestanding builds cannot emit hosted binaries; use object, assembly, llvm-ir, staticlib, sharedlib, a kernel image, or pair --emit binary with --no-libc"
                        .to_string(),
                );
            }

            let main_path = project.main_path();
            if !main_path.exists() {
                return Err(format!(
                    "cool build: main file '{}' not found (from cool.toml)",
                    project.main
                ));
            }

            let source = fs::read_to_string(&main_path).map_err(|e| format!("cool build: {e}"))?;
            let display = format!("{} v{} ({})", project.name, project.version, project.main);
            run_build_profile_checks(&main_path, profile, &display)?;
            let toolchain = native_toolchain_config(&project);
            let native_links = native_link_config(&project);

            let base_output = PathBuf::from(project.output_name());
            let final_path = final_artifact.output_path(&base_output, effective_target.as_deref());
            let compile_output = if final_artifact == BuildFinalArtifact::KernelImage {
                base_output.with_extension("o")
            } else {
                final_path.clone()
            };

            let compile_kind = match final_artifact {
                BuildFinalArtifact::KernelImage => llvm_codegen::NativeArtifactKind::Object,
                BuildFinalArtifact::Emit(kind) => kind.native_artifact(),
            };
            let label = build_label(
                profile,
                build_mode,
                final_artifact,
                effective_target.as_deref(),
                requested_cpu.as_deref(),
                requested_cpu_features.as_deref(),
                requested_entry.as_deref(),
                debug_info,
                reproducible,
                incremental,
                no_libc,
            );

            perform_native_build(BuildRun {
                source: &source,
                script_path: &main_path,
                manifest_path: Some(project.root.join("cool.toml")),
                cache_root: project.root.join(".cool").join("cache").join("build"),
                display,
                label,
                options: llvm_codegen::NativeCompileOptions {
                    build_mode,
                    artifact_kind: compile_kind,
                    target_triple: effective_target,
                    target_cpu: requested_cpu,
                    target_features: requested_cpu_features,
                    entry_symbol: requested_entry,
                    debug_info,
                    reproducible,
                    no_libc,
                    capabilities: project.capabilities,
                    toolchain,
                    native_links,
                    source_root: Some(project.root.clone()),
                },
                final_artifact,
                final_path,
                compile_output,
                linker_script: effective_linker_script.clone(),
                incremental,
                extra_inputs: effective_linker_script.into_iter().collect(),
            })
        }

        // ── cool build <file.cool>  ───────────────────────────────────────
        Some(file_arg) => {
            let profile = resolve_build_profile(profile_arg.as_deref(), None)?;
            let requested_emit = resolve_build_emit(emit_arg.as_deref(), None)?;
            let requested_target = resolve_build_target(target_arg.as_deref(), None)?;
            let requested_cpu = cpu_arg.clone();
            let requested_cpu_features = cpu_features_arg.clone();
            let requested_entry = entry_arg.clone();
            let debug_info = resolve_build_toggle("debug", debug_enabled, debug_disabled, None, false)?;
            let reproducible =
                resolve_build_toggle("reproducible", reproducible_enabled, reproducible_disabled, None, false)?;
            let incremental =
                resolve_build_toggle("incremental", incremental_enabled, incremental_disabled, None, true)?;
            let no_libc = resolve_no_libc_toggle(no_libc_enabled, no_libc_disabled, None, false)?;
            let effective_linker_script = linker_script_arg.as_deref();
            let freestanding_requested = freestanding
                || no_libc
                || profile.default_build_mode() == llvm_codegen::NativeBuildMode::Freestanding
                || effective_linker_script.is_some();

            let file_path = Path::new(file_arg.as_str());
            if !file_path.exists() {
                return Err(format!("cool build: file not found: {}", file_arg));
            }
            if file_path.extension().and_then(|e| e.to_str()) != Some("cool") {
                eprintln!("cool build: warning: '{}' does not have a .cool extension", file_arg);
            }

            let source = fs::read_to_string(file_path).map_err(|e| format!("cool build: {e}"))?;
            let capabilities = capability_policy_for_dir(file_path.parent().unwrap_or(Path::new(".")))?;
            run_build_profile_checks(file_path, profile, &file_path.display().to_string())?;
            let final_artifact = if requested_emit.is_none() && effective_linker_script.is_some() {
                BuildFinalArtifact::KernelImage
            } else if let Some(emit) = requested_emit {
                BuildFinalArtifact::Emit(emit)
            } else if freestanding_requested {
                BuildFinalArtifact::Emit(BuildEmitKind::Object)
            } else {
                BuildFinalArtifact::Emit(BuildEmitKind::Binary)
            };
            let effective_target = effective_build_target(requested_target, final_artifact);
            let build_mode = if final_artifact == BuildFinalArtifact::KernelImage || freestanding_requested {
                llvm_codegen::NativeBuildMode::Freestanding
            } else {
                llvm_codegen::NativeBuildMode::Hosted
            };
            if build_mode == llvm_codegen::NativeBuildMode::Freestanding
                && final_artifact == BuildFinalArtifact::Emit(BuildEmitKind::Binary)
                && !no_libc
            {
                return Err(
                    "cool build: freestanding builds cannot emit hosted binaries; use object, assembly, llvm-ir, staticlib, sharedlib, a kernel image, or pair --emit binary with --no-libc"
                        .to_string(),
                );
            }

            let base_output = file_path.with_extension("");
            let final_path = final_artifact.output_path(&base_output, effective_target.as_deref());
            let compile_output = if final_artifact == BuildFinalArtifact::KernelImage {
                base_output.with_extension("o")
            } else {
                final_path.clone()
            };

            let compile_kind = match final_artifact {
                BuildFinalArtifact::KernelImage => llvm_codegen::NativeArtifactKind::Object,
                BuildFinalArtifact::Emit(kind) => kind.native_artifact(),
            };
            let label = build_label(
                profile,
                build_mode,
                final_artifact,
                effective_target.as_deref(),
                requested_cpu.as_deref(),
                requested_cpu_features.as_deref(),
                requested_entry.as_deref(),
                debug_info,
                reproducible,
                incremental,
                no_libc,
            );

            perform_native_build(BuildRun {
                source: &source,
                script_path: file_path,
                manifest_path: None,
                cache_root: file_path
                    .parent()
                    .unwrap_or(Path::new("."))
                    .join(".cool")
                    .join("cache")
                    .join("build"),
                display: file_path.display().to_string(),
                label,
                options: llvm_codegen::NativeCompileOptions {
                    build_mode,
                    artifact_kind: compile_kind,
                    target_triple: effective_target,
                    target_cpu: requested_cpu,
                    target_features: requested_cpu_features,
                    entry_symbol: requested_entry,
                    debug_info,
                    reproducible,
                    no_libc,
                    capabilities,
                    toolchain: llvm_codegen::NativeToolchainConfig::default(),
                    native_links: project::NativeLinkConfig::default(),
                    source_root: Some(script_parent_dir(file_path)),
                },
                final_artifact,
                final_path,
                compile_output,
                linker_script: effective_linker_script.map(PathBuf::from),
                incremental,
                extra_inputs: effective_linker_script.into_iter().map(PathBuf::from).collect(),
            })
        }
    }
}

fn cmd_fmt(args: &[&String]) -> Result<(), String> {
    let mut check = false;
    let mut targets = Vec::new();

    for arg in args {
        match arg.as_str() {
            "--check" => check = true,
            "--help" | "-h" => {
                println!(
                    "\
Usage: cool fmt [--check] [path ...]

Format Cool source files in place.

With no path arguments, `cool fmt` formats all `.cool` files under the current project
root (or the current directory if no project is active).

Examples:
  cool fmt
  cool fmt --check
  cool fmt src tests
  cool fmt app/main.cool"
                );
                return Ok(());
            }
            other if other.starts_with('-') => return Err(format!("cool fmt: unexpected flag '{other}'")),
            _ => targets.push(arg.as_str()),
        }
    }

    let cwd = std::env::current_dir().map_err(|e| format!("cool fmt: cannot read current directory: {e}"))?;
    let mut files = Vec::new();
    if targets.is_empty() {
        let root = current_project("cool fmt")
            .map(|project| project.root)
            .unwrap_or_else(|_| cwd.clone());
        collect_cool_files(&root, &mut files)?;
    } else {
        for target in targets {
            let path = Path::new(target);
            if !path.exists() {
                return Err(format!("cool fmt: path not found: {target}"));
            }
            collect_cool_files(path, &mut files)?;
        }
    }
    files.sort();
    files.dedup();

    if files.is_empty() {
        println!("cool fmt: no .cool files found");
        return Ok(());
    }

    let mut changed = Vec::new();
    for file in &files {
        let source = fs::read_to_string(file).map_err(|e| format!("cool fmt: {}: {e}", file.display()))?;
        let formatted = formatter::format_source(&source).map_err(|e| format!("cool fmt: {}: {e}", file.display()))?;
        if formatted != source {
            changed.push(file.clone());
            if !check {
                fs::write(file, formatted).map_err(|e| format!("cool fmt: {}: {e}", file.display()))?;
                println!("formatted {}", display_relative_path(file, &cwd));
            }
        }
    }

    if check {
        if changed.is_empty() {
            println!("fmt check: ok. {} file(s) already formatted", files.len());
            Ok(())
        } else {
            for file in &changed {
                println!("needs format {}", display_relative_path(file, &cwd));
            }
            Err(format!("cool fmt: {} file(s) need formatting", changed.len()))
        }
    } else {
        println!("formatted {} file(s)", changed.len());
        Ok(())
    }
}

fn cmd_hashpath(args: &[&String]) -> Result<(), String> {
    match args {
        [path] => {
            println!("{}", hash_package_path(Path::new(path.as_str()))?);
            Ok(())
        }
        _ => Err("Usage: cool hashpath <path>".to_string()),
    }
}

// ── `cool ast` ────────────────────────────────────────────────────────────────

fn cmd_ast(args: &[&String]) -> Result<(), String> {
    let mut include_line_markers = false;
    let mut file = None::<&str>;

    for arg in args {
        match arg.as_str() {
            "--raw" => include_line_markers = true,
            "--help" | "-h" => {
                println!(
                    "\
Usage: cool ast [--raw] <file.cool>

Parse a Cool source file and print its AST as pretty JSON.

Options:
  --raw    Include internal SetLine markers used for runtime diagnostics"
                );
                return Ok(());
            }
            other if other.starts_with('-') => return Err(format!("cool ast: unexpected flag '{other}'")),
            other => {
                if file.is_some() {
                    return Err("Usage: cool ast [--raw] <file.cool>".to_string());
                }
                file = Some(other);
            }
        }
    }

    let file = file.ok_or_else(|| "Usage: cool ast [--raw] <file.cool>".to_string())?;
    let dump = tooling::build_ast_dump(Path::new(file), include_line_markers)?;
    let json = serde_json::to_string_pretty(&dump).map_err(|e| format!("cool ast: failed to encode JSON: {e}"))?;
    println!("{json}");
    Ok(())
}

// ── `cool inspect` ────────────────────────────────────────────────────────────

fn cmd_inspect(args: &[&String]) -> Result<(), String> {
    let file = match args {
        [arg] if arg.as_str() != "--help" && arg.as_str() != "-h" => arg.as_str(),
        [arg] if arg.as_str() == "--help" || arg.as_str() == "-h" => {
            println!(
                "\
Usage: cool inspect <file.cool>

Parse a Cool source file and print a JSON summary of its top-level imports, functions,
classes, structs, and assigned symbols."
            );
            return Ok(());
        }
        _ => return Err("Usage: cool inspect <file.cool>".to_string()),
    };

    let report = tooling::build_inspect_report(Path::new(file))?;
    let json =
        serde_json::to_string_pretty(&report).map_err(|e| format!("cool inspect: failed to encode JSON: {e}"))?;
    println!("{json}");
    Ok(())
}

// ── `cool symbols` ────────────────────────────────────────────────────────────

fn cmd_symbols(args: &[&String]) -> Result<(), String> {
    let file = match args {
        [] => None,
        [arg] if arg.as_str() == "--help" || arg.as_str() == "-h" => {
            println!(
                "\
Usage: cool symbols [file.cool]

Resolve a Cool entry file and print a JSON symbol index for reachable imports and
top-level definitions. With no file argument inside a project, `cool symbols` uses
the manifest main file."
            );
            return Ok(());
        }
        [arg] if !arg.starts_with('-') => Some(arg.as_str()),
        [arg] => return Err(format!("cool symbols: unexpected flag '{arg}'")),
        _ => return Err("Usage: cool symbols [file.cool]".to_string()),
    };

    let target = match file {
        Some(path) => PathBuf::from(path),
        None => current_project("cool symbols")?.main_path(),
    };

    let report = tooling::build_symbol_index(&target)?;
    let json =
        serde_json::to_string_pretty(&report).map_err(|e| format!("cool symbols: failed to encode JSON: {e}"))?;
    println!("{json}");
    Ok(())
}

// ── `cool diff` ───────────────────────────────────────────────────────────────

fn cmd_diff(args: &[&String]) -> Result<(), String> {
    let (before, after) = match args {
        [before, after] => (before.as_str(), after.as_str()),
        [arg] if arg.as_str() == "--help" || arg.as_str() == "-h" => {
            println!(
                "\
Usage: cool diff <before.cool> <after.cool>

Compare two Cool source files and print a JSON summary of added, removed, and changed
top-level imports and symbols."
            );
            return Ok(());
        }
        _ => return Err("Usage: cool diff <before.cool> <after.cool>".to_string()),
    };

    let report = tooling::build_inspect_diff(Path::new(before), Path::new(after))?;
    let json = serde_json::to_string_pretty(&report).map_err(|e| format!("cool diff: failed to encode JSON: {e}"))?;
    println!("{json}");
    Ok(())
}

// ── `cool doc` ───────────────────────────────────────────────────────────────

fn cmd_doc(args: &[&String]) -> Result<(), String> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum DocFormat {
        Markdown,
        Html,
        Json,
    }

    let mut format = DocFormat::Markdown;
    let mut include_private = false;
    let mut output = None::<String>;
    let mut file = None::<&str>;

    let mut args = args.iter().copied();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => format = DocFormat::Json,
            "--private" => include_private = true,
            "--output" => {
                let value = args
                    .next()
                    .ok_or_else(|| "cool doc: --output requires a value".to_string())?;
                output = Some(value.clone());
            }
            "--format" => {
                let value = args
                    .next()
                    .ok_or_else(|| "cool doc: --format requires a value".to_string())?;
                format = match value.as_str() {
                    "markdown" | "md" => DocFormat::Markdown,
                    "html" => DocFormat::Html,
                    "json" => DocFormat::Json,
                    other => return Err(format!("cool doc: unsupported format '{other}'")),
                };
            }
            "--help" | "-h" => {
                println!(
                    "\
Usage: cool doc [--format markdown|html|json] [--private] [--output path] [file.cool]

Generate API documentation for a Cool file or project entrypoint.

Options:
  --format <kind>  Output format: markdown (default), html, or json
  --json           Shortcut for --format json
  --private        Include private and underscore-prefixed top-level symbols
  --output <path>  Write the rendered docs to a file instead of stdout

With no file argument inside a project, `cool doc` uses the manifest main file."
                );
                return Ok(());
            }
            other if other.starts_with('-') => return Err(format!("cool doc: unexpected flag '{other}'")),
            other => {
                if file.is_some() {
                    return Err(
                        "Usage: cool doc [--format markdown|html|json] [--private] [--output path] [file.cool]"
                            .to_string(),
                    );
                }
                file = Some(other);
            }
        }
    }

    let target = match file {
        Some(path) => PathBuf::from(path),
        None => current_project("cool doc")?.main_path(),
    };

    let report = tooling::build_doc_report(&target, include_private)?;
    let rendered = match format {
        DocFormat::Markdown => tooling::render_doc_markdown(&report),
        DocFormat::Html => tooling::render_doc_html(&report),
        DocFormat::Json => {
            serde_json::to_string_pretty(&report).map_err(|e| format!("cool doc: failed to encode JSON: {e}"))?
        }
    };

    if let Some(output_path) = output {
        let output_path = PathBuf::from(output_path);
        if let Some(parent) = output_path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("cool doc: cannot create '{}': {e}", parent.display()))?;
            }
        }
        fs::write(&output_path, rendered)
            .map_err(|e| format!("cool doc: cannot write '{}': {e}", output_path.display()))?;
        println!("Wrote docs → {}", output_path.display());
    } else {
        print!("{rendered}");
        if !rendered.ends_with('\n') {
            println!();
        }
    }

    Ok(())
}

// ── `cool bindgen` ───────────────────────────────────────────────────────────

fn cmd_bindgen(args: &[&String]) -> Result<(), String> {
    let mut header = None::<String>;
    let mut output = None::<String>;
    let mut library = None::<String>;
    let mut link_kind = None::<String>;

    let mut args = args.iter().copied();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--output" => {
                let value = args
                    .next()
                    .ok_or_else(|| "cool bindgen: --output requires a value".to_string())?;
                output = Some(value.clone());
            }
            "--library" => {
                let value = args
                    .next()
                    .ok_or_else(|| "cool bindgen: --library requires a value".to_string())?;
                library = Some(value.clone());
            }
            "--link-kind" => {
                let value = args
                    .next()
                    .ok_or_else(|| "cool bindgen: --link-kind requires a value".to_string())?;
                link_kind = Some(value.clone());
            }
            "--help" | "-h" => {
                println!(
                    "\
Usage: cool bindgen [--library name] [--link-kind kind] [--output path] <header.h>

Generate Cool FFI declarations from a C header subset.

Supported today:
  - simple #define constants
  - enum constants
  - struct/union layouts with scalar or pointer fields
  - plain C function prototypes

Options:
  --library <name>    Attach library metadata to generated extern defs
  --link-kind <kind>  Attach native link kind metadata (default/static/shared/framework)
  --output <path>     Write generated Cool source to a file instead of stdout"
                );
                return Ok(());
            }
            other if other.starts_with('-') => return Err(format!("cool bindgen: unexpected flag '{other}'")),
            other => {
                if header.is_some() {
                    return Err(
                        "Usage: cool bindgen [--library name] [--link-kind kind] [--output path] <header.h>"
                            .to_string(),
                    );
                }
                header = Some(other.to_string());
            }
        }
    }

    let header = header.ok_or_else(|| {
        "Usage: cool bindgen [--library name] [--link-kind kind] [--output path] <header.h>".to_string()
    })?;
    let rendered = bindgen::generate_bindings(Path::new(&header), &bindgen::BindgenOptions { library, link_kind })?;

    if let Some(output_path) = output {
        let output_path = PathBuf::from(output_path);
        if let Some(parent) = output_path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("cool bindgen: cannot create '{}': {e}", parent.display()))?;
            }
        }
        fs::write(&output_path, rendered)
            .map_err(|e| format!("cool bindgen: cannot write '{}': {e}", output_path.display()))?;
        println!("Wrote bindings → {}", output_path.display());
    } else {
        print!("{rendered}");
        if !rendered.ends_with('\n') {
            println!();
        }
    }
    Ok(())
}

// ── `cool layout` ────────────────────────────────────────────────────────────

fn cmd_layout(args: &[&String]) -> Result<(), String> {
    let mut json = false;
    let mut output = None::<String>;
    let mut artifact = None::<String>;

    let mut args = args.iter().copied();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--output" => {
                let value = args
                    .next()
                    .ok_or_else(|| "cool layout: --output requires a value".to_string())?;
                output = Some(value.clone());
            }
            "--help" | "-h" => {
                println!(
                    "\
Usage: cool layout [--json] [--output path] <artifact>

Inspect section, symbol, and archive-member layout for an object file, executable,
shared library, kernel image, or static archive."
                );
                return Ok(());
            }
            other if other.starts_with('-') => return Err(format!("cool layout: unexpected flag '{other}'")),
            other => {
                if artifact.is_some() {
                    return Err("Usage: cool layout [--json] [--output path] <artifact>".to_string());
                }
                artifact = Some(other.to_string());
            }
        }
    }

    let artifact = artifact.ok_or_else(|| "Usage: cool layout [--json] [--output path] <artifact>".to_string())?;
    let report = layout_tool::inspect_path(Path::new(&artifact))?;
    let rendered = if json {
        serde_json::to_string_pretty(&report).map_err(|e| format!("cool layout: failed to encode JSON: {e}"))?
    } else {
        layout_tool::render_text(&report)
    };

    if let Some(output_path) = output {
        let output_path = PathBuf::from(output_path);
        if let Some(parent) = output_path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("cool layout: cannot create '{}': {e}", parent.display()))?;
            }
        }
        fs::write(&output_path, rendered)
            .map_err(|e| format!("cool layout: cannot write '{}': {e}", output_path.display()))?;
        println!("Wrote layout report → {}", output_path.display());
    } else {
        print!("{rendered}");
        if !rendered.ends_with('\n') {
            println!();
        }
    }
    Ok(())
}

// ── `cool check` ──────────────────────────────────────────────────────────────

fn cmd_check(args: &[&String]) -> Result<(), String> {
    let mut json = false;
    let mut strict = false;
    let mut file = None::<&str>;

    for arg in args {
        match arg.as_str() {
            "--json" => json = true,
            "--strict" => strict = true,
            "--help" | "-h" => {
                println!(
                    "\
Usage: cool check [--json] [--strict] [file.cool]

Statically parse a Cool entry file, resolve reachable imports, and report:
  - Unresolved imports and import cycles
  - Duplicate top-level symbols and class members
  - Literal-type mismatches at typed def boundaries (type checker v0)

Options:
  --strict   Also require every top-level def to have fully annotated parameters
             and a return type; errors on any missing annotation

With no file argument inside a project, `cool check` uses the manifest main file."
                );
                return Ok(());
            }
            other if other.starts_with('-') => return Err(format!("cool check: unexpected flag '{other}'")),
            other => {
                if file.is_some() {
                    return Err("Usage: cool check [--json] [--strict] [file.cool]".to_string());
                }
                file = Some(other);
            }
        }
    }

    let target = match file {
        Some(path) => PathBuf::from(path),
        None => current_project("cool check")?.main_path(),
    };

    let report = tooling::build_check_report(&target, strict)?;
    let error_count = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == tooling::DiagnosticSeverity::Error)
        .count();
    let warning_count = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == tooling::DiagnosticSeverity::Warning)
        .count();
    if json {
        let encoded =
            serde_json::to_string_pretty(&report).map_err(|e| format!("cool check: failed to encode JSON: {e}"))?;
        println!("{encoded}");
    } else {
        if report.diagnostics.is_empty() {
            println!("check ok: {} module(s) checked, 0 issue(s)", report.modules_checked);
        } else {
            for diagnostic in &report.diagnostics {
                eprintln!("{}", format_tooling_diagnostic(diagnostic));
            }
            if error_count == 0 {
                println!(
                    "check ok: {} module(s) checked, 0 error(s), {} warning(s)",
                    report.modules_checked, warning_count
                );
            }
        }
    }

    if error_count == 0 {
        Ok(())
    } else {
        Err(format!(
            "cool check: {} error(s), {} warning(s)",
            error_count, warning_count
        ))
    }
}

fn format_tooling_diagnostic(diagnostic: &tooling::ToolingDiagnostic) -> String {
    let severity = match diagnostic.severity {
        tooling::DiagnosticSeverity::Error => "error",
        tooling::DiagnosticSeverity::Warning => "warning",
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

// ── `cool modulegraph` ────────────────────────────────────────────────────────

fn cmd_modulegraph(args: &[&String]) -> Result<(), String> {
    let file = match args {
        [arg] if arg.as_str() != "--help" && arg.as_str() != "-h" => arg.as_str(),
        [arg] if arg.as_str() == "--help" || arg.as_str() == "-h" => {
            println!(
                "\
Usage: cool modulegraph <file.cool>

Resolve a Cool entry file and print its reachable file/module imports as pretty JSON."
            );
            return Ok(());
        }
        _ => return Err("Usage: cool modulegraph <file.cool>".to_string()),
    };

    let graph = tooling::build_module_graph(Path::new(file))?;
    let json =
        serde_json::to_string_pretty(&graph).map_err(|e| format!("cool modulegraph: failed to encode JSON: {e}"))?;
    println!("{json}");
    Ok(())
}

// ── `cool new` ────────────────────────────────────────────────────────────────

/// Scaffold a new Cool project.
///
/// cool new <name> [--template kind]
fn cmd_new(args: &[&String]) -> Result<(), String> {
    let new_app = new_command_path();
    run_bundled_app("cool new", &new_app, args, &[])
}

// ── `cool task` ───────────────────────────────────────────────────────────────

fn cmd_install(args: &[&String]) -> Result<(), String> {
    let install_app = install_command_path();
    let exe = std::env::current_exe().map_err(|e| format!("cool install: cannot resolve current executable: {e}"))?;
    run_bundled_app(
        "cool install",
        &install_app,
        args,
        &[("COOL_EXE_PATH", exe.to_string_lossy().to_string())],
    )
}

fn cmd_add(args: &[&String]) -> Result<(), String> {
    let add_app = add_command_path();
    let exe = std::env::current_exe().map_err(|e| format!("cool add: cannot resolve current executable: {e}"))?;
    run_bundled_app(
        "cool add",
        &add_app,
        args,
        &[("COOL_EXE_PATH", exe.to_string_lossy().to_string())],
    )
}

fn cmd_pkg(args: &[&String]) -> Result<(), String> {
    let pkg_app = pkg_command_path();
    let exe = std::env::current_exe().map_err(|e| format!("cool pkg: cannot resolve current executable: {e}"))?;
    run_bundled_app(
        "cool pkg",
        &pkg_app,
        args,
        &[("COOL_EXE_PATH", exe.to_string_lossy().to_string())],
    )
}

fn cmd_task(args: &[&String]) -> Result<(), String> {
    let task_app = task_command_path();
    run_bundled_app("cool task", &task_app, args, &[])
}

// ── `cool bundle` ────────────────────────────────────────────────────────────
fn cmd_bundle(args: &[&String]) -> Result<(), String> {
    let bundle_app = bundle_command_path();
    let exe = std::env::current_exe().map_err(|e| format!("cool bundle: cannot resolve current executable: {e}"))?;
    run_bundled_app(
        "cool bundle",
        &bundle_app,
        args,
        &[("COOL_EXE_PATH", exe.to_string_lossy().to_string())],
    )
}

// ── `cool release` ────────────────────────────────────────────────────────────
fn cmd_release(args: &[&String]) -> Result<(), String> {
    let release_app = release_command_path();
    let exe = std::env::current_exe().map_err(|e| format!("cool release: cannot resolve current executable: {e}"))?;
    run_bundled_app(
        "cool release",
        &release_app,
        args,
        &[("COOL_EXE_PATH", exe.to_string_lossy().to_string())],
    )
}

fn cmd_publish(args: &[&String]) -> Result<(), String> {
    let publish_app = publish_command_path();
    let exe = std::env::current_exe().map_err(|e| format!("cool publish: cannot resolve current executable: {e}"))?;
    run_bundled_app(
        "cool publish",
        &publish_app,
        args,
        &[("COOL_EXE_PATH", exe.to_string_lossy().to_string())],
    )
}

// ── help ──────────────────────────────────────────────────────────────────────

fn print_help() {
    println!(
        "\
Cool 1.0.0 — a native-first high-level systems language

USAGE:
    cool build                    Build the project described by cool.toml
    cool build --profile <name>   Build with dev/release/freestanding/strict profile rules
    cool build --emit <kind>      Emit binary/object/assembly/llvm-ir/staticlib artifacts
    cool build --target <triple>  Emit native code for an explicit LLVM target triple
    cool build --debug            Emit native debug info and runtime stack traces
    cool build --reproducible     Normalize source paths and enable deterministic tool output
    cool build <file.cool>        Compile a single file to a native binary
    cool build --freestanding     Build a freestanding object file (.o)
    cool build --linker-script=<ld>  Link a kernel image (.elf) via LLD
    cool --compile <file.cool>    Compile a file to a native binary (LLVM)
    cool <file.cool>              Run a file with the tree-walk interpreter
    cool --vm <file.cool>         Run a file with the bytecode VM
    cool                          Start the REPL
    cool ast <file.cool>          Print the parsed AST as JSON
    cool inspect <file.cool>      Print a JSON summary of top-level symbols
    cool symbols [file.cool]      Print a resolved JSON symbol index
    cool diff <before> <after>    Print a JSON summary of top-level changes
    cool doc [file.cool]          Generate API docs for modules, types, and functions
    cool bindgen <header.h>       Generate Cool FFI declarations from a C header subset
    cool check [file.cool]        Statically check imports, cycles, symbols, and types
    cool layout <artifact>        Inspect section and symbol layout for native artifacts
    cool modulegraph <file.cool>  Print the resolved import graph as JSON
    cool bundle [--target <triple>]  Build and package the project into a distributable tarball
    cool release [--bump patch] [--target <triple>]  Bump version, bundle, and git-tag a release
    cool publish [--dry-run]      Validate and package a source distribution for publishing
    cool install                  Fetch and lock project dependencies
    cool add <name> ...           Add a path or git dependency to cool.toml
    cool pkg <subcommand>         Cool-native package/project workflow helpers
    cool fmt [path ...]           Reformat Cool source files
    cool test [path ...]          Run discovered or explicit Cool tests
    cool bench [path ...]         Compile and benchmark native Cool programs
    cool task [name|list ...]     Run or list manifest-defined project tasks
    cool new <name>               Scaffold a new Cool project
    cool new <name> --template <kind>  Scaffold app/lib/service/freestanding templates
    cool lsp                      Start the language server (LSP) on stdin/stdout
    cool help                     Show this help message

FLAGS:
    --vm        Use the bytecode VM instead of the tree-walk interpreter
    --compile   Compile to a native binary using the LLVM backend

EXAMPLES:
    cool hello.cool               # interpret hello.cool
    cool build hello.cool         # compile hello.cool → ./hello (native binary)
    cool build --profile dev      # checked build using cool.toml or a file
    cool build --debug --reproducible
    cool build --emit object      # compile to .o without linking
    cool build --target i386-unknown-linux-gnu --emit llvm-ir
    cool build --emit staticlib   # compile to lib<name>.a
    cool build --profile strict hello.cool
    cool build --freestanding            # compile cool.toml project → ./name.o
    cool build --linker-script=link.ld   # compile + link → ./name.elf
    cool ast hello.cool           # dump the parsed AST as JSON
    cool inspect hello.cool       # summarize top-level symbols as JSON
    cool symbols hello.cool       # index resolved symbol locations as JSON
    cool diff old.cool new.cool   # compare top-level imports and symbols
    cool doc hello.cool           # generate markdown API docs
    cool bindgen include/demo.h --library demo
    cool check hello.cool         # statically check imports, cycles, symbols, and types
    cool layout hello.o           # inspect section and symbol layout
    cool modulegraph hello.cool   # resolve imports reachable from hello.cool
    cool add toolkit --path ../toolkit
    cool add theme --git https://github.com/acme/theme.git
    cool install                  # fetch git deps into .cool/deps and write cool.lock
    cool pkg doctor               # verify project entrypoints and dependency roots
    cool publish --dry-run        # validate source package contents without archiving
    cool fmt --check              # report files that need formatting
    cool test                     # run test_*.cool / *_test.cool under tests/
    cool bench                    # run bench_*.cool / *_bench.cool under benchmarks/
    cool task list                # list tasks from cool.toml
    cool new myapp                # create myapp/ project
    cool new toolkit --template lib
    cool build                    # compile using myapp/cool.toml

NOTES:
    The LLVM backend (--compile / build) supports:
    integers, floats, strings, booleans, variables, arithmetic,
    comparisons, if/elif/else, while/for loops, break/continue,
    functions with default/keyword args, lists/dicts/tuples, slicing,
    classes with inheritance/super(), list comprehensions,
    ternary expressions, in/not in, print()/str(), range(), len(),
    min()/max()/sum()/round()/sorted(), abs()/int()/float()/bool(),
    fixed-width int helpers i8/u8/i16/u16/i32/u32/i64,
    pointer-width aliases isize/usize and word_bits()/word_bytes(),
    source-relative file imports and project/package imports,
    LLVM-native extern def/data declarations with symbol/cc/section/library/link metadata,
    native import ffi (ffi.open/ffi.func),
    built-in import math/os/sys/path/platform/core/csv/datetime/hashlib/toml/yaml/sqlite/http/subprocess/argparse/logging/test/time and import random
    (seed/random/randint/uniform/choice/shuffle), plus json
    (loads/dumps), plus string
    (split/join/strip/lstrip/rstrip/upper/lower/replace/find/count/
    startswith/endswith/title/capitalize/format), plus list
    (sort/reverse/map/filter/reduce/flatten/unique), plus re
    (match/search/fullmatch/findall/sub/split), plus collections
    (Queue/Stack), csv(rows/dicts/write), datetime(now/format/parse/parts/add_seconds/diff_seconds), hashlib(md5/sha1/sha256/digest), toml(loads/dumps), yaml(loads/dumps for a config-oriented subset), sqlite(execute/query/scalar), http(get/post/head/getjson; requires curl), subprocess(run/call/check_output), argparse(parse/help), logging(basic_config/log/debug/info/warning/error),
    test(equal/not_equal/truthy/falsey/is_nil/not_nil/fail/raises), open()/file methods, with/context managers on
    normal exit, return/break/continue, caught exceptions, and unhandled native raises,
    closures/lambdas,
    inline asm, and raw memory (malloc/free/read_i8/u8/i16/u16/i32/u32/i64/
    write_i8/u8/i16/u16/i32/u32/i64 plus read/write_byte, read/write_f64, read/write_str,
    and volatile *_volatile MMIO variants for byte/i8/u8/i16/u16/i32/u32/i64/f64).
    FFI works in the interpreter and native builds, but not in the bytecode VM.
"
    );
}

// ── main ──────────────────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // ── Subcommand dispatch ───────────────────────────────────────────────
    if let Some(first) = args.get(1).map(|s| s.as_str()) {
        match first {
            "ast" => {
                let rest: Vec<&String> = args[2..].iter().collect();
                if let Err(e) = cmd_ast(&rest) {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
                return;
            }
            "inspect" => {
                let rest: Vec<&String> = args[2..].iter().collect();
                if let Err(e) = cmd_inspect(&rest) {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
                return;
            }
            "symbols" => {
                let rest: Vec<&String> = args[2..].iter().collect();
                if let Err(e) = cmd_symbols(&rest) {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
                return;
            }
            "diff" => {
                let rest: Vec<&String> = args[2..].iter().collect();
                if let Err(e) = cmd_diff(&rest) {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
                return;
            }
            "doc" => {
                let rest: Vec<&String> = args[2..].iter().collect();
                if let Err(e) = cmd_doc(&rest) {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
                return;
            }
            "bindgen" => {
                let rest: Vec<&String> = args[2..].iter().collect();
                if let Err(e) = cmd_bindgen(&rest) {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
                return;
            }
            "check" => {
                let rest: Vec<&String> = args[2..].iter().collect();
                if let Err(e) = cmd_check(&rest) {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
                return;
            }
            "layout" => {
                let rest: Vec<&String> = args[2..].iter().collect();
                if let Err(e) = cmd_layout(&rest) {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
                return;
            }
            "modulegraph" => {
                let rest: Vec<&String> = args[2..].iter().collect();
                if let Err(e) = cmd_modulegraph(&rest) {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
                return;
            }
            "build" => {
                let rest: Vec<&String> = args[2..].iter().collect();
                if let Err(e) = cmd_build(&rest) {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
                return;
            }
            "fmt" => {
                let rest: Vec<&String> = args[2..].iter().collect();
                if let Err(e) = cmd_fmt(&rest) {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
                return;
            }
            "new" => {
                let rest: Vec<&String> = args[2..].iter().collect();
                if let Err(e) = cmd_new(&rest) {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
                return;
            }
            "install" => {
                let rest: Vec<&String> = args[2..].iter().collect();
                if let Err(e) = cmd_install(&rest) {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
                return;
            }
            "add" => {
                let rest: Vec<&String> = args[2..].iter().collect();
                if let Err(e) = cmd_add(&rest) {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
                return;
            }
            "pkg" => {
                let rest: Vec<&String> = args[2..].iter().collect();
                if let Err(e) = cmd_pkg(&rest) {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
                return;
            }
            "test" => {
                let rest: Vec<&String> = args[2..].iter().collect();
                if let Err(e) = cmd_test(&rest) {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
                return;
            }
            "bench" => {
                let rest: Vec<&String> = args[2..].iter().collect();
                if let Err(e) = cmd_bench(&rest) {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
                return;
            }
            "task" => {
                let rest: Vec<&String> = args[2..].iter().collect();
                if let Err(e) = cmd_task(&rest) {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
                return;
            }
            "bundle" => {
                let rest: Vec<&String> = args[2..].iter().collect();
                if let Err(e) = cmd_bundle(&rest) {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
                return;
            }
            "release" => {
                let rest: Vec<&String> = args[2..].iter().collect();
                if let Err(e) = cmd_release(&rest) {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
                return;
            }
            "publish" => {
                let rest: Vec<&String> = args[2..].iter().collect();
                if let Err(e) = cmd_publish(&rest) {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
                return;
            }
            "hashpath" => {
                let rest: Vec<&String> = args[2..].iter().collect();
                if let Err(e) = cmd_hashpath(&rest) {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
                return;
            }
            "lsp" => {
                let rest: Vec<&String> = args[2..].iter().collect();
                if rest.iter().any(|a| a.as_str() == "--help" || a.as_str() == "-h") {
                    println!(
                        "\
Usage: cool lsp

Start the Cool language server (LSP) on stdin/stdout.

Capabilities:
  textDocumentSync    full sync (open, change, close)
  diagnostics         parse errors and duplicate-symbol warnings
  completion          keywords, builtins, modules, file-level symbols
  hover               function/class/struct signatures
  definition          go to definition within open files
  documentSymbol      list symbols in a file
  workspaceSymbol     search symbols across open files

Connect using any LSP client (VS Code, Neovim, Helix, etc.)."
                    );
                    return;
                }
                if let Err(e) = lsp::run_server() {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
                return;
            }
            "help" | "--help" | "-h" => {
                print_help();
                return;
            }
            _ => {} // fall through to legacy flag / file handling
        }
    }

    // ── Legacy flag-based dispatch (backward-compatible) ──────────────────
    let use_vm = args.iter().any(|a| a == "--vm");
    let use_compile = args.iter().any(|a| a == "--compile");
    let file_args: Vec<&String> = args[1..].iter().filter(|a| *a != "--vm" && *a != "--compile").collect();

    match file_args.len() {
        0 => repl(),

        1 => {
            let path = file_args[0];
            if !Path::new(path.as_str()).exists() {
                eprintln!("cool: file not found: {}", path);
                std::process::exit(1);
            }
            let source = fs::read_to_string(path).unwrap_or_else(|e| {
                eprintln!("cool: {e}");
                std::process::exit(1);
            });

            if use_compile {
                let out = Path::new(path.as_str()).with_extension("");
                match compile_to_native(&source, &out, Path::new(path.as_str())) {
                    Ok(()) => println!("Compiled to {}", out.display()),
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                }
                return;
            }

            let source_dir = script_parent_dir(Path::new(path.as_str()));

            let program_args: Vec<String> = file_args[1..].iter().map(|s| (*s).clone()).collect();
            std::env::set_var("COOL_SCRIPT_PATH", path);
            if !program_args.is_empty() {
                std::env::set_var("COOL_PROGRAM_ARGS", program_args.join("\x1F"));
            } else {
                std::env::remove_var("COOL_PROGRAM_ARGS");
            }

            let result = if use_vm {
                run_source_vm(&source, source_dir)
            } else {
                run_source(&source, source_dir)
            };

            std::env::remove_var("COOL_SCRIPT_PATH");
            std::env::remove_var("COOL_PROGRAM_ARGS");

            if let Err(e) = result {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }

        _ => {
            // Multiple args: first is file, rest are program args
            let path = file_args[0];
            if !Path::new(path.as_str()).exists() {
                eprintln!("cool: file not found: {}", path);
                std::process::exit(1);
            }
            let source = fs::read_to_string(path).unwrap_or_else(|e| {
                eprintln!("cool: {e}");
                std::process::exit(1);
            });

            let source_dir = script_parent_dir(Path::new(path.as_str()));

            let program_args: Vec<String> = file_args[1..].iter().map(|s| (*s).clone()).collect();
            std::env::set_var("COOL_SCRIPT_PATH", path);
            if !program_args.is_empty() {
                std::env::set_var("COOL_PROGRAM_ARGS", program_args.join("\x1F"));
            } else {
                std::env::remove_var("COOL_PROGRAM_ARGS");
            }

            let result = if use_vm {
                run_source_vm(&source, source_dir)
            } else {
                run_source(&source, source_dir)
            };

            std::env::remove_var("COOL_SCRIPT_PATH");
            std::env::remove_var("COOL_PROGRAM_ARGS");

            if let Err(e) = result {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
    }
}
