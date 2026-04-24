// Integration tests for Cool language interpreter
// Run with: cargo test --test integration

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_temp_path(stem: &str, ext: &str) -> std::path::PathBuf {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("{stem}_{nonce}_{seq}.{ext}"))
}

fn unique_temp_dir(stem: &str) -> std::path::PathBuf {
    unique_temp_path(stem, "dir")
}

fn cool_bin() -> &'static str {
    env!("CARGO_BIN_EXE_cool")
}

fn run_cool(source: &str) -> Result<String, String> {
    run_cool_with_args(source, &[])
}

fn run_cool_vm(source: &str) -> Result<String, String> {
    run_cool_with_args(source, &["--vm"])
}

fn run_cool_with_args_and_env(source: &str, extra_args: &[&str], envs: &[(&str, &str)]) -> Result<String, String> {
    let temp = unique_temp_path("temp_cool_test", "cool");
    let mut file = std::fs::File::create(&temp).map_err(|e| e.to_string())?;
    file.write_all(source.as_bytes()).map_err(|e| e.to_string())?;
    drop(file);

    let mut cmd = Command::new(cool_bin());
    for arg in extra_args {
        cmd.arg(arg);
    }
    for (key, value) in envs {
        cmd.env(key, value);
    }
    let output = cmd.arg(&temp).output().map_err(|e| e.to_string())?;

    let _ = std::fs::remove_file(&temp);

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        Ok(stdout)
    } else {
        Err(stderr)
    }
}

fn run_cool_with_args(source: &str, extra_args: &[&str]) -> Result<String, String> {
    run_cool_with_args_and_env(source, extra_args, &[])
}

fn run_cool_path_with_args(path: &std::path::Path, extra_args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new(cool_bin());
    for arg in extra_args {
        cmd.arg(arg);
    }
    let output = cmd.arg(path).output().map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        Ok(stdout)
    } else {
        Err(stderr)
    }
}

fn run_cool_path_with_program_args(
    path: &std::path::Path,
    leading_args: &[&str],
    program_args: &[&str],
) -> Result<String, String> {
    let mut cmd = Command::new(cool_bin());
    for arg in leading_args {
        cmd.arg(arg);
    }
    cmd.arg(path);
    for arg in program_args {
        cmd.arg(arg);
    }
    let output = cmd.output().map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        Ok(stdout)
    } else {
        Err(stderr)
    }
}

fn run_cool_stdin_with_args(path: &str, extra_args: &[&str], stdin: &str) -> Result<String, String> {
    let mut cmd = Command::new(cool_bin());
    cmd.arg(path);
    for arg in extra_args {
        cmd.arg(arg);
    }
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    {
        let mut child_stdin = child.stdin.take().ok_or_else(|| "missing stdin pipe".to_string())?;
        child_stdin.write_all(stdin.as_bytes()).map_err(|e| e.to_string())?;
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        Ok(stdout)
    } else {
        Err(stderr)
    }
}

fn run_cool_with_pty_input(path: &str, extra_args: &[&str], input: &[u8]) -> Result<(String, String, i32), String> {
    let mut cmd = Command::new("script");
    cmd.arg("-q").arg("/dev/null").arg(cool_bin()).arg(path);
    for arg in extra_args {
        cmd.arg(arg);
    }
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    {
        let mut child_stdin = child.stdin.take().ok_or_else(|| "missing stdin pipe".to_string())?;
        child_stdin.write_all(input).map_err(|e| e.to_string())?;
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    Ok((stdout, stderr, output.status.code().unwrap_or(-1)))
}

fn run_cool_with_pty_input_delayed_close(
    path: &str,
    extra_args: &[&str],
    input: &[u8],
    delay_ms: u64,
) -> Result<(String, String, i32), String> {
    let mut cmd = Command::new("script");
    cmd.arg("-q").arg("/dev/null").arg(cool_bin()).arg(path);
    for arg in extra_args {
        cmd.arg(arg);
    }
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    {
        let mut child_stdin = child.stdin.take().ok_or_else(|| "missing stdin pipe".to_string())?;
        child_stdin.write_all(input).map_err(|e| e.to_string())?;
        std::thread::sleep(Duration::from_millis(delay_ms));
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    Ok((stdout, stderr, output.status.code().unwrap_or(-1)))
}

fn run_cool_subcommand_in_dir(cwd: &std::path::Path, args: &[&str]) -> Result<(String, String, i32), String> {
    let output = Command::new(cool_bin())
        .current_dir(cwd)
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    Ok((
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    ))
}

fn run_git_in_dir(cwd: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(cwd)
        .env("GIT_AUTHOR_NAME", "Cool Test")
        .env("GIT_AUTHOR_EMAIL", "cool-test@example.com")
        .env("GIT_COMMITTER_NAME", "Cool Test")
        .env("GIT_COMMITTER_EMAIL", "cool-test@example.com")
        .args(args)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{stdout}\nstderr:\n{stderr}",
        args
    );
    stdout
}

fn write_git_dependency_repo(root: &std::path::Path, value: i64) -> String {
    let _ = std::fs::remove_dir_all(root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("cool.toml"),
        r#"[project]
name = "toolkit"
version = "0.3.0"
main = "src/main.cool"
"#,
    )
    .unwrap();
    std::fs::write(root.join("src").join("main.cool"), "print(\"toolkit\")\n").unwrap();
    std::fs::write(root.join("src").join("util.cool"), format!("value = {value}\n")).unwrap();

    run_git_in_dir(root, &["init"]);
    run_git_in_dir(root, &["add", "."]);
    run_git_in_dir(root, &["commit", "-m", "init"]);
    run_git_in_dir(root, &["rev-parse", "HEAD"]).trim().to_string()
}

fn write_project_with_sources_and_dependencies(root: &std::path::Path) {
    let _ = std::fs::remove_dir_all(root);
    std::fs::create_dir_all(root.join("app")).unwrap();
    std::fs::create_dir_all(root.join("lib")).unwrap();
    std::fs::create_dir_all(root.join("deps").join("toolkit").join("src")).unwrap();
    std::fs::create_dir_all(root.join("deps").join("extra").join("src")).unwrap();

    std::fs::write(
        root.join("cool.toml"),
        r#"[project]
name = "demo"
version = "0.1.0"
main = "app/main.cool"
sources = ["app", "lib"]

[dependencies]
toolkit = { path = "deps/toolkit" }
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("app").join("main.cool"),
        "import helper\nimport toolkit.util\nprint(helper.value)\nprint(util.value)\n",
    )
    .unwrap();
    std::fs::write(root.join("lib").join("helper.cool"), "value = 7\n").unwrap();

    std::fs::write(
        root.join("deps").join("toolkit").join("cool.toml"),
        r#"[project]
name = "toolkit"
version = "0.2.0"
main = "src/main.cool"
sources = ["src"]

[dependencies]
extra = { path = "../extra" }
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("deps").join("toolkit").join("src").join("main.cool"),
        "value = 0\n",
    )
    .unwrap();
    std::fs::write(
        root.join("deps").join("toolkit").join("src").join("util.cool"),
        "import extra.more\nvalue = more.value + 1\n",
    )
    .unwrap();

    std::fs::write(
        root.join("deps").join("extra").join("cool.toml"),
        r#"[project]
name = "extra"
version = "0.1.0"
main = "src/main.cool"
sources = ["src"]
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("deps").join("extra").join("src").join("main.cool"),
        "value = 0\n",
    )
    .unwrap();
    std::fs::write(
        root.join("deps").join("extra").join("src").join("more.cool"),
        "value = 8\n",
    )
    .unwrap();
}

fn write_task_project(root: &std::path::Path) {
    let _ = std::fs::remove_dir_all(root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("scripts")).unwrap();

    std::fs::write(
        root.join("cool.toml"),
        r#"[project]
name = "demo"
version = "0.1.0"
main = "src/main.cool"

[tasks.prepare]
description = "Prepare output"
run = "printf 'prep\n'"

[tasks.hello]
description = "Say hello"
deps = ["prepare"]
run = ["printf 'hello %s\n' {args}", "printf 'done\n'"]

[tasks.cwd]
description = "Show task cwd"
cwd = "scripts"
run = "pwd"
"#,
    )
    .unwrap();
    std::fs::write(root.join("src").join("main.cool"), "print(\"demo\")\n").unwrap();
}

fn write_basic_project(root: &std::path::Path, name: &str, source: &str) {
    let _ = std::fs::remove_dir_all(root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("cool.toml"),
        format!("[project]\nname = \"{name}\"\nversion = \"0.1.0\"\nmain = \"src/main.cool\"\n"),
    )
    .unwrap();
    std::fs::write(root.join("src").join("main.cool"), source).unwrap();
}

fn assert_logging_file_output(contents: &str) {
    let lines: Vec<&str> = contents.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines.len(), 3);
    assert!(lines[0].chars().next().unwrap_or_default().is_ascii_digit());
    assert!(lines[0].contains("|INFO|demo|shown"));
    assert!(lines[1].contains("|WARNING|demo|warned"));
    assert!(lines[2].contains("|ERROR|demo|boom"));
    assert!(!contents.contains("hidden"));
}

fn parse_content_length(request: &str) -> usize {
    request
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.eq_ignore_ascii_case("Content-Length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0)
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|window| window == b"\r\n\r\n")
}

fn read_http_request(stream: &mut TcpStream) -> String {
    stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if let Some(header_end) = find_header_end(&buf) {
                    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
                    let content_length = parse_content_length(&head);
                    if buf.len() >= header_end + 4 + content_length {
                        break;
                    }
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock || err.kind() == std::io::ErrorKind::TimedOut => {
                break
            }
            Err(err) => panic!("failed to read test HTTP request: {err}"),
        }
    }
    String::from_utf8_lossy(&buf).to_string()
}

fn spawn_http_test_server(expected_requests: usize) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut handled = 0usize;
        while handled < expected_requests && Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    // Accepted sockets can inherit nonblocking mode from the listener on some
                    // platforms; switch back to blocking reads so we don't parse partial requests.
                    stream.set_nonblocking(false).unwrap();
                    let request = read_http_request(&mut stream);
                    let mut lines = request.lines();
                    let request_line = lines.next().unwrap_or("");
                    let mut parts = request_line.split_whitespace();
                    let method = parts.next().unwrap_or("");
                    let path = parts.next().unwrap_or("");
                    let lower = request.to_ascii_lowercase();
                    let body = request
                        .split_once("\r\n\r\n")
                        .map(|(_, body)| body.to_string())
                        .unwrap_or_default();
                    let has_test_header = lower.contains("x-test: yes");
                    let has_accept_json = lower.contains("accept: application/json");

                    let (status, content_type, extra_headers, body_text) = match method {
                        "GET" if path.ends_with("/plain") => (
                            "200 OK",
                            "text/plain",
                            "X-Reply: plain\r\n",
                            format!("hello header={}\n", if has_test_header { "yes" } else { "no" }),
                        ),
                        "HEAD" if path.ends_with("/plain") => {
                            ("200 OK", "text/plain", "X-Reply: plain\r\n", String::new())
                        }
                        "GET" if path.ends_with("/json") => (
                            "200 OK",
                            "application/json",
                            "",
                            format!(
                                "{{\"ok\":true,\"n\":2,\"accept\":{}}}\n",
                                if has_accept_json { "true" } else { "false" }
                            ),
                        ),
                        "POST" if path.ends_with("/echo") => (
                            "200 OK",
                            "text/plain",
                            "",
                            format!("{}|header={}", body, if has_test_header { "yes" } else { "no" }),
                        ),
                        _ => ("404 Not Found", "text/plain", "", "not found".to_string()),
                    };

                    let response = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n{extra_headers}\r\n{}",
                        body_text.len(),
                        body_text
                    );
                    stream.write_all(response.as_bytes()).unwrap();
                    handled += 1;
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("HTTP test server accept failed: {err}"),
            }
        }
        assert_eq!(
            handled, expected_requests,
            "HTTP test server handled only {handled}/{expected_requests} requests"
        );
    });
    (base_url, handle)
}

#[test]
fn test_hello_world() {
    let result = run_cool("print(\"Hello, World!\")").unwrap();
    assert!(result.contains("Hello, World!"));
}

#[test]
fn test_variables() {
    let result = run_cool("x = 10\nprint(x)").unwrap();
    assert!(result.contains("10"));
}

#[test]
fn test_arithmetic() {
    let result = run_cool("print(2 + 3 * 4)").unwrap();
    assert!(result.contains("14"));
}

#[test]
fn test_arithmetic_float() {
    let result = run_cool("print(10.5 + 2.5)").unwrap();
    assert!(result.contains("13"));
}

#[test]
fn test_sum_tuple() {
    let result = run_cool("print(sum((1, 2, 3)))").unwrap();
    assert!(result.contains("6"));
}

#[test]
fn test_fixed_width_int_builtins() {
    let result = run_cool(
        "print(i8(255))\nprint(u8(-1))\nprint(i16(65535))\nprint(u16(-1))\nprint(i32(4294967295))\nprint(u32(-1))\nprint(i64(42.9))",
    )
    .unwrap();
    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["-1", "255", "-1", "65535", "-1", "4294967295", "42"]);
}

#[test]
fn test_vm_fixed_width_int_builtins() {
    let result = run_cool_vm(
        "print(i8(255))\nprint(u8(-1))\nprint(i16(65535))\nprint(u16(-1))\nprint(i32(4294967295))\nprint(u32(-1))\nprint(i64(42.9))",
    )
    .unwrap();
    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["-1", "255", "-1", "65535", "-1", "4294967295", "42"]);
}

