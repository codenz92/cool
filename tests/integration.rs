// Integration tests for Cool language interpreter
// Run with: cargo test --test integration

use std::io::Write;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

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

fn assert_logging_file_output(contents: &str) {
    let lines: Vec<&str> = contents.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines.len(), 3);
    assert!(lines[0].chars().next().unwrap_or_default().is_ascii_digit());
    assert!(lines[0].contains("|INFO|demo|shown"));
    assert!(lines[1].contains("|WARNING|demo|warned"));
    assert!(lines[2].contains("|ERROR|demo|boom"));
    assert!(!contents.contains("hidden"));
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

    let _ = std::fs::remove_dir_all(&workspace_dir);
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
