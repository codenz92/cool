#[path = "../benchmark.rs"]
mod benchmark;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

struct Workload {
    name: &'static str,
}

struct Config {
    runs: usize,
    warmups: usize,
    filter: Option<String>,
}

struct WorkloadResult {
    workload: &'static Workload,
    cool_compile: Duration,
    rust_compile: Duration,
    cool_stats: benchmark::BenchStats,
    rust_stats: benchmark::BenchStats,
}

const WORKLOADS: &[Workload] = &[
    Workload { name: "integer_loop" },
    Workload {
        name: "string_processing",
    },
    Workload { name: "list_dict" },
    Workload { name: "raw_memory" },
];

fn main() {
    if let Err(err) = run() {
        eprintln!("bench_compare: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = parse_args(env::args().skip(1))?;
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let benchmarks_root = repo_root.join("benchmarks");
    let build_root = benchmarks_root.join("build");
    let cool_source_root = benchmarks_root.join("cool");
    let rust_source_root = benchmarks_root.join("rust");

    let workloads: Vec<&Workload> = WORKLOADS
        .iter()
        .filter(|workload| {
            config
                .filter
                .as_ref()
                .map(|filter| workload.name.contains(filter))
                .unwrap_or(true)
        })
        .collect();
    if workloads.is_empty() {
        return Err("no workloads matched --filter".to_string());
    }

    if build_root.exists() {
        fs::remove_dir_all(&build_root).map_err(|e| format!("remove {}: {e}", build_root.display()))?;
    }
    fs::create_dir_all(build_root.join("cool")).map_err(|e| format!("create build dir: {e}"))?;
    fs::create_dir_all(build_root.join("rust")).map_err(|e| format!("create build dir: {e}"))?;

    let cool_bin = build_cool_binary(&repo_root)?;

    println!(
        "Running {} workload(s) with {} warmup(s) and {} measured run(s)\n",
        workloads.len(),
        config.warmups,
        config.runs
    );

    let mut results = Vec::with_capacity(workloads.len());
    for workload in workloads {
        println!("== {} ==", workload.name);

        let copied_cool_source = build_root.join("cool").join(format!("{}.cool", workload.name));
        fs::copy(
            cool_source_root.join(format!("{}.cool", workload.name)),
            &copied_cool_source,
        )
        .map_err(|e| format!("copy {} benchmark: {e}", workload.name))?;
        let cool_binary = copied_cool_source.with_extension("");

        let cool_compile = benchmark::time_command(|| {
            let mut command = Command::new(&cool_bin);
            command.arg("build").arg(&copied_cool_source).current_dir(&repo_root);
            benchmark::run_command(command, format!("compile Cool {}", workload.name))
        })?;

        let rust_binary = build_root.join("rust").join(workload.name);
        let rust_compile = benchmark::time_command(|| {
            let mut command = Command::new("rustc");
            command
                .arg("-O")
                .arg("-C")
                .arg("target-cpu=native")
                .arg(rust_source_root.join(format!("{}.rs", workload.name)))
                .arg("-o")
                .arg(&rust_binary)
                .current_dir(&repo_root);
            benchmark::run_command(command, format!("compile Rust {}", workload.name))
        })?;

        verify_outputs(&cool_binary, &rust_binary)?;
        let cool_runs = benchmark::measure_binary_runs(&cool_binary, None, config.warmups, config.runs)?;
        let rust_runs = benchmark::measure_binary_runs(&rust_binary, None, config.warmups, config.runs)?;

        let cool_stats = benchmark::summarize(&cool_runs)?;
        let rust_stats = benchmark::summarize(&rust_runs)?;
        print_stats("Cool", &cool_stats);
        print_stats("Rust", &rust_stats);
        println!(
            "ratio: {:.2}x\n",
            cool_stats.mean.as_secs_f64() / rust_stats.mean.as_secs_f64()
        );

        results.push(WorkloadResult {
            workload,
            cool_compile,
            rust_compile,
            cool_stats,
            rust_stats,
        });
    }

    println!("Summary");
    println!(
        "{:<18} {:>12} {:>12} {:>12} {:>12} {:>10}",
        "workload", "cool mean", "rust mean", "cool comp", "rust comp", "ratio"
    );
    for result in &results {
        println!(
            "{:<18} {:>12} {:>12} {:>12} {:>12} {:>10.2}x",
            result.workload.name,
            benchmark::format_duration(result.cool_stats.mean),
            benchmark::format_duration(result.rust_stats.mean),
            benchmark::format_duration(result.cool_compile),
            benchmark::format_duration(result.rust_compile),
            result.cool_stats.mean.as_secs_f64() / result.rust_stats.mean.as_secs_f64(),
        );
    }

    Ok(())
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Config, String> {
    let mut runs = 5usize;
    let mut warmups = 1usize;
    let mut filter = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--runs" => {
                let value = args.next().ok_or_else(|| "--runs requires a value".to_string())?;
                runs = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid --runs value: {value}"))?;
            }
            "--warmups" => {
                let value = args.next().ok_or_else(|| "--warmups requires a value".to_string())?;
                warmups = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid --warmups value: {value}"))?;
            }
            "--filter" => {
                let value = args.next().ok_or_else(|| "--filter requires a value".to_string())?;
                filter = Some(value);
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            other => return Err(format!("unexpected argument: {other}")),
        }
    }

    if runs == 0 {
        return Err("--runs must be at least 1".to_string());
    }

    Ok(Config { runs, warmups, filter })
}

fn print_help() {
    println!("Usage: cargo run --release --bin bench_compare -- [--runs N] [--warmups N] [--filter NAME]");
}

fn build_cool_binary(repo_root: &Path) -> Result<PathBuf, String> {
    let cool_name = if cfg!(windows) { "cool.exe" } else { "cool" };
    let cool_bin = repo_root.join("target").join("release").join(cool_name);
    let mut command = Command::new("cargo");
    command
        .arg("build")
        .arg("--release")
        .arg("--bin")
        .arg("cool")
        .current_dir(repo_root);
    benchmark::run_command(command, "build cool release binary")?;
    Ok(cool_bin)
}

fn verify_outputs(cool_binary: &Path, rust_binary: &Path) -> Result<(), String> {
    let cool_output = benchmark::capture_binary_output(cool_binary, None)?;
    let rust_output = benchmark::capture_binary_output(rust_binary, None)?;
    if cool_output != rust_output {
        return Err(format!(
            "output mismatch for {} and {}:\nCool: {}\nRust: {}",
            cool_binary.display(),
            rust_binary.display(),
            cool_output.trim_end(),
            rust_output.trim_end(),
        ));
    }
    Ok(())
}

fn print_stats(label: &str, stats: &benchmark::BenchStats) {
    println!(
        "{label:<4} mean {:>10}  median {:>10}  min {:>10}",
        benchmark::format_duration(stats.mean),
        benchmark::format_duration(stats.median),
        benchmark::format_duration(stats.min),
    );
}
