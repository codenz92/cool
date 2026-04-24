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
mod opcode;
mod parser;
mod sqlite_runtime;
mod subprocess_runtime;
mod toml_runtime;
mod vm;
mod yaml_runtime;

use interpreter::Interpreter;
use lexer::Lexer;
use parser::Parser;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

// ── Runners ───────────────────────────────────────────────────────────────────

fn run_source(source: &str, source_dir: PathBuf) -> Result<(), String> {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize()?;

    let mut parser = Parser::new(tokens);
    let program = parser.parse_program()?;

    let mut interpreter = Interpreter::new(source_dir, source);
    interpreter.run(&program)
}

fn run_source_vm(source: &str, source_dir: PathBuf) -> Result<(), String> {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize()?;

    let mut parser = Parser::new(tokens);
    let program = parser.parse_program()?;

    let chunk = compiler::compile(&program)?;
    let mut machine = vm::VM::new(source_dir);
    machine.run(&chunk)
}

fn compile_to_native(source: &str, output_path: &Path, script_path: &Path) -> Result<(), String> {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize()?;

    let mut parser = Parser::new(tokens);
    let program = parser.parse_program()?;

    llvm_codegen::compile_program(&program, output_path, script_path)
}

// ── REPL ──────────────────────────────────────────────────────────────────────

