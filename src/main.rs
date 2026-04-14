mod lexer;
mod ast;
mod parser;
mod interpreter;
mod opcode;
mod compiler;
mod vm;

use std::fs;
use std::path::{Path, PathBuf};
use lexer::Lexer;
use parser::Parser;
use interpreter::Interpreter;

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

fn repl() {
    use std::io::{self, Write, BufRead};
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
        if line.trim().is_empty() { continue; }
        if let Err(e) = run_source(&line, cwd.clone()) {
            eprintln!("Error: {}", e);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Check for --vm flag anywhere in args.
    let use_vm = args.iter().any(|a| a == "--vm");
    let file_args: Vec<&String> = args[1..].iter().filter(|a| *a != "--vm").collect();

    match file_args.len() {
        0 => repl(),
        1 => {
            let path = file_args[0];
            if !Path::new(path).exists() {
                eprintln!("cool: file not found: {}", path);
                std::process::exit(1);
            }
            let source = fs::read_to_string(path)
                .unwrap_or_else(|e| { eprintln!("cool: {}", e); std::process::exit(1); });
            let source_dir = Path::new(path).parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."));
            let result = if use_vm {
                run_source_vm(&source, source_dir)
            } else {
                run_source(&source, source_dir)
            };
            if let Err(e) = result {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        _ => {
            eprintln!("Usage: cool [--vm] [file.cool]");
            std::process::exit(1);
        }
    }
}
