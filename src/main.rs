mod argparse_runtime;
mod ast;
mod compiler;
mod csv_runtime;
mod datetime_runtime;
mod hashlib_runtime;
mod http_runtime;
mod interpreter;
mod lexer;
mod llvm_codegen;
mod logging_runtime;
mod lsp;
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
use project::{
    add_dependency_to_manifest, install_dependencies, normalize_dependency_source_arg, CoolProject, DependencySource,
    DependencySpec, ModuleResolver,
};
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

fn task_app_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("coolapps")
        .join("task.cool")
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

fn looks_like_git_source(source: &str) -> bool {
    !(Path::new(source).exists() || source.starts_with('.') || source.starts_with('/'))
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

struct TestFailure {
    stdout: String,
    stderr: String,
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

fn display_test_path(path: &Path, cwd: &Path) -> String {
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
        let display = display_test_path(path, &cwd);
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

// ── `cool build` ─────────────────────────────────────────────────────────────

/// Build a Cool project or file to a native binary.
///
/// Usage:
///   cool build                 # reads cool.toml in the current directory
///   cool build <file.cool>     # compiles the given file (output = file stem)
///   cool build --freestanding [file.cool]  # emits an object file (.o)
fn cmd_build(args: &[&String]) -> Result<(), String> {
    let mut freestanding = false;
    let mut help = false;
    let mut file_arg = None::<&String>;

    for arg in args {
        match arg.as_str() {
            "--freestanding" => freestanding = true,
            "--help" | "-h" => help = true,
            other if other.starts_with('-') => return Err(format!("cool build: unexpected flag '{other}'")),
            _ => {
                if file_arg.is_some() {
                    return Err("Usage: cool build [--freestanding] [file.cool]".to_string());
                }
                file_arg = Some(arg);
            }
        }
    }

    if help {
        println!(
            "\
Usage: cool build [--freestanding] [file.cool]

Build a Cool project from cool.toml or compile a single file.

Options:
  --freestanding    Emit an object file (.o) without linking the hosted Cool runtime

Examples:
  cool build
  cool build hello.cool
  cool build --freestanding
  cool build --freestanding hello.cool"
        );
        return Ok(());
    }

    let build_mode = if freestanding {
        llvm_codegen::NativeBuildMode::Freestanding
    } else {
        llvm_codegen::NativeBuildMode::Hosted
    };

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

            let main_path = project.main_path();
            if !main_path.exists() {
                return Err(format!(
                    "cool build: main file '{}' not found (from cool.toml)",
                    project.main
                ));
            }

            let source = fs::read_to_string(&main_path).map_err(|e| format!("cool build: {e}"))?;

            let output_path_buf = if freestanding {
                PathBuf::from(project.output_name()).with_extension("o")
            } else {
                PathBuf::from(project.output_name())
            };
            let output_path = output_path_buf.as_path();

            if freestanding {
                println!(
                    "  Compiling {} v{} ({}) [freestanding]",
                    project.name, project.version, project.main
                );
            } else {
                println!("  Compiling {} v{} ({})", project.name, project.version, project.main);
            }

            let t0 = std::time::Instant::now();
            compile_to_native_with_mode(&source, output_path, &main_path, build_mode)?;
            let elapsed = t0.elapsed();

            println!(
                "   Finished in {:.2}s → {}",
                elapsed.as_secs_f64(),
                output_path.display()
            );
            Ok(())
        }

        // ── cool build <file.cool>  ───────────────────────────────────────
        Some(file_arg) => {
            let file_path = Path::new(file_arg.as_str());
            if !file_path.exists() {
                return Err(format!("cool build: file not found: {}", file_arg));
            }
            if file_path.extension().and_then(|e| e.to_str()) != Some("cool") {
                eprintln!("cool build: warning: '{}' does not have a .cool extension", file_arg);
            }

            let source = fs::read_to_string(file_path).map_err(|e| format!("cool build: {e}"))?;

            let output_path = if freestanding {
                file_path.with_extension("o")
            } else {
                // Output binary = file stem (e.g., hello.cool → ./hello)
                file_path.with_extension("")
            };

            if freestanding {
                println!("  Compiling {} [freestanding] ...", file_path.display());
            } else {
                println!("  Compiling {} ...", file_path.display());
            }
            let t0 = std::time::Instant::now();
            compile_to_native_with_mode(&source, &output_path, file_path, build_mode)?;
            let elapsed = t0.elapsed();

            println!(
                "   Finished in {:.2}s → {}",
                elapsed.as_secs_f64(),
                output_path.display()
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
    let mut file = None::<&str>;

    for arg in args {
        match arg.as_str() {
            "--json" => json = true,
            "--help" | "-h" => {
                println!(
                    "\
Usage: cool check [--json] [file.cool]

Statically parse a Cool entry file, resolve reachable imports, and report unresolved imports
or import cycles. With no file argument inside a project, `cool check` uses the manifest main file."
                );
                return Ok(());
            }
            other if other.starts_with('-') => return Err(format!("cool check: unexpected flag '{other}'")),
            other => {
                if file.is_some() {
                    return Err("Usage: cool check [--json] [file.cool]".to_string());
                }
                file = Some(other);
            }
        }
    }

    let target = match file {
        Some(path) => PathBuf::from(path),
        None => current_project("cool check")?.main_path(),
    };

    let report = tooling::build_check_report(&target)?;
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
/// cool new <name>
fn cmd_new(args: &[&String]) -> Result<(), String> {
    let name = match args.first() {
        Some(n) => n.as_str(),
        None => return Err("Usage: cool new <project-name>".to_string()),
    };

    let project_dir = Path::new(name);
    if project_dir.exists() {
        return Err(format!("cool new: directory '{}' already exists", name));
    }

    // Create directory structure
    fs::create_dir_all(project_dir.join("src")).map_err(|e| format!("cool new: {e}"))?;
    fs::create_dir_all(project_dir.join("tests")).map_err(|e| format!("cool new: {e}"))?;

    // cool.toml
    let manifest = format!(
        "[project]\nname = \"{name}\"\nversion = \"0.1.0\"\nmain = \"src/main.cool\"\n\n[bundle]\ninclude = []\n\n[tasks.run]\ndescription = \"Run the app\"\nrun = \"cool src/main.cool\"\n\n[tasks.build]\ndescription = \"Build a native binary\"\nrun = \"cool build\"\n\n[tasks.bundle]\ndescription = \"Package a distributable tarball\"\nrun = \"cool bundle\"\n\n[tasks.release]\ndescription = \"Bump version, bundle, and tag a release\"\nrun = \"cool release\"\n\n[tasks.test]\ndescription = \"Run Cool tests\"\nrun = \"cool test\"\n"
    );
    fs::write(project_dir.join("cool.toml"), manifest).map_err(|e| format!("cool new: {e}"))?;

    // src/main.cool
    let main_src = format!("# {name}\n\nprint(\"Hello from {name}!\")\n");
    fs::write(project_dir.join("src").join("main.cool"), main_src).map_err(|e| format!("cool new: {e}"))?;

    // tests/test_main.cool
    let test_src = "assert 1 + 1 == 2\n";
    fs::write(project_dir.join("tests").join("test_main.cool"), test_src).map_err(|e| format!("cool new: {e}"))?;

    // .gitignore
    fs::write(project_dir.join(".gitignore"), format!("{name}\n*.o\n.cool/\n"))
        .map_err(|e| format!("cool new: {e}"))?;

    println!("  Created project '{name}'");
    println!("  ├── cool.toml");
    println!("  ├── src/");
    println!("  │   └── main.cool");
    println!("  ├── tests/");
    println!("  │   └── test_main.cool");
    println!("  └── .gitignore");
    println!();
    println!("  Run your project:");
    println!("    cd {name}");
    println!("    cool src/main.cool          # interpret");
    println!("    cool build                  # compile to native");
    println!("    cool build --freestanding   # emit a freestanding object file");
    println!("    cool bundle                 # package distributable tarball");
    println!("    cool release                # bump version, bundle, and tag");
    println!("    cool install                # fetch git dependencies");
    println!("    cool test                   # run tests/");
    println!("    cool task list              # show project tasks");
    Ok(())
}

// ── `cool task` ───────────────────────────────────────────────────────────────

fn cmd_install(args: &[&String]) -> Result<(), String> {
    if !args.is_empty() {
        return Err("Usage: cool install".to_string());
    }

    let project = current_project("cool install")?;
    let lockfile = install_dependencies(&project)?;
    println!(
        "  Installed {} dependenc{}",
        lockfile.dependencies.len(),
        if lockfile.dependencies.len() == 1 { "y" } else { "ies" }
    );
    println!("  Wrote {}", project.lockfile_path().display());
    Ok(())
}

fn cmd_add(args: &[&String]) -> Result<(), String> {
    if args.is_empty() {
        return Err(
            "Usage: cool add <name> (--path <path> | --git <url>) [--branch <name> | --tag <name> | --rev <sha>] [--version <semver>]"
                .to_string(),
        );
    }

    let project = current_project("cool add")?;
    let cwd = std::env::current_dir().map_err(|e| format!("cool add: cannot read current directory: {e}"))?;

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum SourceKind {
        Auto,
        Path,
        Git,
    }

    let name = args[0].as_str().to_string();
    let mut source_kind = SourceKind::Auto;
    let mut source = None::<String>;
    let mut branch = None::<String>;
    let mut tag = None::<String>;
    let mut rev = None::<String>;
    let mut version = None::<String>;
    let mut i = 1usize;

    while i < args.len() {
        match args[i].as_str() {
            "--path" => {
                source_kind = SourceKind::Path;
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "cool add: missing value after --path".to_string())?;
                source = Some(value.as_str().to_string());
            }
            "--git" => {
                source_kind = SourceKind::Git;
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "cool add: missing value after --git".to_string())?;
                source = Some(value.as_str().to_string());
            }
            "--branch" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "cool add: missing value after --branch".to_string())?;
                branch = Some(value.as_str().to_string());
            }
            "--tag" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "cool add: missing value after --tag".to_string())?;
                tag = Some(value.as_str().to_string());
            }
            "--rev" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "cool add: missing value after --rev".to_string())?;
                rev = Some(value.as_str().to_string());
            }
            "--version" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "cool add: missing value after --version".to_string())?;
                version = Some(value.as_str().to_string());
            }
            other => {
                if source.is_some() {
                    return Err(format!("cool add: unexpected argument '{}'", other));
                }
                source = Some(other.to_string());
            }
        }
        i += 1;
    }

    let source = source.ok_or_else(|| {
        "Usage: cool add <name> (--path <path> | --git <url>) [--branch <name> | --tag <name> | --rev <sha>] [--version <semver>]"
            .to_string()
    })?;

    if usize::from(branch.is_some()) + usize::from(tag.is_some()) + usize::from(rev.is_some()) > 1 {
        return Err("cool add: specify at most one of --branch, --tag, or --rev".to_string());
    }

    let kind = match source_kind {
        SourceKind::Path => SourceKind::Path,
        SourceKind::Git => SourceKind::Git,
        SourceKind::Auto => {
            if looks_like_git_source(&source) {
                SourceKind::Git
            } else {
                SourceKind::Path
            }
        }
    };

    let normalized_source = normalize_dependency_source_arg(&project.root, &cwd, &source);
    let mut dependency = match kind {
        SourceKind::Path => {
            if branch.is_some() || tag.is_some() || rev.is_some() {
                return Err("cool add: --branch/--tag/--rev are only valid for git dependencies".to_string());
            }
            DependencySpec::path(name.clone(), normalized_source)
        }
        SourceKind::Git => {
            let mut dep = DependencySpec::git(name.clone(), normalized_source);
            if let DependencySource::Git {
                branch: dep_branch,
                tag: dep_tag,
                rev: dep_rev,
                ..
            } = &mut dep.source
            {
                *dep_branch = branch;
                *dep_tag = tag;
                *dep_rev = rev;
            }
            dep
        }
        SourceKind::Auto => unreachable!(),
    };
    dependency.version = version;

    add_dependency_to_manifest(&project.manifest_path, &dependency)?;
    let updated_project = CoolProject::from_manifest_path(&project.manifest_path)?;
    install_dependencies(&updated_project)?;

    println!(
        "  Added dependency '{}' to {}",
        dependency.name,
        project.manifest_path.display()
    );
    println!(
        "  Installed dependencies and wrote {}",
        updated_project.lockfile_path().display()
    );
    Ok(())
}