#[test]
fn test_if_statement() {
    // Single-line ternary
    let result = run_cool("x = 5\nprint(\"big\" if x > 3 else \"small\")").unwrap();
    assert!(result.contains("big"));
}

#[test]
fn test_while_loop() {
    let result = run_cool("i = 0\nwhile i < 3:\n\tprint(i)\n\ti = i + 1").unwrap();
    assert!(result.contains("0"));
    assert!(result.contains("1"));
    assert!(result.contains("2"));
}

#[test]
fn test_while_loop_basic() {
    let result = run_cool("count = 0\nwhile count < 5:\n\tcount = count + 1\nprint(count)").unwrap();
    assert!(result.contains("5"));
}

#[test]
fn test_for_loop() {
    let result = run_cool("for i in range(3):\n\tprint(i)").unwrap();
    assert!(result.contains("0"));
    assert!(result.contains("1"));
    assert!(result.contains("2"));
}

#[test]
fn test_list() {
    let result = run_cool("lst = [1, 2, 3]\nprint(len(lst))").unwrap();
    assert!(result.contains("3"));
}

#[test]
fn test_list_comprehension() {
    let result = run_cool("squares = [x * x for x in range(5)]\nprint(squares)").unwrap();
    assert!(result.contains("16")); // 4*4 is in the list
}

#[test]
fn test_function() {
    let result = run_cool("def add(a, b):\n\treturn a + b\nprint(add(3, 4))").unwrap();
    assert!(result.contains("7"));
}

#[test]
fn test_function_default_args() {
    let result =
        run_cool("def greet(name, greeting=\"Hello\"):\n\treturn greeting + \", \" + name\nprint(greet(\"World\"))")
            .unwrap();
    assert!(result.contains("Hello, World"));
}

#[test]
fn test_class() {
    let result = run_cool("class Dog:\n\tdef __init__(self, name):\n\t\tself.name = name\n\tdef speak(self):\n\t\treturn self.name + \" says woof!\"\ndog = Dog(\"Rex\")\nprint(dog.speak())").unwrap();
    assert!(result.contains("Rex says woof!"));
}

#[test]
fn test_inheritance() {
    let result = run_cool("class Animal:\n\tdef __init__(self, name):\n\t\tself.name = name\nclass Dog(Animal):\n\tdef speak(self):\n\t\treturn self.name + \" says woof!\"\ndog = Dog(\"Rex\")\nprint(dog.speak())").unwrap();
    assert!(result.contains("Rex says woof!"));
}

#[test]
fn test_string_methods() {
    let result = run_cool("s = \"  Hello World  \"\nprint(s.strip().upper())").unwrap();
    assert!(result.contains("HELLO WORLD"));
}

#[test]
fn test_fstring() {
    let result = run_cool("name = \"Cool\"\nprint(f\"Hello, {name}!\")").unwrap();
    assert!(result.contains("Hello, Cool!"));
}

#[test]
fn test_dict() {
    let result = run_cool("d = {\"a\": 1, \"b\": 2}\nprint(d[\"a\"])").unwrap();
    assert!(result.contains("1"));
}

#[test]
fn test_dict_copy() {
    let result = run_cool(
        "d = {\"a\": 1}\nc = d.copy()\nd[\"a\"] = 2\nc[\"b\"] = 3\nprint(d[\"a\"])\nprint(c[\"a\"])\nprint(\"b\" in d)\nprint(\"b\" in c)",
    )
    .unwrap();
    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["2", "1", "false", "true"]);
}

#[test]
fn test_vm_dict_copy() {
    let result = run_cool_vm(
        "d = {\"a\": 1}\nc = d.copy()\nd[\"a\"] = 2\nc[\"b\"] = 3\nprint(d[\"a\"])\nprint(c[\"a\"])\nprint(\"b\" in d)\nprint(\"b\" in c)",
    )
    .unwrap();
    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["2", "1", "false", "true"]);
}

#[test]
fn test_tuple_unpacking() {
    let result = run_cool("t = (1, 2)\na = t[0]\nb = t[1]\nprint(a)\nprint(b)").unwrap();
    assert!(result.contains("1"));
    assert!(result.contains("2"));
}

#[test]
fn test_try_except() {
    let result = run_cool("try:\n\tx = 1 / 0\nexcept:\n\tprint(\"caught\")").unwrap();
    assert!(result.contains("caught"));
}

#[test]
fn test_lambda() {
    let result = run_cool("f = lambda x: x * 2\nprint(f(5))").unwrap();
    assert!(result.contains("10"));
}

#[test]
fn test_closure() {
    let result = run_cool(
        "def make_adder(n):\n\tdef adder(x):\n\t\treturn x + n\n\treturn adder\nadd5 = make_adder(5)\nprint(add5(10))",
    )
    .unwrap();
    assert!(result.contains("15"));
}

#[test]
fn test_super() {
    let result = run_cool("class Animal:\n\tdef speak(self):\n\t\treturn \"...\"\nclass Dog(Animal):\n\tdef speak(self):\n\t\treturn \"woof!\"\ndog = Dog()\nprint(dog.speak())").unwrap();
    assert!(result.contains("woof!"));
}

#[test]
fn test_import() {
    let result =
        run_cool("import math\nprint(math.sqrt(4))\nprint(math.round(3.5))\nprint(math.abs(-7))\nprint(math.log(100, 10))\nimport os\nprint(os.path(\"a\", \"b\"))")
            .unwrap();
    assert!(result.contains("2"));
    assert!(result.contains("4"));
    assert!(result.contains("7"));
    assert!(result.contains("a/b"));
    assert!(result.matches("\n2\n").count() >= 2 || result.contains("\n2.0\n"));
}

#[test]
fn test_vm_import_math_module() {
    let result =
        run_cool_vm("import math\nprint(math.round(3.5))\nprint(math.round(3.14159, 2))\nprint(math.abs(-7))").unwrap();
    assert!(result.contains("4"));
    assert!(result.contains("3.14"));
    assert!(result.contains("7"));
}

#[test]
fn test_import_subprocess_module() {
    let result = run_cool(
        "import subprocess\nres = subprocess.run(\"printf 'out'; printf 'err' 1>&2; exit 7\")\nprint(res[\"code\"])\nprint(res[\"stdout\"])\nprint(res[\"stderr\"])\nprint(res[\"timed_out\"])\nprint(res[\"ok\"])\nprint(subprocess.call(\"exit 3\"))\nprint(subprocess.check_output(\"printf 'hi'\"))",
    )
    .unwrap();
    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["7", "out", "err", "false", "false", "3", "hi"]);
}

#[test]
fn test_import_subprocess_timeout() {
    let result = run_cool("import subprocess\nres = subprocess.run(\"sleep 1\", 0.05)\nprint(res[\"timed_out\"])\nprint(res[\"code\"] == nil)\nprint(res[\"ok\"])").unwrap();
    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["true", "true", "false"]);
}

#[test]
fn test_vm_import_subprocess_module() {
    let result = run_cool_vm(
        "import subprocess\nres = subprocess.run(\"printf 'out'; printf 'err' 1>&2; exit 7\")\nprint(res[\"code\"])\nprint(res[\"stdout\"])\nprint(res[\"stderr\"])\nprint(res[\"timed_out\"])\nprint(res[\"ok\"])\nprint(subprocess.call(\"exit 3\"))\nprint(subprocess.check_output(\"printf 'hi'\"))",
    )
    .unwrap();
    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["7", "out", "err", "false", "false", "3", "hi"]);
}

#[test]
fn test_vm_import_subprocess_timeout() {
    let result =
        run_cool_vm("import subprocess\nres = subprocess.run(\"sleep 1\", 0.05)\nprint(res[\"timed_out\"])\nprint(res[\"code\"] == nil)\nprint(res[\"ok\"])").unwrap();
    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["true", "true", "false"]);
}

#[test]
fn test_import_argparse_module() {
    let result = run_cool(
        r#"import argparse
spec = {
    "prog": "serve",
    "description": "Serve static files",
    "positionals": [
        {"name": "root", "help": "root directory"}
    ],
    "options": [
        {"name": "port", "short": "p", "type": "int", "default": 8000, "help": "listen port"},
        {"name": "host", "type": "str", "default": "127.0.0.1", "help": "bind host"},
        {"name": "verbose", "short": "v", "type": "bool", "help": "verbose output"}
    ]
}
args = argparse.parse(spec, ["site", "-v", "--port", "9000", "--host=0.0.0.0"])
print(args["root"])
print(args["verbose"])
print(args["port"])
print(args["host"])
print(argparse.help(spec))
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().collect();
    assert_eq!(lines[0..4], ["site", "true", "9000", "0.0.0.0"]);
    assert!(result.contains("Usage: serve [--port PORT] [--host HOST] [--verbose] ROOT"));
    assert!(result.contains("Serve static files"));
    assert!(result.contains("-p, --port PORT"));
    assert!(result.contains("(default: 8000)"));
}

#[test]
fn test_vm_import_argparse_module() {
    let result = run_cool_vm(
        r#"import argparse
spec = {
    "prog": "serve",
    "description": "Serve static files",
    "positionals": [
        {"name": "root", "help": "root directory"}
    ],
    "options": [
        {"name": "port", "short": "p", "type": "int", "default": 8000, "help": "listen port"},
        {"name": "host", "type": "str", "default": "127.0.0.1", "help": "bind host"},
        {"name": "verbose", "short": "v", "type": "bool", "help": "verbose output"}
    ]
}
args = argparse.parse(spec, ["site", "-v", "--port", "9000", "--host=0.0.0.0"])
print(args["root"])
print(args["verbose"])
print(args["port"])
print(args["host"])
print(argparse.help(spec))
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().collect();
    assert_eq!(lines[0..4], ["site", "true", "9000", "0.0.0.0"]);
    assert!(result.contains("Usage: serve [--port PORT] [--host HOST] [--verbose] ROOT"));
    assert!(result.contains("Serve static files"));
    assert!(result.contains("-p, --port PORT"));
    assert!(result.contains("(default: 8000)"));
}

#[test]
fn test_argparse_uses_process_args_by_default() {
    let source_path = unique_temp_path("cool_argparse_process_args", "cool");
    std::fs::write(
        &source_path,
        "import argparse\nspec = {\n    \"positionals\": [{\"name\": \"action\"}],\n    \"options\": [{\"name\": \"count\", \"short\": \"c\", \"type\": \"int\", \"default\": 1}]\n}\nargs = argparse.parse(spec)\nprint(args[\"action\"])\nprint(args[\"count\"])\n",
    )
    .unwrap();

    let result = run_cool_path_with_program_args(&source_path, &[], &["deploy", "-c", "3"]).unwrap();
    let _ = std::fs::remove_file(&source_path);
    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["deploy", "3"]);
}

#[test]
fn test_vm_argparse_uses_process_args_by_default() {
    let source_path = unique_temp_path("cool_vm_argparse_process_args", "cool");
    std::fs::write(
        &source_path,
        "import argparse\nspec = {\n    \"positionals\": [{\"name\": \"action\"}],\n    \"options\": [{\"name\": \"count\", \"short\": \"c\", \"type\": \"int\", \"default\": 1}]\n}\nargs = argparse.parse(spec)\nprint(args[\"action\"])\nprint(args[\"count\"])\n",
    )
    .unwrap();

    let result = run_cool_path_with_program_args(&source_path, &["--vm"], &["deploy", "-c", "3"]).unwrap();
    let _ = std::fs::remove_file(&source_path);
    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["deploy", "3"]);
}

