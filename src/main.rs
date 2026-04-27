mod argparse_runtime;
mod ast;
mod benchmark;
mod compiler;
mod core_runtime;
mod csv_runtime;
mod datetime_runtime;
mod hashlib_runtime;
mod http_runtime;
mod interpreter;
mod lexer;
mod llvm_codegen;
mod logging_runtime;
mod lsp;
mod module_exports;
mod opcode;
mod parser;
mod project;
mod sqlite_runtime;
mod subprocess_runtime;
mod toml_runtime;
mod tooling;
mod vm;
mod yaml_runtime;

use interpreter::Interpreter;
use lexer::Lexer;
use parser::Parser;
use project::{CoolProject, ModuleResolver};
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
    let mut interpreter = Interpreter::new(source_dir, source, module_resolver);
    interpreter.run(&program)
}

fn run_source_vm(source: &str, source_dir: PathBuf) -> Result<(), String> {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize()?;

    let mut parser = Parser::new(tokens);
    let program = parser.parse_program()?;

    let chunk = compiler::compile(&program)?;
    let module_resolver = ModuleResolver::discover_for_script(&source_dir)?;
    let mut machine = vm::VM::new(source_dir, module_resolver);
    machine.run(&chunk)
}

fn compile_to_native(source: &str, output_path: &Path, script_path: &Path) -> Result<(), String> {
    compile_to_native_with_mode(source, output_path, script_path, llvm_codegen::NativeBuildMode::Hosted)
}

fn compile_to_native_with_mode(
    source: &str,
    output_path: &Path,
    script_path: &Path,
    build_mode: llvm_codegen::NativeBuildMode,
) -> Result<(), String> {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize()?;

    let mut parser = Parser::new(tokens);
    let program = parser.parse_program()?;

    match build_mode {
        llvm_codegen::NativeBuildMode::Hosted => llvm_codegen::compile_program(&program, output_path, script_path),
        llvm_codegen::NativeBuildMode::Freestanding => {
            llvm_codegen::compile_program_with_mode(&program, output_path, script_path, build_mode)
        }
    }
}

fn bundled_command_path(file_name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("cmd").join(file_name)
}

fn find_lld() -> Option<String> {
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

fn link_kernel_image(obj_path: &Path, script_path: &Path, output_path: &Path) -> Result<(), String> {
    let lld = find_lld().ok_or_else(|| {
        "cool build: no LLD linker found — install LLVM/LLD to enable kernel image linking\n  \
         macOS: brew install llvm\n  \
         Debian/Ubuntu: apt install lld"
            .to_string()
    })?;

    let status = std::process::Command::new(&lld)
        .arg("-T")
        .arg(script_path)
        .arg("-o")
        .arg(output_path)
        .arg(obj_path)
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

fn install_command_path() -> PathBuf {
    bundled_command_path("install.cool")
}

fn add_command_path() -> PathBuf {
    bundled_command_path("add.cool")
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

fn current_project(command_name: &str) -> Result<CoolProject, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("{command_name}: cannot read current directory: {e}"))?;
    match CoolProject::discover(&cwd)? {
        Some(project) => Ok(project),
        None => Err(format!(
            "{command_name}: no cool.toml found in this directory or any parent"
        )),
    }
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

struct TestFailure {
    stdout: String,
    stderr: String,
}

struct BenchConfig {
    runs: usize,
    warmups: usize,
}

struct BenchFailure {
    stdout: String,
    stderr: String,
}

struct BenchResult {
    compile_time: std::time::Duration,
    stats: benchmark::BenchStats,
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
        Ok(BenchResult { compile_time, stats })
    })();

    let _ = fs::remove_file(&binary_path);

    benchmark_result.map_err(|stderr| BenchFailure {
        stdout: String::new(),
        stderr,
    })
}

fn run_benchmark_file(path: &Path, current_dir: &Path, config: &BenchConfig) -> Result<BenchResult, BenchFailure> {
    run_native_benchmark(path, current_dir, config)
}