fn cmd_task(args: &[&String]) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("cool task: cannot resolve current executable: {e}"))?;
    let task_app = task_app_path();
    if !task_app.exists() {
        return Err(format!("cool task: task runner not found at '{}'", task_app.display()));
    }

    let status = Command::new(exe)
        .arg(&task_app)
        .args(args.iter().map(|arg| arg.as_str()))
        .status()
        .map_err(|e| format!("cool task: failed to launch task runner: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(match status.code() {
            Some(code) => format!("cool task failed with exit code {code}"),
            None => "cool task failed".to_string(),
        })
    }
}

// ── `cool bundle` ────────────────────────────────────────────────────────────

fn target_triple() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        other => other,
    };
    format!("{os}-{arch}")
}

/// Recursively copy src into dst (dst is created if needed).
fn copy_into(src: &Path, dst: &Path) -> Result<(), String> {
    if src.is_dir() {
        fs::create_dir_all(dst).map_err(|e| format!("bundle: cannot create '{}': {e}", dst.display()))?;
        for entry in fs::read_dir(src).map_err(|e| format!("bundle: {e}"))? {
            let entry = entry.map_err(|e| format!("bundle: {e}"))?;
            copy_into(&entry.path(), &dst.join(entry.file_name()))?;
        }
    } else {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("bundle: {e}"))?;
        }
        fs::copy(src, dst)
            .map_err(|e| format!("bundle: cannot copy '{}' → '{}': {e}", src.display(), dst.display()))?;
    }
    Ok(())
}