#[test]
fn test_import_csv_module() {
    let result = run_cool(
        r#"import csv
text = "name,city,quote\nAlice,\"New York, NY\",\"She said \"\"hi\"\"\"\nBob,Paris,\n"
rows = csv.rows(text)
print(rows[1][1])
print(rows[1][2])
dicts = csv.dicts(text)
print(dicts[0]["city"])
print(dicts[1]["quote"] == "")
rendered = csv.write(dicts)
print("name,city,quote" in rendered)
print("\"New York, NY\"" in rendered)
print("\"She said \"\"hi\"\"\"" in rendered)
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(
        lines,
        [
            "New York, NY",
            "She said \"hi\"",
            "New York, NY",
            "true",
            "true",
            "true",
            "true",
        ]
    );
}

#[test]
fn test_vm_import_csv_module() {
    let result = run_cool_vm(
        r#"import csv
text = "name,city,quote\nAlice,\"New York, NY\",\"She said \"\"hi\"\"\"\nBob,Paris,\n"
rows = csv.rows(text)
print(rows[1][1])
print(rows[1][2])
dicts = csv.dicts(text)
print(dicts[0]["city"])
print(dicts[1]["quote"] == "")
rendered = csv.write(dicts)
print("name,city,quote" in rendered)
print("\"New York, NY\"" in rendered)
print("\"She said \"\"hi\"\"\"" in rendered)
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(
        lines,
        [
            "New York, NY",
            "She said \"hi\"",
            "New York, NY",
            "true",
            "true",
            "true",
            "true",
        ]
    );
}

#[test]
fn test_import_datetime_module() {
    let result = run_cool(
        r#"import datetime
print(datetime.now() > 1000000000)
ts = datetime.parse("2024-01-02 03:04:05")
print(datetime.format(ts))
parts = datetime.parts(ts)
print(parts["year"])
print(parts["month"])
print(parts["day"])
print(parts["hour"])
print(parts["minute"])
print(parts["second"])
print(parts["weekday"])
print(parts["yearday"])
shifted = datetime.add_seconds(ts, 90)
print(datetime.format(shifted))
print(datetime.diff_seconds(shifted, ts) == 90)
print(datetime.format(datetime.parse("2024/05/06", "%Y/%m/%d"), "%Y/%m/%d"))
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(
        lines,
        [
            "true",
            "2024-01-02 03:04:05",
            "2024",
            "1",
            "2",
            "3",
            "4",
            "5",
            "2",
            "2",
            "2024-01-02 03:05:35",
            "true",
            "2024/05/06",
        ]
    );
}

#[test]
fn test_vm_import_datetime_module() {
    let result = run_cool_vm(
        r#"import datetime
print(datetime.now() > 1000000000)
ts = datetime.parse("2024-01-02 03:04:05")
print(datetime.format(ts))
parts = datetime.parts(ts)
print(parts["year"])
print(parts["month"])
print(parts["day"])
print(parts["hour"])
print(parts["minute"])
print(parts["second"])
print(parts["weekday"])
print(parts["yearday"])
shifted = datetime.add_seconds(ts, 90)
print(datetime.format(shifted))
print(datetime.diff_seconds(shifted, ts) == 90)
print(datetime.format(datetime.parse("2024/05/06", "%Y/%m/%d"), "%Y/%m/%d"))
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(
        lines,
        [
            "true",
            "2024-01-02 03:04:05",
            "2024",
            "1",
            "2",
            "3",
            "4",
            "5",
            "2",
            "2",
            "2024-01-02 03:05:35",
            "true",
            "2024/05/06",
        ]
    );
}

#[test]
fn test_import_hashlib_module() {
    let result = run_cool(
        r#"import hashlib
print(hashlib.md5("abc"))
print(hashlib.sha1("abc"))
print(hashlib.sha256("abc"))
print(hashlib.digest("sha256", "abc"))
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(
        lines,
        [
            "900150983cd24fb0d6963f7d28e17f72",
            "a9993e364706816aba3e25717850c26c9cd0d89d",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ]
    );
}

#[test]
fn test_vm_import_hashlib_module() {
    let result = run_cool_vm(
        r#"import hashlib
print(hashlib.md5("abc"))
print(hashlib.sha1("abc"))
print(hashlib.sha256("abc"))
print(hashlib.digest("sha256", "abc"))
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(
        lines,
        [
            "900150983cd24fb0d6963f7d28e17f72",
            "a9993e364706816aba3e25717850c26c9cd0d89d",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ]
    );
}

#[test]
fn test_import_toml_module() {
    let result = run_cool(
        r#"import toml
text = "title = \"cool\"\nports = [8000, 8001]\nrelease = 1.5\n[server]\nhost = \"127.0.0.1\"\ndebug = true\n"
data = toml.loads(text)
print(data["title"])
print(data["ports"][1])
print(data["release"])
print(data["server"]["host"])
print(data["server"]["debug"])
rendered = toml.dumps({
    "title": "cool",
    "ports": [8000, 8001],
    "server": {
        "host": "127.0.0.1",
        "debug": true
    }
})
print("title = \"cool\"" in rendered)
print("ports = [8000, 8001]" in rendered or "ports = [8000,8001]" in rendered)
print("[server]" in rendered)
print("host = \"127.0.0.1\"" in rendered)
print("debug = true" in rendered)
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(
        lines,
        [
            "cool",
            "8001",
            "1.5",
            "127.0.0.1",
            "true",
            "true",
            "true",
            "true",
            "true",
            "true",
        ]
    );
}

#[test]
fn test_vm_import_toml_module() {
    let result = run_cool_vm(
        r#"import toml
text = "title = \"cool\"\nports = [8000, 8001]\nrelease = 1.5\n[server]\nhost = \"127.0.0.1\"\ndebug = true\n"
data = toml.loads(text)
print(data["title"])
print(data["ports"][1])
print(data["release"])
print(data["server"]["host"])
print(data["server"]["debug"])
rendered = toml.dumps({
    "title": "cool",
    "ports": [8000, 8001],
    "server": {
        "host": "127.0.0.1",
        "debug": true
    }
})
print("title = \"cool\"" in rendered)
print("ports = [8000, 8001]" in rendered or "ports = [8000,8001]" in rendered)
print("[server]" in rendered)
print("host = \"127.0.0.1\"" in rendered)
print("debug = true" in rendered)
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(
        lines,
        [
            "cool",
            "8001",
            "1.5",
            "127.0.0.1",
            "true",
            "true",
            "true",
            "true",
            "true",
            "true",
        ]
    );
}

#[test]
fn test_import_yaml_module() {
    let result = run_cool(
        r#"import yaml
text = "name: cool\nenabled: true\nports:\n  - 8000\n  - 8001\nservice:\n  host: 127.0.0.1\n  retries: 3\nnote: null\n"
data = yaml.loads(text)
print(data["name"])
print(data["enabled"])
print(data["ports"][1])
print(data["service"]["host"])
print(data["service"]["retries"])
print(data["note"] == nil)
rendered = yaml.dumps({
    "name": "cool",
    "enabled": true,
    "ports": [8000, 8001],
    "service": {
        "host": "127.0.0.1",
        "retries": 3
    },
    "note": nil
})
print("name: cool" in rendered)
print("enabled: true" in rendered)
print("ports:" in rendered)
print("- 8000" in rendered)
print("service:" in rendered)
print("host: 127.0.0.1" in rendered)
print("note: null" in rendered)
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(
        lines,
        [
            "cool",
            "true",
            "8001",
            "127.0.0.1",
            "3",
            "true",
            "true",
            "true",
            "true",
            "true",
            "true",
            "true",
            "true",
        ]
    );
}

#[test]
fn test_vm_import_yaml_module() {
    let result = run_cool_vm(
        r#"import yaml
text = "name: cool\nenabled: true\nports:\n  - 8000\n  - 8001\nservice:\n  host: 127.0.0.1\n  retries: 3\nnote: null\n"
data = yaml.loads(text)
print(data["name"])
print(data["enabled"])
print(data["ports"][1])
print(data["service"]["host"])
print(data["service"]["retries"])
print(data["note"] == nil)
rendered = yaml.dumps({
    "name": "cool",
    "enabled": true,
    "ports": [8000, 8001],
    "service": {
        "host": "127.0.0.1",
        "retries": 3
    },
    "note": nil
})
print("name: cool" in rendered)
print("enabled: true" in rendered)
print("ports:" in rendered)
print("- 8000" in rendered)
print("service:" in rendered)
print("host: 127.0.0.1" in rendered)
print("note: null" in rendered)
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(
        lines,
        [
            "cool",
            "true",
            "8001",
            "127.0.0.1",
            "3",
            "true",
            "true",
            "true",
            "true",
            "true",
            "true",
            "true",
            "true",
        ]
    );
}

#[test]
fn test_import_sqlite_module() {
    let db_path = unique_temp_path("cool_sqlite_module_test", "db");
    let source = format!(
        r#"import sqlite
db = "{db}"
print(sqlite.execute(db, "create table items (id integer primary key, name text, score real, active integer)") == 0)
print(sqlite.execute(db, "insert into items (name, score, active) values (?, ?, ?)", ["alpha", 1.5, true]))
print(sqlite.execute(db, "insert into items (name, score, active) values (?, ?, ?)", ["beta", 2.25, false]))
rows = sqlite.query(db, "select name, score, active from items where score >= ? order by id", [1.5])
print(len(rows))
print(rows[0]["name"])
print(rows[1]["score"])
print(rows[0]["active"])
print(sqlite.scalar(db, "select name from items where active = ? order by id limit 1", [true]))
"#,
        db = db_path.display()
    );

    let result = run_cool(&source).unwrap();
    let _ = std::fs::remove_file(&db_path);
    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["true", "1", "1", "2", "alpha", "2.25", "1", "alpha"]);
}

#[test]
fn test_vm_import_sqlite_module() {
    let db_path = unique_temp_path("cool_vm_sqlite_module_test", "db");
    let source = format!(
        r#"import sqlite
db = "{db}"
print(sqlite.execute(db, "create table items (id integer primary key, name text, score real, active integer)") == 0)
print(sqlite.execute(db, "insert into items (name, score, active) values (?, ?, ?)", ["alpha", 1.5, true]))
print(sqlite.execute(db, "insert into items (name, score, active) values (?, ?, ?)", ["beta", 2.25, false]))
rows = sqlite.query(db, "select name, score, active from items where score >= ? order by id", [1.5])
print(len(rows))
print(rows[0]["name"])
print(rows[1]["score"])
print(rows[0]["active"])
print(sqlite.scalar(db, "select name from items where active = ? order by id limit 1", [true]))
"#,
        db = db_path.display()
    );

    let result = run_cool_vm(&source).unwrap();
    let _ = std::fs::remove_file(&db_path);
    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["true", "1", "1", "2", "alpha", "2.25", "1", "alpha"]);
}

#[test]
fn test_import_http_module() {
    let (base_url, handle) = spawn_http_test_server(4);
    let source = format!(
        r#"import http
import string

base = "{base_url}"
print(http.get(base + "/plain", ["X-Test: yes"]).strip())
print(string.find(http.head(base + "/plain"), "X-Reply: plain") >= 0)
data = http.getjson(base + "/json")
print(data["ok"])
print(data["n"])
print(http.post(base + "/echo", "payload", ["X-Test: yes"]).strip())
"#
    );

    let result = run_cool(&source).unwrap();
    handle.join().unwrap();
    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["hello header=yes", "true", "true", "2", "payload|header=yes"]);
}

#[test]
fn test_vm_import_http_module() {
    let (base_url, handle) = spawn_http_test_server(4);
    let source = format!(
        r#"import http
import string

base = "{base_url}"
print(http.get(base + "/plain", ["X-Test: yes"]).strip())
print(string.find(http.head(base + "/plain"), "X-Reply: plain") >= 0)
data = http.getjson(base + "/json")
print(data["ok"])
print(data["n"])
print(http.post(base + "/echo", "payload", ["X-Test: yes"]).strip())
"#
    );

    let result = run_cool_vm(&source).unwrap();
    handle.join().unwrap();
    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["hello header=yes", "true", "true", "2", "payload|header=yes"]);
}

fn spawn_echo_server() -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let handle = thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
            let mut buf = [0u8; 1024];
            let n = stream.read(&mut buf).unwrap_or(0);
            if n > 0 {
                stream.write_all(&buf[..n]).ok();
            }
        }
    });
    (addr, handle)
}

#[test]
fn test_import_socket_module() {
    let (addr, handle) = spawn_echo_server();
    let parts: Vec<&str> = addr.splitn(2, ':').collect();
    let host = parts[0];
    let port: i64 = parts[1].parse().unwrap();
    let source = format!(
        r#"import socket
conn = socket.connect("{host}", {port})
conn.send("hello socket\n")
data = conn.recv(64)
conn.close()
print(data.strip())
"#
    );
    let result = run_cool(&source).unwrap();
    handle.join().unwrap();
    assert_eq!(result.trim(), "hello socket");
}

#[test]
fn test_vm_import_socket_module() {
    let (addr, handle) = spawn_echo_server();
    let parts: Vec<&str> = addr.splitn(2, ':').collect();
    let host = parts[0];
    let port: i64 = parts[1].parse().unwrap();
    let source = format!(
        r#"import socket
conn = socket.connect("{host}", {port})
conn.send("hello vm socket\n")
data = conn.recv(64)
conn.close()
print(data.strip())
"#
    );
    let result = run_cool_vm(&source).unwrap();
    handle.join().unwrap();
    assert_eq!(result.trim(), "hello vm socket");
}

#[test]
fn test_socket_server_accept() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
            let mut buf = vec![0u8; 256];
            let n = stream.read(&mut buf).unwrap_or(0);
            let _ = stream.write_all(&buf[..n]);
        }
    });
    let source = format!(
        r#"import socket
conn = socket.connect("127.0.0.1", {port})
conn.send("ping\n")
reply = conn.readline()
conn.close()
print(reply.strip())
"#
    );
    let result = run_cool(&source).unwrap();
    handle.join().unwrap();
    assert_eq!(result.trim(), "ping");
}

#[test]
fn test_struct_basic() {
    let result = run_cool(
        r#"struct Point:
    x: i32
    y: i32

p = Point(3, 4)
print(p.x)
print(p.y)
p.x = 10
print(p.x)
"#,
    )
    .unwrap();
    assert_eq!(result.trim(), "3\n4\n10");
}

#[test]
fn test_struct_type_coercion() {
    let result = run_cool(
        r#"struct Counts:
    hits: u8
    score: i32

c = Counts(300, -500000)
print(c.hits)
print(c.score)
"#,
    )
    .unwrap();
    let lines: Vec<_> = result.lines().collect();
    assert_eq!(lines[0], "44"); // 300 wraps to u8: 300 % 256 = 44
    assert_eq!(lines[1], "-500000");
}

#[test]
fn test_struct_kwargs() {
    let result = run_cool(
        r#"struct Vec2:
    x: i64
    y: i64

v = Vec2(x=5, y=7)
print(v.x + v.y)
"#,
    )
    .unwrap();
    assert_eq!(result.trim(), "12");
}

#[test]
fn test_vm_struct_basic() {
    let result = run_cool_vm(
        r#"struct Point:
    x: i32
    y: i32

p = Point(3, 4)
print(p.x)
print(p.y)
p.x = 10
print(p.x)
"#,
    )
    .unwrap();
    assert_eq!(result.trim(), "3\n4\n10");
}

#[test]
fn test_struct_in_function() {
    let result = run_cool(
        r#"struct Rect:
    w: i32
    h: i32

def area(r):
    return r.w * r.h

r = Rect(6, 7)
print(area(r))
"#,
    )
    .unwrap();
    assert_eq!(result.trim(), "42");
}

