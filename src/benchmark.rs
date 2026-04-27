use std::path::Path;
use std::process::{Command, ExitStatus, Output, Stdio};
use std::time::{Duration, Instant};

pub struct BenchStats {
    pub min: Duration,
    pub median: Duration,
    pub mean: Duration,
}

pub fn capture_binary_output(binary: &Path, current_dir: Option<&Path>) -> Result<String, String> {
    let mut command = Command::new(binary);
    if let Some(dir) = current_dir {
        command.current_dir(dir);
    }
    let output = command.output().map_err(|e| format!("run {}: {e}", binary.display()))?;
    if !output.status.success() {
        return Err(format_command_failure(&format!("run {}", binary.display()), &output));
    }
    String::from_utf8(output.stdout).map_err(|e| format!("stdout from {} was not utf-8: {e}", binary.display()))
}

pub fn measure_binary_runs(
    binary: &Path,
    current_dir: Option<&Path>,
    warmups: usize,
    runs: usize,
) -> Result<Vec<Duration>, String> {
    for _ in 0..warmups {
        run_binary_silent(binary, current_dir)?;
    }

    let mut out = Vec::with_capacity(runs);
    for _ in 0..runs {
        let start = Instant::now();
        run_binary_silent(binary, current_dir)?;
        out.push(start.elapsed());
    }
    Ok(out)
}

pub fn summarize(samples: &[Duration]) -> Result<BenchStats, String> {
    if samples.is_empty() {
        return Err("no benchmark samples collected".to_string());
    }

    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let total_secs: f64 = samples.iter().map(Duration::as_secs_f64).sum();
    Ok(BenchStats {
        min: sorted[0],
        median: sorted[sorted.len() / 2],
        mean: Duration::from_secs_f64(total_secs / samples.len() as f64),
    })
}

pub fn format_duration(duration: Duration) -> String {
    let millis = duration.as_secs_f64() * 1000.0;
    if millis >= 1000.0 {
        format!("{:.2}s", duration.as_secs_f64())
    } else {
        format!("{millis:.1}ms")
    }
}

pub fn time_command<F>(f: F) -> Result<Duration, String>
where
    F: FnOnce() -> Result<(), String>,
{
    let start = Instant::now();
    f()?;
    Ok(start.elapsed())
}

#[allow(dead_code)]
pub fn run_command(mut command: Command, label: impl Into<String>) -> Result<(), String> {
    let label = label.into();
    let output = command.output().map_err(|e| format!("{label}: {e}"))?;
    if !output.status.success() {
        return Err(format_command_failure(&label, &output));
    }
    Ok(())
}

pub fn format_command_failure(label: &str, output: &Output) -> String {
    format!(
        "{label} failed with {}.\nstdout:\n{}\nstderr:\n{}",
        format_status(output.status),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}

fn run_binary_silent(binary: &Path, current_dir: Option<&Path>) -> Result<(), String> {
    let mut command = Command::new(binary);
    command.stdout(Stdio::null()).stderr(Stdio::null());
    if let Some(dir) = current_dir {
        command.current_dir(dir);
    }
    let status = command.status().map_err(|e| format!("run {}: {e}", binary.display()))?;
    if !status.success() {
        let mut rerun = Command::new(binary);
        if let Some(dir) = current_dir {
            rerun.current_dir(dir);
        }
        let output = rerun
            .output()
            .map_err(|e| format!("rerun {} after failure: {e}", binary.display()))?;
        return Err(format_command_failure(&format!("run {}", binary.display()), &output));
    }
    Ok(())
}

fn format_status(status: ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("exit code {code}"),
        None => "terminated by signal".to_string(),
    }
}
