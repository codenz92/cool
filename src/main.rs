mod argparse_runtime;
mod ast;
mod compiler;
mod interpreter;
mod lexer;
mod llvm_codegen;
mod opcode;
mod parser;
mod subprocess_runtime;
mod vm;

use interpreter::Interpreter;
use lexer::Lexer;
use parser::Parser;
use std::fs;
use std::path::{Path, PathBuf};

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

/// Minimal cool.toml parser (no external TOML dep — hand-rolled for the basic keys we need).
#[derive(Debug)]
struct CoolProject {
    name: String,
    version: String,
    main: String,
    output: Option<String>,
}

impl CoolProject {
    fn from_str(src: &str) -> Result<Self, String> {
        let mut name: Option<String> = None;
        let mut version: Option<String> = None;
        let mut main: Option<String> = None;
        let mut output: Option<String> = None;

        for (lineno, raw) in src.lines().enumerate() {
            let line = raw.trim();
            // Skip blank lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // Expect `key = "value"` or `key = value`
            let Some((k, v)) = line.split_once('=') else {
                return Err(format!("cool.toml line {}: expected `key = value`", lineno + 1));
            };
            let key = k.trim();
            let val = v.trim().trim_matches('"').to_string();
            match key {
                "name" => name = Some(val),
                "version" => version = Some(val),
                "main" => main = Some(val),
                "output" => output = Some(val),
                other => eprintln!("cool.toml: unknown key '{}' (ignored)", other),
            }
        }

        Ok(CoolProject {
            name: name.unwrap_or_else(|| "project".to_string()),
            version: version.unwrap_or_else(|| "0.1.0".to_string()),
            main: main.ok_or("cool.toml: missing required key 'main'")?,
            output,
        })
    }

    fn output_name(&self) -> &str {
        self.output.as_deref().unwrap_or(&self.name)
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

    // cool.toml
    let manifest = format!("name = \"{name}\"\nversion = \"0.1.0\"\nmain = \"src/main.cool\"\n");
    fs::write(project_dir.join("cool.toml"), manifest).map_err(|e| format!("cool new: {e}"))?;

    // src/main.cool
    let main_src = format!("# {name}\n\nprint(\"Hello from {name}!\")\n");
    fs::write(project_dir.join("src").join("main.cool"), main_src).map_err(|e| format!("cool new: {e}"))?;

    // .gitignore
    fs::write(project_dir.join(".gitignore"), format!("{name}\n*.o\n")).map_err(|e| format!("cool new: {e}"))?;

    println!("  Created project '{name}'");
    println!("  ├── cool.toml");
    println!("  ├── src/");
    println!("  │   └── main.cool");
    println!("  └── .gitignore");
    println!();
    println!("  Run your project:");
    println!("    cd {name}");
    println!("    cool src/main.cool          # interpret");
    println!("    cool build                  # compile to native");
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
    cool new <name>               Scaffold a new Cool project
    cool help                     Show this help message

FLAGS:
    --vm        Use the bytecode VM instead of the tree-walk interpreter
    --compile   Compile to a native binary using the LLVM backend

EXAMPLES:
    cool hello.cool               # interpret hello.cool
    cool build hello.cool         # compile hello.cool → ./hello (native binary)
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
    built-in import math/os/sys/path/subprocess/argparse/time and import random
    (seed/random/randint/uniform/choice/shuffle), plus json
    (loads/dumps), plus string
    (split/join/strip/lstrip/rstrip/upper/lower/replace/find/count/
    startswith/endswith/title/capitalize/format), plus list
    (sort/reverse/map/filter/reduce/flatten/unique), plus re
    (match/search/fullmatch/findall/sub/split), plus collections
    (Queue/Stack), subprocess(run/call/check_output), argparse(parse/help), open()/file methods, with/context managers on
    normal exit, return/break/continue, caught exceptions, and unhandled native raises,
    inline asm, and raw memory.
    Closures/lambdas, broader import support beyond those built-ins,
    and import ffi still require the interpreter or bytecode VM.
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