#[test]
fn test_import_test_module() {
    let result = run_cool(
        r#"import test

def boom(msg):
    raise msg

def failer():
    test.fail("bad")

test.equal(2 + 2, 4)
test.not_equal(2 + 2, 5)
test.truthy(1 < 2)
test.falsey(2 < 1)
test.is_nil(nil)
test.not_nil("x")
print(test.raises(boom, ["boom"], "boom"))
print(test.raises(failer, nil, "AssertionError"))
print("ok")
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["boom", "AssertionError: bad", "ok"]);
}

#[test]
fn test_vm_import_test_module() {
    let result = run_cool_vm(
        r#"import test

def boom(msg):
    raise msg

def failer():
    test.fail("bad")

test.equal(2 + 2, 4)
test.not_equal(2 + 2, 5)
test.truthy(1 < 2)
test.falsey(2 < 1)
test.is_nil(nil)
test.not_nil("x")
print(test.raises(boom, ["boom"], "boom"))
print(test.raises(failer, nil, "AssertionError"))
print("ok")
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["boom", "AssertionError: bad", "ok"]);
}

#[test]
fn test_import_random_choice_tuple() {
    let result =
        run_cool("import random\nrandom.seed(1)\nprint(random.choice((\"x\", \"y\")) in (\"x\", \"y\"))").unwrap();
    assert!(result.contains("true"));
}

#[test]
fn test_vm_import_random_choice_tuple() {
    let result =
        run_cool_vm("import random\nrandom.seed(1)\nprint(random.choice((\"x\", \"y\")) in (\"x\", \"y\"))").unwrap();
    assert!(result.contains("true"));
}

#[test]
fn test_import_sys_argv_uses_script_path() {
    let result = run_cool("import sys\nprint(sys.argv[0])").unwrap();
    assert!(result.contains("temp_cool_test_"));
    assert!(result.contains(".cool"));
}

#[test]
fn test_vm_import_sys_argv_uses_script_path() {
    let result = run_cool_vm("import sys\nprint(sys.argv[0])").unwrap();
    assert!(result.contains("temp_cool_test_"));
    assert!(result.contains(".cool"));
}

#[test]
fn test_vm_sum_tuple() {
    let result = run_cool_vm("print(sum((1, 2, 3)))").unwrap();
    assert!(result.contains("6"));
}

#[test]
fn test_vm_random_seed_reproducible() {
    let result = run_cool_vm(
        "import random\nrandom.seed(42)\na = random.random()\nb = random.random()\nrandom.seed(42)\nprint(a == random.random())\nprint(b == random.random())",
    )
    .unwrap();
    assert!(result.matches("true").count() >= 2);
}

#[test]
fn test_vm_random_randint_invalid_bounds() {
    let err = run_cool_vm("import random\nprint(random.randint(5, 3))").unwrap_err();
    assert!(err.contains("random.randint a must be <= b"));
}

#[test]
fn test_time_sleep_negative_is_clamped() {
    let result = run_cool("import time\ntime.sleep(-0.01)\nprint(\"ok\")").unwrap();
    assert!(result.contains("ok"));
}

#[test]
fn test_vm_time_sleep_negative_is_clamped() {
    let result = run_cool_vm("import time\ntime.sleep(-0.01)\nprint(\"ok\")").unwrap();
    assert!(result.contains("ok"));
}

#[test]
fn test_vm_import_string_module() {
    let result = run_cool_vm(
        "import string\nprint(string.upper(\"hello\"))\nprint(string.join(\" | \", [\"a\", \"b\", \"c\"]))",
    )
    .unwrap();
    assert!(result.contains("HELLO"));
    assert!(result.contains("a | b | c"));
}

#[test]
fn test_vm_import_os_module() {
    let result = run_cool_with_args_and_env(
        "import os\nprint(os.getenv(\"COOL_VM_OS_ENV\"))\nprint(os.getenv(\"COOL_VM_MISSING_ENV\"))\nprint(os.path(\"a\", \"b\", \"c\"))\nprint(os.popen(\"printf vm-os\"))",
        &["--vm"],
        &[("COOL_VM_OS_ENV", "present")],
    )
    .unwrap();
    assert!(result.contains("present"));
    assert!(result.contains("nil"));
    assert!(result.contains("a/b/c"));
    assert!(result.contains("vm-os"));
}

#[test]
fn test_import_logging_module() {
    let log_path = unique_temp_path("cool_logging_module_test", "log");
    let source = format!(
        "import logging\nlogging.basic_config({{\"level\": \"INFO\", \"format\": \"{{timestamp}}|{{level}}|{{name}}|{{message}}\", \"stdout\": false, \"file\": \"{file}\", \"append\": false}})\nlogging.debug(\"hidden\", \"demo\")\nlogging.info(\"shown\", \"demo\")\nlogging.warn(\"warned\", \"demo\")\nlogging.error(\"boom\", \"demo\")\n",
        file = log_path.display()
    );

    let result = run_cool(&source).unwrap();
    let contents = std::fs::read_to_string(&log_path).unwrap();
    let _ = std::fs::remove_file(&log_path);

    assert!(result.trim().is_empty());
    assert_logging_file_output(&contents);
}

#[test]
fn test_vm_import_logging_module() {
    let log_path = unique_temp_path("cool_vm_logging_module_test", "log");
    let source = format!(
        "import logging\nlogging.basic_config({{\"level\": \"INFO\", \"format\": \"{{timestamp}}|{{level}}|{{name}}|{{message}}\", \"stdout\": false, \"file\": \"{file}\", \"append\": false}})\nlogging.debug(\"hidden\", \"demo\")\nlogging.info(\"shown\", \"demo\")\nlogging.warning(\"warned\", \"demo\")\nlogging.error(\"boom\", \"demo\")\n",
        file = log_path.display()
    );

    let result = run_cool_vm(&source).unwrap();
    let contents = std::fs::read_to_string(&log_path).unwrap();
    let _ = std::fs::remove_file(&log_path);

    assert!(result.trim().is_empty());
    assert_logging_file_output(&contents);
}

#[test]
fn test_cool_build_reads_project_table_manifest() {
    let project_dir = unique_temp_dir("cool_project_manifest");
    let _ = std::fs::remove_dir_all(&project_dir);
    std::fs::create_dir_all(project_dir.join("src")).unwrap();
    std::fs::write(
        project_dir.join("cool.toml"),
        r#"[project]
name = "demo"
version = "0.2.0"
main = "src/main.cool"
output = "demo-bin"
"#,
    )
    .unwrap();
    std::fs::write(project_dir.join("src").join("main.cool"), "print(\"project table\")\n").unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&project_dir, &["build"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());

    let binary_path = project_dir.join("demo-bin");
    assert!(
        binary_path.exists(),
        "expected built binary at {}",
        binary_path.display()
    );

    let output = Command::new(&binary_path).output().unwrap();
    let binary_stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let binary_stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "binary failed\nstdout:\n{binary_stdout}\nstderr:\n{binary_stderr}"
    );
    assert!(binary_stdout.contains("project table"));

    let _ = std::fs::remove_dir_all(&project_dir);
}

#[test]
fn test_cool_new_writes_project_table_manifest() {
    let workspace_dir = unique_temp_dir("cool_new_project_table");
    let _ = std::fs::remove_dir_all(&workspace_dir);
    std::fs::create_dir_all(&workspace_dir).unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&workspace_dir, &["new", "demo"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());

    let manifest = std::fs::read_to_string(workspace_dir.join("demo").join("cool.toml")).unwrap();
    assert!(manifest.contains("[project]"));
    assert!(manifest.contains("name = \"demo\""));
    assert!(manifest.contains("main = \"src/main.cool\""));
    let gitignore = std::fs::read_to_string(workspace_dir.join("demo").join(".gitignore")).unwrap();
    assert!(gitignore.contains(".cool/"));

    let _ = std::fs::remove_dir_all(&workspace_dir);
}

#[test]
fn test_repl_banner_shows_current_version() {
    let mut cmd = Command::new(cool_bin());
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn().unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(b"exit\n").unwrap();
    }
    let output = child.wait_with_output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Cool 1.0.0"));
    assert!(stdout.contains("type 'exit' to quit"));
}

#[test]
fn test_cool_test_discovers_named_tests() {
    let temp_dir = unique_temp_dir("cool_test_command_discovery");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(temp_dir.join("tests").join("nested")).unwrap();
    std::fs::write(temp_dir.join("tests").join("test_math.cool"), "assert 2 + 2 == 4\n").unwrap();
    std::fs::write(
        temp_dir.join("tests").join("nested").join("strings_test.cool"),
        "assert \"co\" + \"ol\" == \"cool\"\n",
    )
    .unwrap();
    std::fs::write(temp_dir.join("tests").join("helper.cool"), "assert false\n").unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&temp_dir, &["test"]).unwrap();
    let _ = std::fs::remove_dir_all(&temp_dir);

    assert_eq!(code, 0, "stderr was: {stderr}");
    assert!(stdout.contains("running 2 Cool test file(s) with interpreter"));
    assert!(stdout.contains("ok tests/test_math.cool"));
    assert!(stdout.contains("ok tests/nested/strings_test.cool"));
    assert!(stdout.contains("test result: ok. 2 passed; 0 failed"));
    assert!(!stdout.contains("helper.cool"));
    assert!(stderr.trim().is_empty());
}

#[test]
fn test_cool_test_reports_failures() {
    let temp_dir = unique_temp_dir("cool_test_command_failure");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(temp_dir.join("tests")).unwrap();
    std::fs::write(
        temp_dir.join("tests").join("test_fail.cool"),
        "assert false, \"boom\"\n",
    )
    .unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&temp_dir, &["test"]).unwrap();
    let _ = std::fs::remove_dir_all(&temp_dir);

    assert_ne!(code, 0);
    assert!(stdout.contains("FAILED tests/test_fail.cool"));
    assert!(stdout.contains("test result: FAILED. 0 passed; 1 failed"));
    assert!(stderr.contains("AssertionError: boom"));
    assert!(stderr.contains("cool test: 1 test file(s) failed"));
}

#[test]
fn test_cool_test_vm_mode() {
    let temp_dir = unique_temp_dir("cool_test_command_vm");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(temp_dir.join("tests")).unwrap();
    std::fs::write(
        temp_dir.join("tests").join("test_vm.cool"),
        "items = [1, 2, 3]\nassert len(items) == 3\n",
    )
    .unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&temp_dir, &["test", "--vm"]).unwrap();
    let _ = std::fs::remove_dir_all(&temp_dir);

    assert_eq!(code, 0, "stderr was: {stderr}");
    assert!(stdout.contains("with bytecode VM"));
    assert!(stdout.contains("ok tests/test_vm.cool"));
    assert!(stdout.contains("test result: ok. 1 passed; 0 failed"));
}

#[test]
fn test_cool_test_compile_mode() {
    let temp_dir = unique_temp_dir("cool_test_command_compile");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(temp_dir.join("tests")).unwrap();
    std::fs::write(
        temp_dir.join("tests").join("test_native.cool"),
        "assert sum([1, 2, 3]) == 6\n",
    )
    .unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&temp_dir, &["test", "--compile"]).unwrap();
    let _ = std::fs::remove_dir_all(&temp_dir);

    assert_eq!(code, 0, "stderr was: {stderr}");
    assert!(stdout.contains("with native"));
    assert!(stdout.contains("ok tests/test_native.cool"));
    assert!(stdout.contains("test result: ok. 1 passed; 0 failed"));
}

#[test]
fn test_cool_ast_subcommand_outputs_json_ast() {
    let temp = unique_temp_path("cool_ast_command", "cool");
    std::fs::write(&temp, "x = 1\nif x:\n    print(x)\n").unwrap();

    let cwd = temp.parent().unwrap();
    let file_name = temp.file_name().unwrap().to_str().unwrap();
    let expected_path = temp.canonicalize().unwrap().display().to_string();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(cwd, &["ast", file_name]).unwrap();
    let _ = std::fs::remove_file(&temp);

    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());
    assert!(!stdout.contains("set_line"));

    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed["path"].as_str().unwrap(), expected_path);

    let ast = parsed["ast"].as_array().unwrap();
    assert_eq!(ast.len(), 2);
    assert_eq!(ast[0]["assign"]["name"].as_str().unwrap(), "x");
    assert_eq!(
        ast[1]["if"]["then_body"][0]["expr"]["call"]["callee"]["ident"]
            .as_str()
            .unwrap(),
        "print"
    );
}