fn repl() {
    use std::io::{self, BufRead, Write};
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    println!("Cool 0.2.0 — type 'exit' to quit");
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

// ── cool.toml project file ────────────────────────────────────────────────────

#[derive(Debug)]
struct CoolProject {
    name: String,
    version: String,
    main: String,
    output: Option<String>,
}

impl CoolProject {
    fn from_str(src: &str) -> Result<Self, String> {
        let parsed: toml::Value = src
            .parse()
            .map_err(|e: toml::de::Error| format!("cool.toml parse error: {}", e.message()))?;
        let root = parsed
            .as_table()
            .ok_or_else(|| "cool.toml: root must be a table".to_string())?;
        let project = root.get("project").and_then(toml::Value::as_table);

        let field =
            |key: &str| -> Option<&toml::Value> { project.and_then(|table| table.get(key)).or_else(|| root.get(key)) };
        let req_str = |key: &str| -> Result<Option<String>, String> {
            match field(key) {
                None => Ok(None),
                Some(toml::Value::String(s)) => Ok(Some(s.clone())),
                Some(other) => Err(format!(
                    "cool.toml: field '{}' must be a string, got {}",
                    key,
                    other.type_str()
                )),
            }
        };

        Ok(CoolProject {
            name: req_str("name")?.unwrap_or_else(|| "project".to_string()),
            version: req_str("version")?.unwrap_or_else(|| "0.1.0".to_string()),
            main: req_str("main")?.ok_or("cool.toml: missing required key 'main'")?,
            output: req_str("output")?,
        })
    }

    fn output_name(&self) -> &str {
        self.output.as_deref().unwrap_or(&self.name)
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
fn cmd_build(args: &[&String]) -> Result<(), String> {
    match args {
        // ── cool build  (no extra args) ───────────────────────────────────
        [] => {
            let manifest_path = Path::new("cool.toml");
            if !manifest_path.exists() {
                return Err("cool build: no cool.toml found in current directory.\n\
                     Create one or run `cool build <file.cool>` to compile a single file."
                    .to_string());
            }

            let manifest_src =
                fs::read_to_string(manifest_path).map_err(|e| format!("cool build: cannot read cool.toml: {e}"))?;
            let project = CoolProject::from_str(&manifest_src)?;

            let main_path = Path::new(&project.main);
            if !main_path.exists() {
                return Err(format!(
                    "cool build: main file '{}' not found (from cool.toml)",
                    project.main
                ));
            }

            let source = fs::read_to_string(main_path).map_err(|e| format!("cool build: {e}"))?;

            let output_path = Path::new(project.output_name());

            println!("  Compiling {} v{} ({})", project.name, project.version, project.main);

            let t0 = std::time::Instant::now();
            compile_to_native(&source, output_path, main_path)?;
            let elapsed = t0.elapsed();

            println!(
                "   Finished in {:.2}s → {}",
                elapsed.as_secs_f64(),
                output_path.display()
            );
            Ok(())
        }

        // ── cool build <file.cool>  ───────────────────────────────────────
        [file_arg] => {
            let file_path = Path::new(file_arg.as_str());
            if !file_path.exists() {
                return Err(format!("cool build: file not found: {}", file_arg));
            }
            if file_path.extension().and_then(|e| e.to_str()) != Some("cool") {
                eprintln!("cool build: warning: '{}' does not have a .cool extension", file_arg);
            }

            let source = fs::read_to_string(file_path).map_err(|e| format!("cool build: {e}"))?;

            // Output binary = file stem (e.g., hello.cool → ./hello)
            let output_path = file_path.with_extension("");

            println!("  Compiling {} ...", file_path.display());
            let t0 = std::time::Instant::now();
            compile_to_native(&source, &output_path, file_path)?;
            let elapsed = t0.elapsed();

            println!(
                "   Finished in {:.2}s → {}",
                elapsed.as_secs_f64(),
                output_path.display()
            );
            Ok(())
        }

        _ => Err("Usage: cool build [file.cool]".to_string()),
    }
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
    let manifest = format!("[project]\nname = \"{name}\"\nversion = \"0.1.0\"\nmain = \"src/main.cool\"\n");
    fs::write(project_dir.join("cool.toml"), manifest).map_err(|e| format!("cool new: {e}"))?;

    // src/main.cool
    let main_src = format!("# {name}\n\nprint(\"Hello from {name}!\")\n");
    fs::write(project_dir.join("src").join("main.cool"), main_src).map_err(|e| format!("cool new: {e}"))?;

    // tests/test_main.cool
    let test_src = "assert 1 + 1 == 2\n";
    fs::write(project_dir.join("tests").join("test_main.cool"), test_src).map_err(|e| format!("cool new: {e}"))?;

    // .gitignore
    fs::write(project_dir.join(".gitignore"), format!("{name}\n*.o\n")).map_err(|e| format!("cool new: {e}"))?;

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
    println!("    cool test                   # run tests/");
    Ok(())
}

// ── help ──────────────────────────────────────────────────────────────────────

fn print_help() {
    println!(
        "\
Cool 1.0.0 — a Python-inspired scripting language

USAGE:
    cool                          Start the REPL
    cool <file.cool>              Run a file with the tree-walk interpreter
    cool --vm <file.cool>         Run a file with the bytecode VM
    cool --compile <file.cool>    Compile a file to a native binary (LLVM)
    cool build                    Build the project described by cool.toml
    cool build <file.cool>        Compile a single file to a native binary
    cool test [path ...]          Run discovered or explicit Cool tests
    cool new <name>               Scaffold a new Cool project
    cool help                     Show this help message

FLAGS:
    --vm        Use the bytecode VM instead of the tree-walk interpreter
    --compile   Compile to a native binary using the LLVM backend

EXAMPLES:
    cool hello.cool               # interpret hello.cool
    cool build hello.cool         # compile hello.cool → ./hello (native binary)
    cool test                     # run test_*.cool / *_test.cool under tests/
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
    source-relative file imports and project/package imports,
    native import ffi (ffi.open/ffi.func),
    built-in import math/os/sys/path/csv/datetime/hashlib/toml/yaml/sqlite/http/subprocess/argparse/logging/test/time and import random
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
    write_i8/u8/i16/u16/i32/u32/i64 plus read/write_byte, read/write_f64, read/write_str).
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
            "test" => {
                let rest: Vec<&String> = args[2..].iter().collect();
                if let Err(e) = cmd_test(&rest) {
                    eprintln!("{e}");
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
