mod lexer;
mod ast;
mod parser;
mod interpreter;

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
    match args.len() {
        1 => repl(),
        2 => {
            let path = &args[1];
            if !Path::new(path).exists() {
                eprintln!("cool: file not found: {}", path);
                std::process::exit(1);
            }
            let source = fs::read_to_string(path)
                .unwrap_or_else(|e| { eprintln!("cool: {}", e); std::process::exit(1); });
            let source_dir = Path::new(path).parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."));
            if let Err(e) = run_source(&source, source_dir) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        _ => {
            eprintln!("Usage: cool [file.cool]");
            std::process::exit(1);
        }
    }
}