#[test]
fn test_cool_inspect_subcommand_summarizes_top_level_symbols() {
    let temp_dir = unique_temp_dir("cool_inspect_command");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(temp_dir.join("app")).unwrap();
    std::fs::create_dir_all(temp_dir.join("lib").join("util")).unwrap();
    std::fs::create_dir_all(temp_dir.join("deps").join("toolkit").join("src")).unwrap();

    std::fs::write(
        temp_dir.join("cool.toml"),
        r#"[project]
name = "inspectdemo"
version = "0.1.0"
main = "app/main.cool"
sources = ["app", "lib"]

[dependencies]
toolkit = { path = "deps/toolkit" }
"#,
    )
    .unwrap();
    std::fs::write(
        temp_dir.join("deps").join("toolkit").join("cool.toml"),
        r#"[project]
name = "toolkit"
version = "0.1.0"
main = "src/main.cool"
"#,
    )
    .unwrap();
    std::fs::write(temp_dir.join("app").join("helper.cool"), "value = 1\n").unwrap();
    std::fs::write(temp_dir.join("app").join("shared.cool"), "value = 2\n").unwrap();
    std::fs::write(temp_dir.join("lib").join("util").join("math.cool"), "value = 3\n").unwrap();
    std::fs::write(
        temp_dir.join("deps").join("toolkit").join("src").join("util.cool"),
        "value = 4\n",
    )
    .unwrap();

    std::fs::write(
        temp_dir.join("app").join("main.cool"),
        r#"import json
import helper
import util.math
import toolkit.util
import "shared.cool"

answer = 42
counter += 1
left, right = pair

def greet(name, title="Hi", *rest, **options):
    return name

class Person(Human):
    species = "human"

    def __init__(self, name):
        self.name = name

    def greet(self, other="world"):
        print(other)

packed struct Header:
    version: u8
    flags: u16
"#,
    )
    .unwrap();

    let entry_path = temp_dir.join("app").join("main.cool").canonicalize().unwrap();
    let helper_path = temp_dir.join("app").join("helper.cool").canonicalize().unwrap();
    let shared_path = temp_dir.join("app").join("shared.cool").canonicalize().unwrap();
    let lib_path = temp_dir
        .join("lib")
        .join("util")
        .join("math.cool")
        .canonicalize()
        .unwrap();
    let dep_path = temp_dir
        .join("deps")
        .join("toolkit")
        .join("src")
        .join("util.cool")
        .canonicalize()
        .unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&temp_dir, &["inspect", "app/main.cool"]).unwrap();
    let _ = std::fs::remove_dir_all(&temp_dir);

    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());

    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed["path"].as_str().unwrap(), entry_path.display().to_string());

    let imports = parsed["imports"].as_array().unwrap();
    assert!(imports
        .iter()
        .any(|import| { import["kind"].as_str() == Some("builtin") && import["specifier"].as_str() == Some("json") }));
    assert!(imports.iter().any(|import| {
        import["specifier"].as_str() == Some("helper")
            && import["resolved"].as_str() == Some(helper_path.to_str().unwrap())
    }));
    assert!(imports.iter().any(|import| {
        import["specifier"].as_str() == Some("util.math")
            && import["resolved"].as_str() == Some(lib_path.to_str().unwrap())
    }));
    assert!(imports.iter().any(|import| {
        import["specifier"].as_str() == Some("toolkit.util")
            && import["resolved"].as_str() == Some(dep_path.to_str().unwrap())
    }));
    assert!(imports.iter().any(|import| {
        import["kind"].as_str() == Some("file")
            && import["specifier"].as_str() == Some("shared.cool")
            && import["resolved"].as_str() == Some(shared_path.to_str().unwrap())
    }));

    let functions = parsed["functions"].as_array().unwrap();
    assert_eq!(functions.len(), 1);
    assert_eq!(functions[0]["name"].as_str().unwrap(), "greet");
    let params = functions[0]["params"].as_array().unwrap();
    assert_eq!(params.len(), 4);
    assert_eq!(params[0]["name"].as_str().unwrap(), "name");
    assert_eq!(params[1]["name"].as_str().unwrap(), "title");
    assert_eq!(params[1]["has_default"].as_bool().unwrap(), true);
    assert_eq!(params[2]["is_vararg"].as_bool().unwrap(), true);
    assert_eq!(params[3]["is_kwarg"].as_bool().unwrap(), true);

    let classes = parsed["classes"].as_array().unwrap();
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0]["name"].as_str().unwrap(), "Person");
    assert_eq!(classes[0]["parent"].as_str().unwrap(), "Human");
    let methods = classes[0]["methods"].as_array().unwrap();
    assert_eq!(methods.len(), 2);
    assert!(methods.iter().any(|method| method["name"].as_str() == Some("__init__")));
    assert!(methods.iter().any(|method| method["name"].as_str() == Some("greet")));
    let class_assignments = classes[0]["class_assignments"].as_array().unwrap();
    assert_eq!(class_assignments.len(), 1);
    assert_eq!(class_assignments[0]["kind"].as_str().unwrap(), "assign");
    assert_eq!(class_assignments[0]["names"][0].as_str().unwrap(), "species");

    let structs = parsed["structs"].as_array().unwrap();
    assert_eq!(structs.len(), 1);
    assert_eq!(structs[0]["name"].as_str().unwrap(), "Header");
    assert_eq!(structs[0]["is_packed"].as_bool().unwrap(), true);
    assert_eq!(structs[0]["fields"][0]["name"].as_str().unwrap(), "version");
    assert_eq!(structs[0]["fields"][0]["type_name"].as_str().unwrap(), "u8");

    let assignments = parsed["assignments"].as_array().unwrap();
    assert!(assignments.iter().any(|assignment| {
        assignment["kind"].as_str() == Some("assign") && assignment["names"][0].as_str() == Some("answer")
    }));
    assert!(assignments.iter().any(|assignment| {
        assignment["kind"].as_str() == Some("aug_assign") && assignment["names"][0].as_str() == Some("counter")
    }));
    assert!(assignments.iter().any(|assignment| {
        assignment["kind"].as_str() == Some("unpack")
            && assignment["names"][0].as_str() == Some("left")
            && assignment["names"][1].as_str() == Some("right")
    }));
}

#[test]
fn test_cool_symbols_subcommand_indexes_project_symbols() {
    let temp_dir = unique_temp_dir("cool_symbols_command");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(temp_dir.join("app")).unwrap();
    std::fs::create_dir_all(temp_dir.join("lib").join("util")).unwrap();

    std::fs::write(
        temp_dir.join("cool.toml"),
        r#"[project]
name = "symboldemo"
version = "0.1.0"
main = "app/main.cool"
sources = ["app", "lib"]
"#,
    )
    .unwrap();

    std::fs::write(
        temp_dir.join("app").join("main.cool"),
        r#"import json
import util.math
import "shared.cool"

APP_NAME = "symboldemo"

def greet(name):
    return math.add(1, 2)

class Person:
    title = "dev"

    def rename(self, name):
        self.name = name
"#,
    )
    .unwrap();
    std::fs::write(
        temp_dir.join("app").join("shared.cool"),
        r#"def shared_helper():
    return "ok"
"#,
    )
    .unwrap();
    std::fs::write(
        temp_dir.join("lib").join("util").join("math.cool"),
        r#"def add(a, b):
    return a + b
"#,
    )
    .unwrap();

    let entry_path = temp_dir.join("app").join("main.cool").canonicalize().unwrap();
    let shared_path = temp_dir.join("app").join("shared.cool").canonicalize().unwrap();
    let math_path = temp_dir
        .join("lib")
        .join("util")
        .join("math.cool")
        .canonicalize()
        .unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&temp_dir, &["symbols"]).unwrap();
    let _ = std::fs::remove_dir_all(&temp_dir);

    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());

    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed["entry"].as_str().unwrap(), entry_path.display().to_string());
    assert_eq!(parsed["modules_indexed"].as_u64().unwrap(), 3);
    assert!(parsed["diagnostics"].as_array().unwrap().is_empty());

    let symbols = parsed["symbols"].as_array().unwrap();
    assert!(symbols.iter().any(|symbol| {
        symbol["path"].as_str() == Some(entry_path.to_str().unwrap())
            && symbol["name"].as_str() == Some("json")
            && symbol["kind"].as_str() == Some("import")
            && symbol["import_kind"].as_str() == Some("builtin")
            && symbol["import_specifier"].as_str() == Some("json")
    }));
    assert!(symbols.iter().any(|symbol| {
        symbol["path"].as_str() == Some(entry_path.to_str().unwrap())
            && symbol["name"].as_str() == Some("math")
            && symbol["kind"].as_str() == Some("import")
            && symbol["import_kind"].as_str() == Some("module")
            && symbol["import_specifier"].as_str() == Some("util.math")
            && symbol["import_resolved"].as_str() == Some(math_path.to_str().unwrap())
    }));
    assert!(symbols.iter().any(|symbol| {
        symbol["path"].as_str() == Some(entry_path.to_str().unwrap())
            && symbol["name"].as_str() == Some("APP_NAME")
            && symbol["kind"].as_str() == Some("assignment")
    }));
    assert!(symbols.iter().any(|symbol| {
        symbol["path"].as_str() == Some(entry_path.to_str().unwrap())
            && symbol["name"].as_str() == Some("greet")
            && symbol["kind"].as_str() == Some("function")
    }));
    assert!(symbols.iter().any(|symbol| {
        symbol["path"].as_str() == Some(entry_path.to_str().unwrap())
            && symbol["name"].as_str() == Some("Person")
            && symbol["kind"].as_str() == Some("class")
    }));
    assert!(symbols.iter().any(|symbol| {
        symbol["path"].as_str() == Some(entry_path.to_str().unwrap())
            && symbol["name"].as_str() == Some("title")
            && symbol["kind"].as_str() == Some("class_assignment")
            && symbol["container"].as_str() == Some("Person")
    }));
    assert!(symbols.iter().any(|symbol| {
        symbol["path"].as_str() == Some(entry_path.to_str().unwrap())
            && symbol["name"].as_str() == Some("rename")
            && symbol["kind"].as_str() == Some("method")
            && symbol["container"].as_str() == Some("Person")
    }));
    assert!(symbols.iter().any(|symbol| {
        symbol["path"].as_str() == Some(shared_path.to_str().unwrap())
            && symbol["name"].as_str() == Some("shared_helper")
            && symbol["kind"].as_str() == Some("function")
    }));
}

#[test]
fn test_cool_diff_subcommand_reports_top_level_changes() {
    let temp_dir = unique_temp_dir("cool_diff_command");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(temp_dir.join("app")).unwrap();
    std::fs::create_dir_all(temp_dir.join("lib").join("util")).unwrap();

    std::fs::write(
        temp_dir.join("cool.toml"),
        r#"[project]
name = "diffdemo"
version = "0.1.0"
main = "app/before.cool"
sources = ["app", "lib"]
"#,
    )
    .unwrap();
    std::fs::write(temp_dir.join("app").join("helper.cool"), "value = 1\n").unwrap();
    std::fs::write(temp_dir.join("lib").join("util").join("math.cool"), "value = 2\n").unwrap();

    std::fs::write(
        temp_dir.join("app").join("before.cool"),
        r#"import json
import helper

answer = 42

def greet(name):
    return name

class Person:
    def greet(self):
        pass

struct Header:
    version: u8
"#,
    )
    .unwrap();
    std::fs::write(
        temp_dir.join("app").join("after.cool"),
        r#"import json
import util.math

answer = 42
total = 99

def greet(name, title="Hi"):
    return name

class Person(Human):
    def greet(self, other="world"):
        print(other)

    def rename(self, name):
        self.name = name

packed struct Header:
    version: u8
    flags: u16
"#,
    )
    .unwrap();

    let before_path = temp_dir.join("app").join("before.cool").canonicalize().unwrap();
    let after_path = temp_dir.join("app").join("after.cool").canonicalize().unwrap();
    let helper_path = temp_dir.join("app").join("helper.cool").canonicalize().unwrap();
    let math_path = temp_dir
        .join("lib")
        .join("util")
        .join("math.cool")
        .canonicalize()
        .unwrap();

    let (stdout, stderr, code) =
        run_cool_subcommand_in_dir(&temp_dir, &["diff", "app/before.cool", "app/after.cool"]).unwrap();
    let _ = std::fs::remove_dir_all(&temp_dir);

    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());

    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed["before"].as_str().unwrap(), before_path.display().to_string());
    assert_eq!(parsed["after"].as_str().unwrap(), after_path.display().to_string());

    let import_added = parsed["imports"]["added"].as_array().unwrap();
    let import_removed = parsed["imports"]["removed"].as_array().unwrap();
    assert!(import_added.iter().any(|item| {
        item["specifier"].as_str() == Some("util.math")
            && item["resolved"].as_str() == Some(math_path.to_str().unwrap())
    }));
    assert!(import_removed.iter().any(|item| {
        item["specifier"].as_str() == Some("helper") && item["resolved"].as_str() == Some(helper_path.to_str().unwrap())
    }));

    let function_changed = parsed["functions"]["changed"].as_array().unwrap();
    assert_eq!(function_changed.len(), 1);
    assert_eq!(function_changed[0]["before"]["name"].as_str().unwrap(), "greet");
    assert_eq!(function_changed[0]["before"]["params"].as_array().unwrap().len(), 1);
    assert_eq!(function_changed[0]["after"]["params"].as_array().unwrap().len(), 2);

    let class_changed = parsed["classes"]["changed"].as_array().unwrap();
    assert_eq!(class_changed.len(), 1);
    assert_eq!(class_changed[0]["before"]["name"].as_str().unwrap(), "Person");
    assert_eq!(class_changed[0]["after"]["parent"].as_str().unwrap(), "Human");
    assert_eq!(class_changed[0]["after"]["methods"].as_array().unwrap().len(), 2);

    let struct_changed = parsed["structs"]["changed"].as_array().unwrap();
    assert_eq!(struct_changed.len(), 1);
    assert_eq!(struct_changed[0]["before"]["is_packed"].as_bool().unwrap(), false);
    assert_eq!(struct_changed[0]["after"]["is_packed"].as_bool().unwrap(), true);
    assert_eq!(struct_changed[0]["after"]["fields"].as_array().unwrap().len(), 2);

    let assignment_added = parsed["assignments"]["added"].as_array().unwrap();
    assert!(assignment_added
        .iter()
        .any(|item| { item["kind"].as_str() == Some("assign") && item["names"][0].as_str() == Some("total") }));
}

#[test]
fn test_cool_check_subcommand_uses_project_main_by_default() {
    let temp_dir = unique_temp_dir("cool_check_command_ok");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(temp_dir.join("app")).unwrap();

    std::fs::write(
        temp_dir.join("cool.toml"),
        r#"[project]
name = "checkdemo"
version = "0.1.0"
main = "app/main.cool"
"#,
    )
    .unwrap();
    std::fs::write(
        temp_dir.join("app").join("main.cool"),
        "import \"helper.cool\"\nprint(\"ok\")\n",
    )
    .unwrap();
    std::fs::write(temp_dir.join("app").join("helper.cool"), "value = 1\n").unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&temp_dir, &["check"]).unwrap();
    let _ = std::fs::remove_dir_all(&temp_dir);

    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stdout.contains("check ok: 2 module(s) checked, 0 issue(s)"));
    assert!(stderr.trim().is_empty());
}