/// Build the project and produce a distributable tarball under dist/.
///
///   cool bundle
fn cmd_bundle(args: &[&String]) -> Result<(), String> {
    if !args.is_empty() {
        return Err("Usage: cool bundle".to_string());
    }

    let manifest_path = Path::new("cool.toml");
    if !manifest_path.exists() {
        return Err("cool bundle: no cool.toml found in current directory".to_string());
    }
    let project = CoolProject::from_manifest_path(manifest_path)?;
    let main_path = project.main_path();
    if !main_path.exists() {
        return Err(format!("cool bundle: main file '{}' not found", project.main));
    }

    // Build the binary first
    let bin_path = Path::new(project.output_name());
    let source = fs::read_to_string(&main_path).map_err(|e| format!("cool bundle: {e}"))?;
    println!("  Compiling {} v{} ({})", project.name, project.version, project.main);
    let t0 = std::time::Instant::now();
    compile_to_native(&source, bin_path, &main_path)?;
    println!(
        "   Compiled in {:.2}s → {}",
        t0.elapsed().as_secs_f64(),
        bin_path.display()
    );

    // Assemble staging directory: dist/{name}-{version}-{target}/
    let target = target_triple();
    let artifact_name = format!("{}-{}-{}", project.name, project.version, target);
    let dist_dir = Path::new("dist");
    let staging = dist_dir.join(&artifact_name);
    let archive = dist_dir.join(format!("{artifact_name}.tar.gz"));

    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|e| format!("cool bundle: {e}"))?;
    }
    fs::create_dir_all(&staging).map_err(|e| format!("cool bundle: {e}"))?;

    // Copy binary
    let dst_bin = staging.join(project.output_name());
    fs::copy(bin_path, &dst_bin).map_err(|e| format!("cool bundle: cannot copy binary: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&dst_bin)
            .map_err(|e| format!("cool bundle: {e}"))?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&dst_bin, perms).map_err(|e| format!("cool bundle: {e}"))?;
    }

    // Copy declared include files/dirs
    for include in &project.bundle_include {
        let src = project.root.join(include);
        if !src.exists() {
            eprintln!("  warning: bundle include '{}' not found, skipping", include);
            continue;
        }
        let name = src
            .file_name()
            .ok_or_else(|| format!("bundle: invalid include path '{include}'"))?;
        copy_into(&src, &staging.join(name))?;
        println!("  Including {}", include);
    }

    // Create tarball via system tar
    let status = Command::new("tar")
        .args([
            "czf",
            archive.to_str().unwrap(),
            "-C",
            dist_dir.to_str().unwrap(),
            &artifact_name,
        ])
        .status()
        .map_err(|e| format!("cool bundle: tar failed: {e}"))?;
    if !status.success() {
        return Err("cool bundle: tar exited with error".to_string());
    }

    // Clean up staging directory
    fs::remove_dir_all(&staging).map_err(|e| format!("cool bundle: {e}"))?;

    println!("  Bundled  → {}", archive.display());
    Ok(())
}