fn cmd_bench(args: &[&String]) -> Result<(), String> {
    let mut config = BenchConfig { runs: 5, warmups: 1 };
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
            "--help" | "-h" => {
                println!(
                    "\
Usage: cool bench [--runs N] [--warmups N] [path ...]

Compile and benchmark native Cool programs.

With no path arguments, `cool bench` discovers files named `bench_*.cool` or `*_bench.cool`
under `benchmarks/` recursively. Inside a project, discovery starts from the project root.

Examples:
  cool bench
  cool bench benchmarks/bench_main.cool
  cool bench benchmarks/numeric
  cool bench --runs 10 --warmups 2"
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
        "running {} Cool benchmark file(s) with native ({} warmup(s), {} measured run(s))",
        benchmarks.len(),
        config.warmups,
        config.runs
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

fn build_label(profile: BuildProfile, freestanding: bool, kernel_image: bool) -> String {
    let mut parts = Vec::new();
    if profile != BuildProfile::Release {
        parts.push(profile.label().to_string());
    }
    if kernel_image {
        parts.push("freestanding -> kernel image".to_string());
    } else if freestanding && profile != BuildProfile::Freestanding {
        parts.push("freestanding".to_string());
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

/// Build a Cool project or file to a native binary.
///
/// Usage:
///   cool build                 # reads cool.toml in the current directory
///   cool build <file.cool>     # compiles the given file (output = file stem)
///   cool build --freestanding [file.cool]  # emits an object file (.o)
fn cmd_build(args: &[&String]) -> Result<(), String> {
    let mut freestanding = false;
    let mut linker_script_arg: Option<String> = None;
    let mut profile_arg: Option<String> = None;
    let mut help = false;
    let mut file_arg = None::<&String>;

    let mut args = args.iter().copied();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--freestanding" => freestanding = true,
            "--help" | "-h" => help = true,
            "--profile" => {
                let value = args
                    .next()
                    .ok_or_else(|| "cool build: --profile requires a value".to_string())?;
                profile_arg = Some(value.clone());
            }
            other if other.starts_with("--profile=") => {
                profile_arg = Some(other["--profile=".len()..].to_string());
            }
            other if other.starts_with("--linker-script=") => {
                linker_script_arg = Some(other["--linker-script=".len()..].to_string());
            }
            other if other.starts_with('-') => return Err(format!("cool build: unexpected flag '{other}'")),
            _ => {
                if file_arg.is_some() {
                    return Err("Usage: cool build [--freestanding] [--linker-script=<path>] [file.cool]".to_string());
                }
                file_arg = Some(arg);
            }
        }
    }

    if help {
        println!(
            "\
Usage: cool build [--profile <name>] [--freestanding] [--linker-script=<path>] [file.cool]

Build a Cool project from cool.toml or compile a single file.

Options:
  --profile <name>      Select a named build profile: dev, release, freestanding, or strict
  --freestanding          Emit an object file (.o) without linking the hosted Cool runtime
  --linker-script=<path>  Link the object file into a kernel image (.elf) using LLD and the
                          given GNU linker script; implies --freestanding

Examples:
  cool build
  cool build --profile dev
  cool build hello.cool
  cool build --profile strict hello.cool
  cool build --freestanding
  cool build --freestanding hello.cool
  cool build --linker-script=link.ld hello.cool"
        );
        return Ok(());
    }

    // A linker script implies freestanding.
    if linker_script_arg.is_some() {
        freestanding = true;
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

            freestanding = freestanding || profile.default_build_mode() == llvm_codegen::NativeBuildMode::Freestanding;

            // CLI flag overrides cool.toml; cool.toml linker_script implies freestanding.
            let effective_linker_script: Option<PathBuf> = match &linker_script_arg {
                Some(path) => {
                    freestanding = true;
                    Some(PathBuf::from(path))
                }
                None => project.linker_script.as_ref().map(|s| project.root.join(s)),
            };
            if effective_linker_script.is_some() {
                freestanding = true;
            }

            let build_mode = if freestanding {
                llvm_codegen::NativeBuildMode::Freestanding
            } else {
                llvm_codegen::NativeBuildMode::Hosted
            };

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

            let obj_path_buf = if freestanding {
                PathBuf::from(project.output_name()).with_extension("o")
            } else {
                PathBuf::from(project.output_name())
            };

            let label = build_label(profile, freestanding, effective_linker_script.is_some());
            println!("  Compiling {display}{label}");

            let t0 = std::time::Instant::now();
            compile_to_native_with_mode(&source, &obj_path_buf, &main_path, build_mode)?;

            let final_path = if let Some(script) = &effective_linker_script {
                let elf_path = PathBuf::from(project.output_name()).with_extension("elf");
                link_kernel_image(&obj_path_buf, script, &elf_path)?;
                elf_path
            } else {
                obj_path_buf
            };

            let elapsed = t0.elapsed();
            println!(
                "   Finished in {:.2}s → {}",
                elapsed.as_secs_f64(),
                final_path.display()
            );
            Ok(())
        }

        // ── cool build <file.cool>  ───────────────────────────────────────
        Some(file_arg) => {
            let profile = resolve_build_profile(profile_arg.as_deref(), None)?;
            freestanding = freestanding || profile.default_build_mode() == llvm_codegen::NativeBuildMode::Freestanding;

            let file_path = Path::new(file_arg.as_str());
            if !file_path.exists() {
                return Err(format!("cool build: file not found: {}", file_arg));
            }
            if file_path.extension().and_then(|e| e.to_str()) != Some("cool") {
                eprintln!("cool build: warning: '{}' does not have a .cool extension", file_arg);
            }

            let source = fs::read_to_string(file_path).map_err(|e| format!("cool build: {e}"))?;
            run_build_profile_checks(file_path, profile, &file_path.display().to_string())?;
            let build_mode = if freestanding {
                llvm_codegen::NativeBuildMode::Freestanding
            } else {
                llvm_codegen::NativeBuildMode::Hosted
            };

            let obj_path = file_path.with_extension("o");
            let output_path = if freestanding {
                obj_path.clone()
            } else {
                file_path.with_extension("")
            };

            let label = build_label(profile, freestanding, linker_script_arg.is_some());
            println!("  Compiling {}{} ...", file_path.display(), label);

            let t0 = std::time::Instant::now();
            compile_to_native_with_mode(&source, &output_path, file_path, build_mode)?;

            let final_path = if let Some(script) = &linker_script_arg {
                let elf_path = file_path.with_extension("elf");
                link_kernel_image(&obj_path, Path::new(script), &elf_path)?;
                elf_path
            } else {
                output_path
            };

            let elapsed = t0.elapsed();
            println!(
                "   Finished in {:.2}s → {}",
                elapsed.as_secs_f64(),
                final_path.display()
            );
            Ok(())
        }
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
    run_bundled_app("cool install", &install_app, args, &[])
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

// ── help ──────────────────────────────────────────────────────────────────────

fn print_help() {
    println!(
        "\
Cool 1.0.0 — a native-first high-level systems language

USAGE:
    cool build                    Build the project described by cool.toml
    cool build --profile <name>   Build with dev/release/freestanding/strict profile rules
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
    cool check [file.cool]        Statically check imports, cycles, symbols, and types
    cool modulegraph <file.cool>  Print the resolved import graph as JSON
    cool bundle                   Build and package the project into a distributable tarball
    cool release [--bump patch]   Bump version, bundle, and git-tag a release
    cool install                  Fetch and lock project dependencies
    cool add <name> ...           Add a path or git dependency to cool.toml
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
    cool build --profile strict hello.cool
    cool build --freestanding            # compile cool.toml project → ./name.o
    cool build --linker-script=link.ld   # compile + link → ./name.elf
    cool ast hello.cool           # dump the parsed AST as JSON
    cool inspect hello.cool       # summarize top-level symbols as JSON
    cool symbols hello.cool       # index resolved symbol locations as JSON
    cool diff old.cool new.cool   # compare top-level imports and symbols
    cool check hello.cool         # statically check imports, cycles, symbols, and types
    cool modulegraph hello.cool   # resolve imports reachable from hello.cool
    cool add toolkit --path ../toolkit
    cool add theme --git https://github.com/acme/theme.git
    cool install                  # fetch git deps into .cool/deps and write cool.lock
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
    LLVM-native extern def/data declarations with symbol/cc/section metadata,
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
    inline asm, and raw memory (malloc/free/read_i8/u8/i16/u16/i32/u32/i64/
    write_i8/u8/i16/u16/i32/u32/i64 plus read/write_byte, read/write_f64, read/write_str,
    and volatile *_volatile MMIO variants for byte/i8/u8/i16/u16/i32/u32/i64/f64).
    Closures/lambdas still require the interpreter or bytecode VM.
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
            "check" => {
                let rest: Vec<&String> = args[2..].iter().collect();
                if let Err(e) = cmd_check(&rest) {
                    eprintln!("{e}");
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

            let source_dir = Path::new(path.as_str())
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."));

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

            let source_dir = Path::new(path.as_str())
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."));

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