#[test]
fn test_cool_check_subcommand_reports_unresolved_imports() {
    let temp_dir = unique_temp_dir("cool_check_command_missing");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(temp_dir.join("app")).unwrap();
    std::fs::write(
        temp_dir.join("app").join("main.cool"),
        "import missing.module\nimport \"missing.cool\"\n",
    )
    .unwrap();

    let main_path = temp_dir.join("app").join("main.cool").canonicalize().unwrap();
    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&temp_dir, &["check", "app/main.cool"]).unwrap();
    let _ = std::fs::remove_dir_all(&temp_dir);

    assert_ne!(code, 0);
    assert!(stdout.trim().is_empty());
    assert!(stderr.contains("error[unresolved_import]"));
    assert!(stderr.contains(main_path.to_str().unwrap()));
    assert!(stderr.contains("unresolved module import 'missing.module'"));
    assert!(stderr.contains("unresolved file import 'missing.cool'"));
    assert!(stderr.contains("cool check: 2 error(s), 0 warning(s)"));
}

#[test]
fn test_cool_check_subcommand_reports_import_cycles() {
    let temp_dir = unique_temp_dir("cool_check_command_cycle");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(temp_dir.join("app")).unwrap();
    std::fs::write(temp_dir.join("app").join("a.cool"), "import \"b.cool\"\n").unwrap();
    std::fs::write(temp_dir.join("app").join("b.cool"), "import \"a.cool\"\n").unwrap();

    let a_path = temp_dir.join("app").join("a.cool").canonicalize().unwrap();
    let b_path = temp_dir.join("app").join("b.cool").canonicalize().unwrap();
    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&temp_dir, &["check", "app/a.cool"]).unwrap();
    let _ = std::fs::remove_dir_all(&temp_dir);

    assert_ne!(code, 0);
    assert!(stdout.trim().is_empty());
    assert!(stderr.contains("error[import_cycle]"));
    assert!(stderr.contains(a_path.to_str().unwrap()));
    assert!(stderr.contains(b_path.to_str().unwrap()));
    assert!(stderr.contains("import cycle detected"));
    assert!(stderr.contains("cool check: 1 error(s), 0 warning(s)"));
}

#[test]
fn test_cool_check_subcommand_warns_on_duplicate_symbols_without_failing() {
    let temp_dir = unique_temp_dir("cool_check_command_warnings");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(temp_dir.join("app")).unwrap();
    std::fs::create_dir_all(temp_dir.join("app").join("util")).unwrap();
    std::fs::write(temp_dir.join("app").join("util").join("math.cool"), "value = 1\n").unwrap();
    std::fs::write(
        temp_dir.join("app").join("main.cool"),
        r#"import math
import util.math

def greet():
    pass

def greet(name):
    return name

class Person:
    title = "x"

    def title(self):
        return "y"
"#,
    )
    .unwrap();

    let main_path = temp_dir.join("app").join("main.cool").canonicalize().unwrap();
    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&temp_dir, &["check", "app/main.cool"]).unwrap();
    let _ = std::fs::remove_dir_all(&temp_dir);

    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stdout.contains("check ok: 2 module(s) checked, 0 error(s), 3 warning(s)"));
    assert!(stderr.contains("warning[duplicate_symbol]"));
    assert!(stderr.contains("warning[duplicate_member]"));
    assert!(stderr.contains(main_path.to_str().unwrap()));
    assert!(stderr.contains("top-level symbol 'math'"));
    assert!(stderr.contains("top-level symbol 'greet'"));
    assert!(stderr.contains("class 'Person' member 'title'"));
}

#[test]
fn test_cool_modulegraph_subcommand_resolves_project_imports() {
    let temp_dir = unique_temp_dir("cool_modulegraph_command");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(temp_dir.join("app")).unwrap();
    std::fs::create_dir_all(temp_dir.join("lib").join("util")).unwrap();
    std::fs::create_dir_all(temp_dir.join("deps").join("toolkit").join("src")).unwrap();

    std::fs::write(
        temp_dir.join("cool.toml"),
        r#"[project]
name = "graphdemo"
version = "0.1.0"
main = "app/main.cool"
sources = ["app", "lib"]

[dependencies]
toolkit = { path = "deps/toolkit" }
"#,
    )
    .unwrap();
    std::fs::write(
        temp_dir.join("deps").join("toolkit").join("cool.toml"),
        r#"[project]
name = "toolkit"
version = "0.1.0"
main = "src/main.cool"
"#,
    )
    .unwrap();

    std::fs::write(
        temp_dir.join("app").join("main.cool"),
        "import json\nimport helper\nimport util.math\nimport toolkit.util\nimport \"shared.cool\"\n",
    )
    .unwrap();
    std::fs::write(temp_dir.join("app").join("helper.cool"), "import string\nvalue = 1\n").unwrap();
    std::fs::write(temp_dir.join("app").join("shared.cool"), "import path\nshared = 1\n").unwrap();
    std::fs::write(
        temp_dir.join("lib").join("util").join("math.cool"),
        "import time\nvalue = 1\n",
    )
    .unwrap();
    std::fs::write(
        temp_dir.join("deps").join("toolkit").join("src").join("util.cool"),
        "import hashlib\nvalue = 1\n",
    )
    .unwrap();

    let entry_path = temp_dir.join("app").join("main.cool").canonicalize().unwrap();
    let helper_path = temp_dir.join("app").join("helper.cool").canonicalize().unwrap();
    let shared_path = temp_dir.join("app").join("shared.cool").canonicalize().unwrap();
    let lib_path = temp_dir
        .join("lib")
        .join("util")
        .join("math.cool")
        .canonicalize()
        .unwrap();
    let dep_path = temp_dir
        .join("deps")
        .join("toolkit")
        .join("src")
        .join("util.cool")
        .canonicalize()
        .unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&temp_dir, &["modulegraph", "app/main.cool"]).unwrap();
    let _ = std::fs::remove_dir_all(&temp_dir);

    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());

    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed["entry"].as_str().unwrap(), entry_path.display().to_string());

    let modules = parsed["modules"].as_array().unwrap();
    let module_paths: Vec<&str> = modules.iter().map(|module| module["path"].as_str().unwrap()).collect();
    assert!(module_paths.contains(&entry_path.to_str().unwrap()));
    assert!(module_paths.contains(&helper_path.to_str().unwrap()));
    assert!(module_paths.contains(&shared_path.to_str().unwrap()));
    assert!(module_paths.contains(&lib_path.to_str().unwrap()));
    assert!(module_paths.contains(&dep_path.to_str().unwrap()));

    let entry_module = modules
        .iter()
        .find(|module| module["path"].as_str() == Some(entry_path.to_str().unwrap()))
        .unwrap();
    let imports = entry_module["imports"].as_array().unwrap();
    assert!(imports
        .iter()
        .any(|import| { import["kind"].as_str() == Some("builtin") && import["specifier"].as_str() == Some("json") }));
    assert!(imports.iter().any(|import| {
        import["kind"].as_str() == Some("module")
            && import["specifier"].as_str() == Some("helper")
            && import["resolved"].as_str() == Some(helper_path.to_str().unwrap())
    }));
    assert!(imports.iter().any(|import| {
        import["kind"].as_str() == Some("module")
            && import["specifier"].as_str() == Some("util.math")
            && import["resolved"].as_str() == Some(lib_path.to_str().unwrap())
    }));
    assert!(imports.iter().any(|import| {
        import["kind"].as_str() == Some("module")
            && import["specifier"].as_str() == Some("toolkit.util")
            && import["resolved"].as_str() == Some(dep_path.to_str().unwrap())
    }));
    assert!(imports.iter().any(|import| {
        import["kind"].as_str() == Some("file")
            && import["specifier"].as_str() == Some("shared.cool")
            && import["resolved"].as_str() == Some(shared_path.to_str().unwrap())
    }));
}

#[test]
fn test_import_path_module() {
    let file_path = unique_temp_path("cool_path_module_test", "txt");
    std::fs::write(&file_path, "ok").unwrap();

    let source = format!(
        "import path\nprint(path.join(\"a\", \"b\", \"c.txt\"))\nprint(path.basename(\"a/b/c.txt\"))\nprint(path.dirname(\"a/b/c.txt\"))\nprint(path.ext(\"a/b/c.txt\"))\nprint(path.stem(\"a/b/c.txt\"))\nprint(path.split(\"a/b/c.txt\"))\nprint(path.normalize(\"a/./b/../c//d.txt\"))\nprint(path.exists(\"{file}\"))\nprint(path.isabs(\"{file}\"))\n",
        file = file_path.display()
    );

    let result = run_cool(&source).unwrap();
    let _ = std::fs::remove_file(&file_path);

    assert!(result.contains("a/b/c.txt"));
    assert!(result.contains("c.txt"));
    assert!(result.contains(".txt"));
    assert!(result.contains("\nc\n") || result.contains("\nc\r\n"));
    assert!(result.contains("[\"a/b\", \"c.txt\"]") || result.contains("[\"a/b\",\"c.txt\"]"));
    assert!(result.contains("a/c/d.txt"));
    assert!(result.matches("true").count() >= 2);
}

#[test]
fn test_vm_import_path_module() {
    let file_path = unique_temp_path("cool_vm_path_module_test", "txt");
    std::fs::write(&file_path, "ok").unwrap();

    let source = format!(
        "import path\nprint(path.join(\"a\", \"b\", \"c.txt\"))\nprint(path.basename(\"a/b/c.txt\"))\nprint(path.dirname(\"a/b/c.txt\"))\nprint(path.ext(\"a/b/c.txt\"))\nprint(path.stem(\"a/b/c.txt\"))\nprint(path.split(\"a/b/c.txt\"))\nprint(path.normalize(\"a/./b/../c//d.txt\"))\nprint(path.exists(\"{file}\"))\nprint(path.isabs(\"{file}\"))\n",
        file = file_path.display()
    );

    let result = run_cool_vm(&source).unwrap();
    let _ = std::fs::remove_file(&file_path);

    assert!(result.contains("a/b/c.txt"));
    assert!(result.contains("c.txt"));
    assert!(result.contains(".txt"));
    assert!(result.contains("\nc\n") || result.contains("\nc\r\n"));
    assert!(result.contains("[\"a/b\", \"c.txt\"]") || result.contains("[\"a/b\",\"c.txt\"]"));
    assert!(result.contains("a/c/d.txt"));
    assert!(result.matches("true").count() >= 2);
}

#[test]
fn test_vm_import_list_module() {
    let result =
        run_cool_vm("import list\nnums = [3, 1, 2]\nprint(list.sort(nums))\nprint(list.unique([1, 1, 2, 2, 3]))")
            .unwrap();
    assert!(result.contains("[1, 2, 3]") || result.contains("[1,2,3]"));
    assert!(result.contains("[1, 2, 3]") || result.contains("[1,2,3]"));
}

#[test]
fn test_vm_self_import_reports_error() {
    let temp_dir = unique_temp_dir("cool_vm_self_import_test");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    let source_path = temp_dir.join("string.cool");
    std::fs::write(&source_path, "import string\nprint(\"unreachable\")\n").unwrap();

    let output = Command::new(cool_bin())
        .args(["--vm", source_path.to_str().unwrap()])
        .output()
        .unwrap();

    let _ = std::fs::remove_file(&source_path);
    let _ = std::fs::remove_dir(&temp_dir);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("circular import detected"));
}

#[test]
fn test_import_collections_module() {
    let result = run_cool(
        "import collections\nq = collections.Queue()\nq.enqueue(\"first\")\nq.enqueue(\"second\")\nprint(q.dequeue())\ns = collections.Stack()\ns.push(\"a\")\ns.push(\"b\")\nprint(s.pop())",
    )
    .unwrap();
    assert!(result.contains("first"));
    assert!(result.contains("b"));
}

#[test]
fn test_vm_import_collections_module() {
    let result = run_cool_vm(
        "import collections\nq = collections.Queue()\nq.enqueue(\"first\")\nq.enqueue(\"second\")\nprint(q.dequeue())\ns = collections.Stack()\ns.push(\"a\")\ns.push(\"b\")\nprint(s.pop())",
    )
    .unwrap();
    assert!(result.contains("first"));
    assert!(result.contains("b"));
}

#[test]
fn test_with_context_manager_uses_enter_result() {
    let result = run_cool(
        "class C:\n\tdef __enter__(self):\n\t\tprint(\"enter\")\n\t\treturn 42\n\tdef __exit__(self, exc_type, exc_val, exc_tb):\n\t\tprint(\"exit\")\nwith C() as value:\n\tprint(value)",
    )
    .unwrap();
    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["enter", "42", "exit"]);
    assert!(!result.contains("<C object>"));
}

#[test]
fn test_vm_with_context_manager_uses_enter_result() {
    let result = run_cool_vm(
        "class C:\n\tdef __enter__(self):\n\t\tprint(\"enter\")\n\t\treturn 42\n\tdef __exit__(self, exc_type, exc_val, exc_tb):\n\t\tprint(\"exit\")\nwith C() as value:\n\tprint(value)",
    )
    .unwrap();
    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["enter", "42", "exit"]);
    assert!(!result.contains("<C object>"));
}