// ── `cool release` ────────────────────────────────────────────────────────────

fn bump_version(version: &str, bump: &str) -> Result<String, String> {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() < 3 {
        return Err(format!("cool release: cannot parse version '{version}'"));
    }
    let major: u64 = parts[0]
        .parse()
        .map_err(|_| format!("cool release: invalid version '{version}'"))?;
    let minor: u64 = parts[1]
        .parse()
        .map_err(|_| format!("cool release: invalid version '{version}'"))?;
    let patch: u64 = parts[2]
        .parse()
        .map_err(|_| format!("cool release: invalid version '{version}'"))?;
    Ok(match bump {
        "major" => format!("{}.0.0", major + 1),
        "minor" => format!("{}.{}.0", major, minor + 1),
        "patch" => format!("{}.{}.{}", major, minor, patch + 1),
        explicit => explicit.to_string(),
    })
}

/// Rewrite the version field in cool.toml in place.
fn set_manifest_version(manifest_path: &Path, new_version: &str) -> Result<(), String> {
    let src = fs::read_to_string(manifest_path).map_err(|e| format!("cool release: {e}"))?;
    // Replace version = "..." under [project] or at top level — simple line-based rewrite.
    let mut found = false;
    let new_src: String = src
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if !found && trimmed.starts_with("version") {
                let eq_pos = trimmed.find('=');
                if let Some(eq) = eq_pos {
                    let after_eq = trimmed[eq + 1..].trim();
                    if after_eq.starts_with('"') {
                        found = true;
                        let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
                        return format!("{indent}version = \"{new_version}\"");
                    }
                }
            }
            line.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    // Preserve trailing newline if original had one
    let new_src = if src.ends_with('\n') {
        format!("{new_src}\n")
    } else {
        new_src
    };
    if !found {
        return Err("cool release: could not find 'version = ...' in cool.toml".to_string());
    }
    fs::write(manifest_path, new_src).map_err(|e| format!("cool release: {e}"))?;
    Ok(())
}

