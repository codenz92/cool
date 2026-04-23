use std::io::Read;
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub struct SubprocessResult {
    pub code: Option<i64>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

#[cfg(unix)]
fn shell_command(command: &str) -> Command {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(command);
    cmd
}

#[cfg(windows)]
fn shell_command(command: &str) -> Command {
    let mut cmd = Command::new("cmd");
    cmd.arg("/C").arg(command);
    cmd
}

fn spawn_reader<R: Read + Send + 'static>(mut reader: R) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = reader.read_to_end(&mut buf);
        buf
    })
}

fn status_code(status: ExitStatus) -> Option<i64> {
    status.code().map(i64::from)
}

pub fn run_shell_command(command: &str, timeout_secs: Option<f64>) -> Result<SubprocessResult, String> {
    let timeout_secs = timeout_secs.map(|secs| secs.max(0.0));
    let mut child = shell_command(command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;

    let stdout_handle = child.stdout.take().map(spawn_reader);
    let stderr_handle = child.stderr.take().map(spawn_reader);

    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break (status, false);
        }

        if let Some(timeout) = timeout_secs {
            if start.elapsed() >= Duration::from_secs_f64(timeout) {
                let _ = child.kill();
                let status = child.wait().map_err(|e| e.to_string())?;
                break (status, true);
            }
        }

        thread::sleep(Duration::from_millis(10));
    };

    let stdout = stdout_handle
        .map(|handle| String::from_utf8_lossy(&handle.join().unwrap_or_default()).to_string())
        .unwrap_or_default();
    let stderr = stderr_handle
        .map(|handle| String::from_utf8_lossy(&handle.join().unwrap_or_default()).to_string())
        .unwrap_or_default();

    Ok(SubprocessResult {
        code: status_code(status.0),
        stdout,
        stderr,
        timed_out: status.1,
    })
}