#[test]
fn test_vm_with_context_manager_cleans_on_exception() {
    let result = run_cool_vm(
        "class C:\n\tdef __enter__(self):\n\t\tprint(\"enter\")\n\t\treturn self\n\tdef __exit__(self, exc_type, exc_val, exc_tb):\n\t\tprint(\"exit\")\ntry:\n\twith C() as value:\n\t\tx = 1 / 0\nexcept:\n\tprint(\"caught\")",
    )
    .unwrap();
    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["enter", "exit", "caught"]);
}

#[test]
fn test_vm_with_context_manager_cleans_on_return() {
    let result = run_cool_vm(
        "class C:\n\tdef __enter__(self):\n\t\tprint(\"enter\")\n\t\treturn self\n\tdef __exit__(self, exc_type, exc_val, exc_tb):\n\t\tprint(\"exit\")\ndef f():\n\twith C():\n\t\treturn 7\nprint(f())",
    )
    .unwrap();
    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["enter", "exit", "7"]);
}

#[test]
fn test_vm_with_context_manager_cleans_on_continue() {
    let result = run_cool_vm(
        "class C:\n\tdef __init__(self, name):\n\t\tself.name = name\n\tdef __enter__(self):\n\t\tprint(\"enter \" + self.name)\n\t\treturn self\n\tdef __exit__(self, exc_type, exc_val, exc_tb):\n\t\tprint(\"exit \" + self.name)\nfor i in range(2):\n\twith C(str(i)):\n\t\tif i == 0:\n\t\t\tcontinue\n\t\tprint(\"body\")\nprint(\"done\")",
    )
    .unwrap();
    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["enter 0", "exit 0", "enter 1", "body", "exit 1", "done"]);
}

#[test]
fn test_vm_with_context_manager_break_only_cleans_exited_scope() {
    let result = run_cool_vm(
        "class C:\n\tdef __init__(self, name):\n\t\tself.name = name\n\tdef __enter__(self):\n\t\tprint(\"enter \" + self.name)\n\t\treturn self\n\tdef __exit__(self, exc_type, exc_val, exc_tb):\n\t\tprint(\"exit \" + self.name)\nwith C(\"outer\"):\n\tfor i in range(2):\n\t\twith C(\"inner\"):\n\t\t\tbreak\n\tprint(\"after\")",
    )
    .unwrap();
    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(
        lines,
        ["enter outer", "enter inner", "exit inner", "after", "exit outer"]
    );
}

#[test]
fn test_import_dotted_module_package_path() {
    let temp_dir = unique_temp_dir("cool_import_package_test");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(temp_dir.join("foo")).unwrap();
    let source_path = temp_dir.join("main.cool");
    std::fs::write(temp_dir.join("foo").join("bar.cool"), "value = 42\n").unwrap();
    std::fs::write(&source_path, "import foo.bar\nprint(bar.value)\n").unwrap();

    let result = run_cool_path_with_args(&source_path, &[]).unwrap();

    let _ = std::fs::remove_dir_all(&temp_dir);
    assert!(result.contains("42"));
}

#[test]
fn test_vm_import_dotted_module_package_path() {
    let temp_dir = unique_temp_dir("cool_vm_import_package_test");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(temp_dir.join("foo")).unwrap();
    let source_path = temp_dir.join("main.cool");
    std::fs::write(temp_dir.join("foo").join("bar.cool"), "value = 42\n").unwrap();
    std::fs::write(&source_path, "import foo.bar\nprint(bar.value)\n").unwrap();

    let result = run_cool_path_with_args(&source_path, &["--vm"]).unwrap();

    let _ = std::fs::remove_dir_all(&temp_dir);
    assert!(result.contains("42"));
}

#[test]
fn test_project_sources_and_dependencies_resolve_imports() {
    let temp_dir = unique_temp_dir("cool_project_sources_and_deps");
    write_project_with_sources_and_dependencies(&temp_dir);

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&temp_dir, &["app/main.cool"]).unwrap();
    let _ = std::fs::remove_dir_all(&temp_dir);

    assert_eq!(code, 0, "stderr:\n{stderr}");
    let lines: Vec<_> = stdout.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["7", "9"]);
}

#[test]
fn test_vm_project_sources_and_dependencies_resolve_imports() {
    let temp_dir = unique_temp_dir("cool_vm_project_sources_and_deps");
    write_project_with_sources_and_dependencies(&temp_dir);

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&temp_dir, &["--vm", "app/main.cool"]).unwrap();
    let _ = std::fs::remove_dir_all(&temp_dir);

    assert_eq!(code, 0, "stderr:\n{stderr}");
    let lines: Vec<_> = stdout.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["7", "9"]);
}

#[test]
fn test_cool_build_uses_sources_and_dependencies() {
    let temp_dir = unique_temp_dir("cool_build_sources_and_deps");
    write_project_with_sources_and_dependencies(&temp_dir);

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&temp_dir, &["build"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");

    let binary_path = temp_dir.join("demo");
    let output = Command::new(&binary_path).output().unwrap();
    let _ = std::fs::remove_dir_all(&temp_dir);

    assert!(output.status.success());
    let binary_stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<_> = binary_stdout.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["7", "9"]);
}

#[test]
fn test_cool_install_fetches_git_dependency_and_build_uses_it() {
    let workspace_dir = unique_temp_dir("cool_install_git_dependency");
    let project_dir = workspace_dir.join("app");
    let dep_dir = workspace_dir.join("toolkit_repo");
    let dep_rev = write_git_dependency_repo(&dep_dir, 42);
    write_basic_project(&project_dir, "demo", "import toolkit.util\nprint(util.value)\n");
    std::fs::write(
        project_dir.join("cool.toml"),
        r#"[project]
name = "demo"
version = "0.1.0"
main = "src/main.cool"

[dependencies]
toolkit = { git = "../toolkit_repo" }
"#,
    )
    .unwrap();

    let (install_stdout, install_stderr, install_code) =
        run_cool_subcommand_in_dir(&project_dir, &["install"]).unwrap();
    assert_eq!(install_code, 0, "stdout:\n{install_stdout}\nstderr:\n{install_stderr}");
    assert!(install_stdout.contains("Installed 1 dependency"));
    assert!(install_stderr.trim().is_empty());

    let lockfile = std::fs::read_to_string(project_dir.join("cool.lock")).unwrap();
    assert!(lockfile.contains("kind = \"git\""));
    assert!(lockfile.contains("git = \"../toolkit_repo\""));
    assert!(lockfile.contains(&format!("rev = \"{dep_rev}\"")));
    assert!(project_dir
        .join(".cool")
        .join("deps")
        .join("toolkit")
        .join(".git")
        .exists());

    let (build_stdout, build_stderr, build_code) = run_cool_subcommand_in_dir(&project_dir, &["build"]).unwrap();
    assert_eq!(build_code, 0, "stdout:\n{build_stdout}\nstderr:\n{build_stderr}");
    let binary_output = Command::new(project_dir.join("demo")).output().unwrap();

    let _ = std::fs::remove_dir_all(&workspace_dir);

    assert!(binary_output.status.success());
    assert_eq!(String::from_utf8_lossy(&binary_output.stdout).trim(), "42");
}

#[test]
fn test_cool_build_reports_install_hint_for_missing_git_dependency() {
    let workspace_dir = unique_temp_dir("cool_missing_git_dependency");
    let project_dir = workspace_dir.join("app");
    let dep_dir = workspace_dir.join("toolkit_repo");
    write_git_dependency_repo(&dep_dir, 12);
    write_basic_project(&project_dir, "demo", "import toolkit.util\nprint(util.value)\n");
    std::fs::write(
        project_dir.join("cool.toml"),
        r#"[project]
name = "demo"
version = "0.1.0"
main = "src/main.cool"

[dependencies]
toolkit = { git = "../toolkit_repo" }
"#,
    )
    .unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&project_dir, &["build"]).unwrap();
    let _ = std::fs::remove_dir_all(&workspace_dir);

    assert_ne!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.contains("Run `cool install`"));
}

#[test]
fn test_cool_add_path_dependency_updates_manifest_and_lockfile() {
    let workspace_dir = unique_temp_dir("cool_add_path_dependency");
    let project_dir = workspace_dir.join("app");
    let dep_dir = workspace_dir.join("toolkit");
    write_basic_project(&project_dir, "demo", "import toolkit.util\nprint(util.value)\n");
    let _ = std::fs::remove_dir_all(&dep_dir);
    std::fs::create_dir_all(dep_dir.join("src")).unwrap();
    std::fs::write(
        dep_dir.join("cool.toml"),
        r#"[project]
name = "toolkit"
version = "0.2.1"
main = "src/main.cool"
"#,
    )
    .unwrap();
    std::fs::write(dep_dir.join("src").join("main.cool"), "print(\"toolkit\")\n").unwrap();
    std::fs::write(dep_dir.join("src").join("util.cool"), "value = 77\n").unwrap();

    let (stdout, stderr, code) =
        run_cool_subcommand_in_dir(&project_dir, &["add", "toolkit", "--path", "../toolkit"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());

    let manifest = std::fs::read_to_string(project_dir.join("cool.toml")).unwrap();
    let parsed: toml::Value = manifest.parse().unwrap();
    assert_eq!(parsed["dependencies"]["toolkit"]["path"].as_str(), Some("../toolkit"));

    let lockfile = std::fs::read_to_string(project_dir.join("cool.lock")).unwrap();
    assert!(lockfile.contains("kind = \"path\""));
    assert!(lockfile.contains("path = \"../toolkit\""));
    assert!(lockfile.contains("version = \"0.2.1\""));

    let run_output = run_cool_path_with_args(&project_dir.join("src").join("main.cool"), &[]).unwrap();
    let _ = std::fs::remove_dir_all(&workspace_dir);
    assert_eq!(run_output.trim(), "77");
}

#[test]
fn test_cool_add_git_dependency_installs_and_runs() {
    let workspace_dir = unique_temp_dir("cool_add_git_dependency");
    let project_dir = workspace_dir.join("app");
    let dep_dir = workspace_dir.join("toolkit_repo");
    let dep_rev = write_git_dependency_repo(&dep_dir, 91);
    write_basic_project(&project_dir, "demo", "import toolkit.util\nprint(util.value)\n");

    let (stdout, stderr, code) =
        run_cool_subcommand_in_dir(&project_dir, &["add", "toolkit", "--git", "../toolkit_repo"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stdout.contains("Added dependency 'toolkit'"));
    assert!(stderr.trim().is_empty());

    let manifest = std::fs::read_to_string(project_dir.join("cool.toml")).unwrap();
    let parsed: toml::Value = manifest.parse().unwrap();
    assert_eq!(
        parsed["dependencies"]["toolkit"]["git"].as_str(),
        Some("../toolkit_repo")
    );
    assert!(project_dir
        .join(".cool")
        .join("deps")
        .join("toolkit")
        .join(".git")
        .exists());

    let lockfile = std::fs::read_to_string(project_dir.join("cool.lock")).unwrap();
    assert!(lockfile.contains("kind = \"git\""));
    assert!(lockfile.contains(&format!("rev = \"{dep_rev}\"")));

    let run_output = run_cool_path_with_args(&project_dir.join("src").join("main.cool"), &[]).unwrap();
    let _ = std::fs::remove_dir_all(&workspace_dir);
    assert_eq!(run_output.trim(), "91");
}

#[test]
fn test_cool_task_list_and_run() {
    let temp_dir = unique_temp_dir("cool_task_runner");
    write_task_project(&temp_dir);

    let (list_stdout, list_stderr, list_code) = run_cool_subcommand_in_dir(&temp_dir, &["task", "list"]).unwrap();
    assert_eq!(list_code, 0, "stderr:\n{list_stderr}");
    assert!(list_stdout.contains("prepare - Prepare output"));
    assert!(list_stdout.contains("hello - Say hello"));
    assert!(list_stdout.contains("cwd - Show task cwd"));

    let (run_stdout, run_stderr, run_code) =
        run_cool_subcommand_in_dir(&temp_dir, &["task", "hello", "world"]).unwrap();
    let _ = std::fs::remove_dir_all(&temp_dir);

    assert_eq!(run_code, 0, "stderr:\n{run_stderr}");
    assert!(run_stdout.contains("prep"));
    assert!(run_stdout.contains("hello world"));
    assert!(run_stdout.contains("done"));
    assert!(run_stdout.find("prep").unwrap() < run_stdout.find("hello world").unwrap());
}

#[test]
fn test_cool_task_respects_task_cwd() {
    let temp_dir = unique_temp_dir("cool_task_runner_cwd");
    write_task_project(&temp_dir);

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&temp_dir, &["task", "cwd"]).unwrap();
    let _ = std::fs::remove_dir_all(&temp_dir);

    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert!(stdout.contains("/scripts"));
}

#[test]
fn test_vm_term_get_char() {
    let source_path = unique_temp_path("cool_vm_term_module", "cool");
    std::fs::write(
        &source_path,
        "import term\nterm.raw()\nch = term.get_char()\nterm.normal()\nprint(ch)\n",
    )
    .unwrap();

    let (stdout, _stderr, status) =
        run_cool_with_pty_input_delayed_close(source_path.to_str().unwrap(), &["--vm"], b"q", 100).unwrap();
    let _ = std::fs::remove_file(&source_path);

    assert_eq!(status, 0, "stdout:\n{stdout}");
    assert!(stdout.trim_end().ends_with('q'));
}

#[test]
fn test_self_hosted_compiler_suite_runs() {
    let output = Command::new(cool_bin()).arg("coolc/compiler_vm.cool").output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("=== Self-Hosted Compiler v2.0 ==="));
    assert!(stdout.contains("=== All tests complete ==="));
    assert!(stdout.contains("-- Inheritance --"));
}