/// Build, bundle, and tag a release.
///
///   cool release [--bump patch|minor|major] [--version X.Y.Z] [--no-tag]
fn cmd_release(args: &[&String]) -> Result<(), String> {
    let manifest_path = Path::new("cool.toml");
    if !manifest_path.exists() {
        return Err("cool release: no cool.toml found in current directory".to_string());
    }
    let project = CoolProject::from_manifest_path(manifest_path)?;

    let mut bump = "patch";
    let mut explicit_version: Option<&str> = None;
    let mut no_tag = false;
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--bump" => {
                i += 1;
                bump = args
                    .get(i)
                    .map(|s| s.as_str())
                    .ok_or("cool release: --bump requires an argument (patch|minor|major)")?;
                if !matches!(bump, "patch" | "minor" | "major") {
                    return Err(format!(
                        "cool release: --bump must be patch, minor, or major, got '{bump}'"
                    ));
                }
            }
            "--version" => {
                i += 1;
                explicit_version = Some(
                    args.get(i)
                        .map(|s| s.as_str())
                        .ok_or("cool release: --version requires an argument")?,
                );
            }
            "--no-tag" => no_tag = true,
            other => return Err(format!("cool release: unexpected argument '{other}'")),
        }
        i += 1;
    }

    let new_version = if let Some(v) = explicit_version {
        v.to_string()
    } else {
        bump_version(&project.version, bump)?
    };

    // Warn if git working tree is dirty
    let git_clean = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .map(|o| o.stdout.is_empty())
        .unwrap_or(true);
    if !git_clean {
        eprintln!("  warning: working tree has uncommitted changes");
    }

    println!("  Releasing {} v{} → v{}", project.name, project.version, new_version);

    // Bump version in cool.toml
    set_manifest_version(manifest_path, &new_version)?;
    println!("  Updated  cool.toml version → {new_version}");

    // Build + bundle with the updated manifest
    let project = CoolProject::from_manifest_path(manifest_path)?;
    let main_path = project.main_path();
    let source = fs::read_to_string(&main_path).map_err(|e| format!("cool release: {e}"))?;
    let bin_path = Path::new(project.output_name());
    println!("  Compiling {} v{} ({})", project.name, project.version, project.main);
    let t0 = std::time::Instant::now();
    compile_to_native(&source, bin_path, &main_path)?;
    println!(
        "   Compiled in {:.2}s → {}",
        t0.elapsed().as_secs_f64(),
        bin_path.display()
    );

    // Bundle
    let target = target_triple();
    let artifact_name = format!("{}-{}-{}", project.name, project.version, target);
    let dist_dir = Path::new("dist");
    let staging = dist_dir.join(&artifact_name);
    let archive = dist_dir.join(format!("{artifact_name}.tar.gz"));
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|e| format!("cool release: {e}"))?;
    }
    fs::create_dir_all(&staging).map_err(|e| format!("cool release: {e}"))?;
    let dst_bin = staging.join(project.output_name());
    fs::copy(bin_path, &dst_bin).map_err(|e| format!("cool release: cannot copy binary: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&dst_bin)
            .map_err(|e| format!("cool release: {e}"))?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&dst_bin, perms).map_err(|e| format!("cool release: {e}"))?;
    }
    for include in &project.bundle_include {
        let src = project.root.join(include);
        if !src.exists() {
            continue;
        }
        let name = src
            .file_name()
            .ok_or_else(|| format!("release: invalid include '{include}'"))?;
        copy_into(&src, &staging.join(name))?;
    }
    let status = Command::new("tar")
        .args([
            "czf",
            archive.to_str().unwrap(),
            "-C",
            dist_dir.to_str().unwrap(),
            &artifact_name,
        ])
        .status()
        .map_err(|e| format!("cool release: tar failed: {e}"))?;
    if !status.success() {
        return Err("cool release: tar exited with error".to_string());
    }
    fs::remove_dir_all(&staging).map_err(|e| format!("cool release: {e}"))?;
    println!("  Bundled  → {}", archive.display());

    // Git tag
    if !no_tag {
        let tag = format!("v{new_version}");
        let tag_result = Command::new("git")
            .args(["tag", "-a", &tag, "-m", &format!("Release {tag}")])
            .status();
        match tag_result {
            Ok(s) if s.success() => println!("  Tagged   → {tag}"),
            Ok(_) => eprintln!("  warning: git tag failed (already exists or not a git repo)"),
            Err(_) => eprintln!("  warning: git not found, skipping tag"),
        }
    }

    println!();
    println!("  Released {} v{new_version}", project.name);
    println!("  Archive  → {}", archive.display());
    if !no_tag {
        println!("  Run 'git push --tags' to publish the tag.");
    }
    Ok(())
}

// ── help ──────────────────────────────────────────────────────────────────────

fn print_help() {
    println!(
        "\
Cool 1.0.0 — a native-first high-level systems language

USAGE:
    cool build                    Build the project described by cool.toml
    cool build <file.cool>        Compile a single file to a native binary
    cool build --freestanding     Build a freestanding object file (.o)
    cool --compile <file.cool>    Compile a file to a native binary (LLVM)
    cool <file.cool>              Run a file with the tree-walk interpreter
    cool --vm <file.cool>         Run a file with the bytecode VM
    cool                          Start the REPL
    cool ast <file.cool>          Print the parsed AST as JSON
    cool inspect <file.cool>      Print a JSON summary of top-level symbols
    cool symbols [file.cool]      Print a resolved JSON symbol index
    cool diff <before> <after>    Print a JSON summary of top-level changes
    cool check [file.cool]        Statically check imports and cycles
    cool modulegraph <file.cool>  Print the resolved import graph as JSON
    cool bundle                   Build and package the project into a distributable tarball
    cool release [--bump patch]   Bump version, bundle, and git-tag a release
    cool install                  Fetch and lock project dependencies
    cool add <name> ...           Add a path or git dependency to cool.toml
    cool test [path ...]          Run discovered or explicit Cool tests
    cool task [name|list ...]     Run or list manifest-defined project tasks
    cool new <name>               Scaffold a new Cool project
    cool lsp                      Start the language server (LSP) on stdin/stdout
    cool help                     Show this help message

FLAGS:
    --vm        Use the bytecode VM instead of the tree-walk interpreter
    --compile   Compile to a native binary using the LLVM backend

EXAMPLES:
    cool hello.cool               # interpret hello.cool
    cool build hello.cool         # compile hello.cool → ./hello (native binary)
    cool build --freestanding     # compile cool.toml project → ./name.o
    cool ast hello.cool           # dump the parsed AST as JSON
    cool inspect hello.cool       # summarize top-level symbols as JSON
    cool symbols hello.cool       # index resolved symbol locations as JSON
    cool diff old.cool new.cool   # compare top-level imports and symbols
    cool check hello.cool         # statically check imports and cycles
    cool modulegraph hello.cool   # resolve imports reachable from hello.cool
    cool add toolkit --path ../toolkit
    cool add theme --git https://github.com/acme/theme.git
    cool install                  # fetch git deps into .cool/deps and write cool.lock
    cool test                     # run test_*.cool / *_test.cool under tests/
    cool task list                # list tasks from cool.toml
    cool new myapp                # create myapp/ project
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
    built-in import math/os/sys/path/platform/csv/datetime/hashlib/toml/yaml/sqlite/http/subprocess/argparse/logging/test/time and import random
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