#[test]
fn test_self_hosted_compiler_bootstrap_mode() {
    let output = Command::new(cool_bin())
        .args(["coolc/compiler_vm.cool", "--bootstrap"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("=== BOOTSTRAP: Self-hosted compiler compiling itself ==="));
    assert!(stdout.contains("Bootstrap phase: lexing..."));
    assert!(stdout.contains("Bootstrap phase: parsing..."));
    assert!(stdout.contains("Bootstrap phase: codegen..."));
    assert!(stdout.contains("=== Bootstrap SUCCESS! ==="));
}

#[test]
fn test_http_app_cli_args() {
    let temp_dir = unique_temp_dir("cool_http_app_test");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    let body_path = temp_dir.join("body.txt");
    std::fs::write(&body_path, "hello from cool http app\n").unwrap();
    let url = format!("file://{}", body_path.display());

    let output = Command::new(cool_bin())
        .args(["coolapps/http.cool", "get", &url])
        .output()
        .unwrap();

    let _ = std::fs::remove_dir_all(&temp_dir);

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("hello from cool http app"));
}

#[test]
fn test_http_app_getjson_and_head() {
    let temp_dir = unique_temp_dir("cool_http_app_json_test");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    let json_path = temp_dir.join("data.json");
    std::fs::write(&json_path, "{\"ok\":true,\"n\":2}\n").unwrap();
    let json_url = format!("file://{}", json_path.display());

    let getjson_output = Command::new(cool_bin())
        .args(["coolapps/http.cool", "getjson", &json_url])
        .output()
        .unwrap();
    assert!(getjson_output.status.success());
    let getjson_stdout = String::from_utf8_lossy(&getjson_output.stdout);
    assert!(getjson_stdout.contains("\"ok\": true"));
    assert!(getjson_stdout.contains("\"n\": 2"));

    let body_path = temp_dir.join("body.txt");
    std::fs::write(&body_path, "plain body\n").unwrap();
    let body_url = format!("file://{}", body_path.display());

    let head_output = Command::new(cool_bin())
        .args(["coolapps/http.cool", "head", &body_url])
        .output()
        .unwrap();

    let _ = std::fs::remove_dir_all(&temp_dir);

    assert!(head_output.status.success());
}

#[test]
fn test_runfile_passes_program_args() {
    let temp_dir = unique_temp_dir("cool_runfile_args_test");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    let child_path = temp_dir.join("child.cool");
    let main_path = temp_dir.join("main.cool");
    std::fs::write(
        &child_path,
        "import sys\nprint(sys.argv[0])\nprint(sys.argv[1])\nprint(sys.argv[2])\n",
    )
    .unwrap();
    std::fs::write(
        &main_path,
        format!("runfile(\"{}\", [\"one\", \"two\"])\n", child_path.display()),
    )
    .unwrap();

    let output = Command::new(cool_bin()).arg(&main_path).output().unwrap();

    let _ = std::fs::remove_dir_all(&temp_dir);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("child.cool"));
    assert!(stdout.contains("\none\n"));
    assert!(stdout.contains("\ntwo\n"));
}

#[test]
fn test_shell_http_app_launch() {
    let result = run_cool_stdin_with_args("coolapps/shell.cool", &[], "http help\nexit\n").unwrap();
    assert!(result.contains("http v1.0 — simple HTTP client"));
    assert!(result.contains("http get <url>"));
}

#[test]
fn test_shell_alias_env_and_history() {
    let input = "set NAME=Cool\necho $NAME\nalias hi echo hello\nhi\necho one\necho two\nhistory\nexit\n";
    let result = run_cool_stdin_with_args("coolapps/shell.cool", &[], input).unwrap();
    assert!(result.contains("Cool"));
    assert!(result.contains("hello"));
    assert!(result.contains("0  set NAME=Cool"));
    assert!(result.contains("6  history"));
}

#[test]
fn test_shell_source_and_pipe() {
    let temp_dir = unique_temp_dir("cool_shell_source_test");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    let script_path = temp_dir.join("script.coolsh");
    let pipe_path = temp_dir.join("pipe.txt");
    std::fs::write(&script_path, "echo sourced\n").unwrap();
    std::fs::write(&pipe_path, "echo alpha\necho beta\n").unwrap();

    let input = format!(
        "source {}\ncat {} | grep beta\nexit\n",
        script_path.display(),
        pipe_path.display()
    );
    let result = run_cool_stdin_with_args("coolapps/shell.cool", &[], &input).unwrap();

    let _ = std::fs::remove_dir_all(&temp_dir);
    assert!(result.contains("sourced"));
    assert!(result.contains("echo beta"));
}

#[test]
fn test_shell_run_passes_program_args() {
    let temp_dir = unique_temp_dir("cool_shell_run_args_test");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    let script_path = temp_dir.join("argv_app.cool");
    std::fs::write(&script_path, "import sys\nprint(sys.argv[1])\nprint(sys.argv[2])\n").unwrap();

    let input = format!("run {} one two\nexit\n", script_path.display());
    let result = run_cool_stdin_with_args("coolapps/shell.cool", &[], &input).unwrap();

    let _ = std::fs::remove_dir_all(&temp_dir);
    assert!(result.contains("one"));
    assert!(result.contains("two"));
}

#[test]
fn test_shell_calc_app_launch() {
    let result = run_cool_stdin_with_args("coolapps/shell.cool", &[], "calc\n2 + 3\nexit\nexit\n").unwrap();
    assert!(result.contains("calc v1.0 — expression calculator"));
    assert!(result.contains("= 5"));
}

#[test]
fn test_calc_app_persistent_variables() {
    let input = "x = 5\nx * 2\nexit\n";
    let result = run_cool_stdin_with_args("coolapps/calc.cool", &[], input).unwrap();
    assert!(result.contains("= 10"));
}

#[test]
fn test_shell_notes_app_launch() {
    let result = run_cool_stdin_with_args("coolapps/shell.cool", &[], "notes\nexit\nexit\n").unwrap();
    assert!(result.contains("notes v1.0 — commands:"));
    assert!(result.contains("new <name>"));
}

#[test]
fn test_notes_app_crud_flow() {
    let temp_dir = unique_temp_dir("cool_notes_app_test");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    let input = "new demo\nfirst line\nsecond line\n\nshow demo\nappend demo\nextra\nshow demo\nsearch second\ndelete demo\nlist\nexit\n";
    let mut cmd = Command::new(cool_bin());
    cmd.arg("coolapps/notes.cool");
    cmd.env("HOME", &temp_dir);
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn().unwrap();
    {
        let mut child_stdin = child.stdin.take().unwrap();
        child_stdin.write_all(input.as_bytes()).unwrap();
    }
    let output = child.wait_with_output().unwrap();

    let _ = std::fs::remove_dir_all(&temp_dir);

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Saved 'demo'."));
    assert!(stdout.contains("=== demo ==="));
    assert!(stdout.contains("first line"));
    assert!(stdout.contains("second line"));
    assert!(stdout.contains("extra"));
    assert!(stdout.contains("Deleted 'demo'."));
    assert!(stdout.contains("No notes yet. Use 'new <name>' to create one."));
}

#[test]
fn test_edit_app_can_save_empty_existing_file() {
    let file_path = unique_temp_path("cool_edit_app_test", "txt");
    std::fs::write(&file_path, "").unwrap();

    let (stdout, _stderr, status) =
        run_cool_with_pty_input("coolapps/edit.cool", &[file_path.to_str().unwrap()], b"abc\x18y").unwrap();

    let saved = std::fs::read_to_string(&file_path).unwrap();
    let _ = std::fs::remove_file(&file_path);

    assert_eq!(status, 0);
    assert!(stdout.contains("Save before exit? (y/n)"));
    assert_eq!(saved, "abc\n");
}

#[test]
fn test_snake_app_quits_on_q() {
    let (stdout, _stderr, status) = run_cool_with_pty_input("coolapps/snake.cool", &[], b"q").unwrap();
    assert_eq!(status, 0);
    assert!(stdout.contains("Game over! Final score:"));
}

#[test]
fn test_break_continue() {
    let result =
        run_cool("result = []\nfor i in range(10):\n\tif i == 5:\n\t\tbreak\n\tresult.append(i)\nprint(len(result))")
            .unwrap();
    assert!(result.contains("5"));
}

// ── LSP tests ─────────────────────────────────────────────────────────────────

fn lsp_message(body: &str) -> Vec<u8> {
    format!("Content-Length: {}\r\n\r\n{}", body.len(), body).into_bytes()
}

fn read_lsp_response(reader: &mut impl std::io::BufRead) -> serde_json::Value {
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let trimmed = line.trim_end_matches(|c: char| c == '\r' || c == '\n');
        if trimmed.is_empty() {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix("Content-Length: ") {
            content_length = rest.trim().parse().unwrap_or(0);
        }
    }
    let mut buf = vec![0u8; content_length];
    reader.read_exact(&mut buf).unwrap();
    serde_json::from_slice(&buf).unwrap()
}

#[test]
fn test_lsp_initialize() {
    use std::io::{BufReader, Write};
    use std::process::{Command, Stdio};

    let mut child = Command::new(cool_bin())
        .arg("lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn cool lsp");

    let stdin = child.stdin.as_mut().unwrap();
    let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#;
    stdin.write_all(&lsp_message(init)).unwrap();

    let stdout = child.stdout.as_mut().unwrap();
    let mut reader = BufReader::new(stdout);
    let response = read_lsp_response(&mut reader);

    assert_eq!(response["id"], 1);
    let caps = &response["result"]["capabilities"];
    assert!(caps["hoverProvider"].as_bool().unwrap_or(false));
    assert!(caps["definitionProvider"].as_bool().unwrap_or(false));
    assert!(caps["documentSymbolProvider"].as_bool().unwrap_or(false));
    assert_eq!(caps["textDocumentSync"]["change"], 1);

    let shutdown = r#"{"jsonrpc":"2.0","id":2,"method":"shutdown","params":null}"#;
    child.stdin.as_mut().unwrap().write_all(&lsp_message(shutdown)).unwrap();
    let shutdown_resp = read_lsp_response(&mut reader);
    assert_eq!(shutdown_resp["id"], 2);
    assert!(shutdown_resp["result"].is_null());

    let exit = r#"{"jsonrpc":"2.0","method":"exit","params":null}"#;
    child.stdin.as_mut().unwrap().write_all(&lsp_message(exit)).unwrap();
    let _ = child.wait();
}

#[test]
fn test_lsp_diagnostics_on_parse_error() {
    use std::io::{BufReader, Write};
    use std::process::{Command, Stdio};

    let mut child = Command::new(cool_bin())
        .arg("lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn cool lsp");

    let stdin = child.stdin.as_mut().unwrap();
    let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#;
    stdin.write_all(&lsp_message(init)).unwrap();

    let stdout = child.stdout.as_mut().unwrap();
    let mut reader = BufReader::new(stdout);
    let _init_resp = read_lsp_response(&mut reader);

    let did_open = r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///tmp/test.cool","languageId":"cool","version":1,"text":"def foo(\n  x = \n"}}}"#;
    child.stdin.as_mut().unwrap().write_all(&lsp_message(did_open)).unwrap();

    let notification = read_lsp_response(&mut reader);
    assert_eq!(notification["method"], "textDocument/publishDiagnostics");
    let diags = &notification["params"]["diagnostics"];
    assert!(diags.as_array().map(|a| !a.is_empty()).unwrap_or(false));

    let exit = r#"{"jsonrpc":"2.0","method":"exit","params":null}"#;
    child.stdin.as_mut().unwrap().write_all(&lsp_message(exit)).unwrap();
    let _ = child.wait();
}

#[test]
fn test_lsp_document_symbols() {
    use std::io::{BufReader, Write};
    use std::process::{Command, Stdio};

    let mut child = Command::new(cool_bin())
        .arg("lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn cool lsp");

    let stdin = child.stdin.as_mut().unwrap();
    let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#;
    stdin.write_all(&lsp_message(init)).unwrap();

    let stdout = child.stdout.as_mut().unwrap();
    let mut reader = BufReader::new(stdout);
    let _init_resp = read_lsp_response(&mut reader);

    let source = "def greet(name):\n    print(name)\n\nclass Dog:\n    def bark(self):\n        print(\"woof\")\n";
    let did_open_val = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": "file:///tmp/sym_test.cool",
                "languageId": "cool",
                "version": 1,
                "text": source
            }
        }
    });
    let did_open = serde_json::to_string(&did_open_val).unwrap();
    child.stdin.as_mut().unwrap().write_all(&lsp_message(&did_open)).unwrap();
    let _diag_notif = read_lsp_response(&mut reader); // publishDiagnostics

    let sym_req = r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/documentSymbol","params":{"textDocument":{"uri":"file:///tmp/sym_test.cool"}}}"#;
    child.stdin.as_mut().unwrap().write_all(&lsp_message(sym_req)).unwrap();
    let sym_resp = read_lsp_response(&mut reader);

    assert_eq!(sym_resp["id"], 2);
    let symbols = sym_resp["result"].as_array().unwrap();
    let names: Vec<&str> = symbols.iter().filter_map(|s| s["name"].as_str()).collect();
    assert!(names.contains(&"greet"), "expected greet in symbols: {names:?}");
    assert!(names.contains(&"Dog"), "expected Dog in symbols: {names:?}");
    assert!(names.contains(&"bark"), "expected bark in symbols: {names:?}");

    let exit = r#"{"jsonrpc":"2.0","method":"exit","params":null}"#;
    child.stdin.as_mut().unwrap().write_all(&lsp_message(exit)).unwrap();
    let _ = child.wait();
}
