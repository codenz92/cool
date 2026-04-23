use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static LLVM_BUILD_LOCK: Mutex<()> = Mutex::new(());

fn cool_bin() -> &'static str {
    env!("CARGO_BIN_EXE_cool")
}

fn unique_test_path(stem: &str, ext: &str) -> PathBuf {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    PathBuf::from(format!("{stem}_{nonce}.{ext}"))
}

fn unique_temp_dir(stem: &str) -> PathBuf {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    std::env::current_dir().unwrap().join(format!("{stem}_{nonce}"))
}

fn cleanup_native_artifacts(source_path: &PathBuf, binary_path: &PathBuf) {
    let _ = fs::remove_file(source_path);
    let _ = fs::remove_file(binary_path);
}

fn compile_and_run_native(source: &str) -> Result<String, String> {
    compile_and_run_native_with_env(source, &[])
}

fn compile_and_run_native_with_env(source: &str, envs: &[(&str, &str)]) -> Result<String, String> {
    let _guard = LLVM_BUILD_LOCK.lock().unwrap();
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let source_path = cwd.join(unique_test_path("temp_llvm_test", "cool"));
    let binary_path = source_path.with_extension("");

    fs::write(&source_path, source).map_err(|e| e.to_string())?;

    let build_output = Command::new(cool_bin())
        .args(["build", source_path.to_str().unwrap()])
        .output()
        .map_err(|e| e.to_string())?;

    if !build_output.status.success() {
        let stderr = String::from_utf8_lossy(&build_output.stderr).to_string();
        let stdout = String::from_utf8_lossy(&build_output.stdout).to_string();
        cleanup_native_artifacts(&source_path, &binary_path);
        return Err(format!("{stdout}{stderr}"));
    }

    let mut run_cmd = Command::new(&binary_path);
    for (k, v) in envs {
        run_cmd.env(k, v);
    }
    let run_output = match run_cmd.output() {
        Ok(output) => output,
        Err(e) => {
            cleanup_native_artifacts(&source_path, &binary_path);
            return Err(e.to_string());
        }
    };

    cleanup_native_artifacts(&source_path, &binary_path);

    if run_output.status.success() {
        Ok(String::from_utf8_lossy(&run_output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&run_output.stderr).to_string())
    }
}

fn compile_and_run_native_path(source_path: &PathBuf) -> Result<String, String> {
    let _guard = LLVM_BUILD_LOCK.lock().unwrap();
    let binary_path = source_path.with_extension("");

    let build_output = Command::new(cool_bin())
        .args(["build", source_path.to_str().unwrap()])
        .output()
        .map_err(|e| e.to_string())?;

    if !build_output.status.success() {
        let stderr = String::from_utf8_lossy(&build_output.stderr).to_string();
        let stdout = String::from_utf8_lossy(&build_output.stdout).to_string();
        let _ = fs::remove_file(&binary_path);
        return Err(format!("{stdout}{stderr}"));
    }

    let run_output = Command::new(&binary_path).output().map_err(|e| e.to_string())?;
    let _ = fs::remove_file(&binary_path);

    if run_output.status.success() {
        Ok(String::from_utf8_lossy(&run_output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&run_output.stderr).to_string())
    }
}

fn compile_and_run_native_expect_runtime_error(source: &str) -> String {
    let _guard = LLVM_BUILD_LOCK.lock().unwrap();
    let cwd = std::env::current_dir().unwrap();
    let source_path = cwd.join(unique_test_path("temp_llvm_test", "cool"));
    let binary_path = source_path.with_extension("");

    fs::write(&source_path, source).unwrap();

    let build_output = Command::new(cool_bin())
        .args(["build", source_path.to_str().unwrap()])
        .output()
        .unwrap();

    if !build_output.status.success() {
        let stderr = String::from_utf8_lossy(&build_output.stderr).to_string();
        let stdout = String::from_utf8_lossy(&build_output.stdout).to_string();
        cleanup_native_artifacts(&source_path, &binary_path);
        panic!("expected native build to succeed, got:\n{stdout}{stderr}");
    }

    let run_output = Command::new(&binary_path).output().unwrap();
    cleanup_native_artifacts(&source_path, &binary_path);

    assert!(!run_output.status.success(), "expected native run to fail");
    let stdout = String::from_utf8_lossy(&run_output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&run_output.stderr).to_string();
    format!("{stdout}{stderr}")
}

fn compile_and_run_native_manual(source: &str, envs: &[(&str, &str)]) -> Result<(String, PathBuf), String> {
    let _guard = LLVM_BUILD_LOCK.lock().unwrap();
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let source_path = cwd.join(unique_test_path("temp_llvm_test", "cool"));
    let binary_path = source_path.with_extension("");

    fs::write(&source_path, source).map_err(|e| e.to_string())?;

    let build_output = Command::new(cool_bin())
        .args(["build", source_path.to_str().unwrap()])
        .output()
        .map_err(|e| e.to_string())?;

    if !build_output.status.success() {
        let stderr = String::from_utf8_lossy(&build_output.stderr).to_string();
        let stdout = String::from_utf8_lossy(&build_output.stdout).to_string();
        cleanup_native_artifacts(&source_path, &binary_path);
        return Err(format!("{stdout}{stderr}"));
    }

    let mut run_cmd = Command::new(&binary_path);
    for (k, v) in envs {
        run_cmd.env(k, v);
    }
    let run_output = match run_cmd.output() {
        Ok(output) => output,
        Err(e) => {
            cleanup_native_artifacts(&source_path, &binary_path);
            return Err(e.to_string());
        }
    };

    let stdout = String::from_utf8_lossy(&run_output.stdout).to_string();
    cleanup_native_artifacts(&source_path, &binary_path);

    if run_output.status.success() {
        Ok((stdout, source_path))
    } else {
        Err(String::from_utf8_lossy(&run_output.stderr).to_string())
    }
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
fn test_llvm_default_kwargs_and_sorted() {
    let result = compile_and_run_native(
        r#"
greeting = "hello"
print(greeting.upper() + ", world!")

def greet(name, title=""):
    if title != "":
        return f"Hello, {title} {name}!"
    return f"Hello, {name}!"

nums = [4, 1, 3, 2]
print(greet("Alice"))
print(greet("Smith", title="Dr."))
print(sorted(nums))
"#,
    )
    .unwrap();

    assert!(result.contains("HELLO, world!"));
    assert!(result.contains("Hello, Alice!"));
    assert!(result.contains("Hello, Dr. Smith!"));
    assert!(result.contains("[1,2,3,4]") || result.contains("[1, 2, 3, 4]"));
}

#[test]
fn test_llvm_class_slice_and_builtins() {
    let result = compile_and_run_native(
        r#"
class Base:
    def __init__(self, x=2):
        self.x = x

    def __str__(self):
        return f"Base({self.x})"

class Child(Base):
    def area(self, r):
        return round(r * r, 2)

obj = Child()
vals = [1, 2, 3, 4]
print(str(obj))
print(vals[1:3])
print(sum(vals))
print(min(9, 4, 7))
print(max(9, 4, 7))
print(isinstance(obj, "Child"))
print(obj.area(2))
"#,
    )
    .unwrap();

    assert!(result.contains("Base(2)"));
    assert!(result.contains("[2, 3]") || result.contains("[2,3]"));
    assert!(result.contains("\n10\n"));
    assert!(result.contains("\n4\n"));
    assert!(result.contains("\n9\n"));
    assert!(result.contains("true"));
}

#[test]
fn test_llvm_scalar_conversion_builtins() {
    let result = compile_and_run_native(
        r#"
print(abs(-7))
print(abs(-3.5))
print(int("42"))
print(int(true))
print(float("2.5"))
print(float(4))
print(bool(""))
print(bool("cool"))
"#,
    )
    .unwrap();

    assert!(result.contains("\n7\n") || result.starts_with("7\n"));
    assert!(result.contains("3.5"));
    assert!(result.contains("42"));
    assert!(result.contains("\n1\n"));
    assert!(result.contains("2.5"));
    assert!(result.contains("\n4\n"));
    assert!(result.contains("false"));
    assert!(result.contains("true"));
}

#[test]
fn test_llvm_import_math_module() {
    let result = compile_and_run_native(
        r#"
import math
print(math)
print(math.pi)
print(math.sqrt(4))
print(math.pow(2, 5))
print(math.floor(3.9))
print(math.ceil(3.1))
print(math.abs(-7))
print(math.round(3.5))
print(math.round(3.14159, 2))
print(math.trunc(3.9))
print(math.log(100, 10))
print(math.log2(8))
print(math.exp2(4))
print(math.isfinite(1.0))
"#,
    )
    .unwrap();

    assert!(result.contains("<module math>"));
    assert!(result.contains("3.14159"));
    assert!(result.contains("\n2\n") || result.contains("\n2.0\n"));
    assert!(result.contains("32"));
    assert!(result.contains("\n3\n4\n7\n4\n3.14\n"));
    assert!(result.contains("3.14"));
    assert!(result.contains("\n3\n") || result.contains("\n3.0\n"));
    assert!(result.matches("\n2\n").count() >= 1 || result.contains("\n2.0\n"));
    assert!(result.matches("\n3\n").count() >= 2 || result.matches("\n3.0\n").count() >= 2);
    assert!(result.contains("\n16\n") || result.contains("\n16.0\n"));
    assert!(result.contains("true"));
}

#[test]
fn test_llvm_import_os_module() {
    let cwd = std::env::current_dir().unwrap();
    let temp_dir = cwd.join(unique_test_path("temp_llvm_os_dir", "d"));
    fs::create_dir_all(&temp_dir).unwrap();
    fs::write(temp_dir.join("sample.txt"), "ok").unwrap();

    let source = format!(
        r#"
import os
print(os)
print(os.getcwd())
joined = os.join("{dir}", "sample.txt")
print(os.exists(joined))
print(os.getenv("COOL_LLVM_OS_ENV"))
print(os.path("a", "b", "c"))
print(os.popen("printf llvm-os"))
nested = os.join("{dir}", "nested", "deeper")
os.mkdir(nested)
print(os.exists(nested))
print(os.listdir("{dir}"))
"#,
        dir = temp_dir.display()
    );

    let result = compile_and_run_native_with_env(&source, &[("COOL_LLVM_OS_ENV", "present")]).unwrap();

    let _ = fs::remove_dir_all(&temp_dir);

    assert!(result.contains("<module os>"));
    assert!(result.contains(&cwd.display().to_string()));
    assert!(result.contains("true"));
    assert!(result.contains("present"));
    assert!(result.contains("a/b/c"));
    assert!(result.contains("llvm-os"));
    assert!(result.matches("true").count() >= 2);
    assert!(result.contains("sample.txt"));
}

#[test]
fn test_llvm_import_sys_module() {
    let (result, source_path) = compile_and_run_native_manual(
        r#"
import sys
print(sys)
print(sys.argv[0])
print(len(sys.argv))
print(sys.argv[1])
"#,
        &[("COOL_PROGRAM_ARGS", "alpha\x1Fbeta")],
    )
    .unwrap();

    assert!(result.contains("<module sys>"));
    assert!(result.contains(&source_path.display().to_string()));
    assert!(result.contains("alpha"));
}

#[test]
fn test_llvm_import_argparse_module() {
    let result = compile_and_run_native(
        r#"
import argparse
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
fn test_llvm_import_logging_module() {
    let cwd = std::env::current_dir().unwrap();
    let log_path = cwd.join(unique_test_path("temp_llvm_logging_module", "log"));
    let source = format!(
        r#"
import logging
logging.basic_config({{"level": "INFO", "format": "{{timestamp}}|{{level}}|{{name}}|{{message}}", "stdout": false, "file": "{file}", "append": false}})
logging.debug("hidden", "demo")
logging.info("shown", "demo")
logging.warning("warned", "demo")
logging.error("boom", "demo")
"#,
        file = log_path.display()
    );

    let result = compile_and_run_native(&source).unwrap();
    let contents = fs::read_to_string(&log_path).unwrap();
    let _ = fs::remove_file(&log_path);

    assert!(result.trim().is_empty());
    assert_logging_file_output(&contents);
}

#[test]
fn test_llvm_argparse_uses_process_args_by_default() {
    let result = compile_and_run_native_with_env(
        r#"
import argparse
spec = {
    "positionals": [{"name": "action"}],
    "options": [{"name": "count", "short": "c", "type": "int", "default": 1}]
}
args = argparse.parse(spec)
print(args["action"])
print(args["count"])
"#,
        &[("COOL_PROGRAM_ARGS", "deploy\x1F-c\x1F3")],
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["deploy", "3"]);
}

#[test]
fn test_llvm_import_time_module() {
    let result = compile_and_run_native(
        r#"
import time
print(time)
t1 = time.monotonic()
time.sleep(0.01)
t2 = time.monotonic()
print(t1 > 0)
print(t2 >= t1)
print(time.time() > 0)
"#,
    )
    .unwrap();

    assert!(result.contains("<module time>"));
    assert!(result.matches("true").count() >= 3);
}

#[test]
fn test_llvm_import_random_module() {
    let result = compile_and_run_native(
        r#"
import random
print(random)
random.seed(42)
a = random.random()
b = random.random()
random.seed(42)
print(a == random.random())
print(b == random.random())
n = random.randint(3, 7)
print(n >= 3)
print(n <= 7)
u = random.uniform(10, 20)
print(u >= 10)
print(u <= 20)
items = [1, 2, 3, 4]
random.seed(7)
picked = random.choice(items)
print(picked >= 1)
print(picked <= 4)
pair = ("x", "y")
print(random.choice(pair) in pair)
random.shuffle(items)
print(len(items))
print(sum(items))
"#,
    )
    .unwrap();

    assert!(result.contains("<module random>"));
    assert!(result.matches("true").count() >= 9);
    assert!(result.contains("\n4\n"));
    assert!(result.contains("\n10\n"));
}

#[test]
fn test_llvm_import_json_module() {
    let result = compile_and_run_native(
        r#"
import json
print(json)
data = json.loads('{"name":"Alice","scores":[95,87],"ok":true,"meta":null}')
print(data["name"])
print(data["scores"][1])
print(data["ok"])
print(data["meta"])
print(json.dumps({"user": data["name"], "count": len(data["scores"]), "vals": [1, 2, 3]}))
"#,
    )
    .unwrap();

    assert!(result.contains("<module json>"));
    assert!(result.contains("Alice"));
    assert!(result.contains("\n87\n"));
    assert!(result.contains("true"));
    assert!(result.contains("nil"));
    assert!(result.contains("\"user\": \"Alice\""));
    assert!(result.contains("\"count\": 2"));
    assert!(result.contains("\"vals\": [1, 2, 3]"));
}

#[test]
fn test_llvm_import_string_module() {
    let result = compile_and_run_native(
        r#"
import string
print(string)
print(string.split("a,b,c", ","))
print(string.join(" | ", ["a", "b", "c"]))
print(string.upper("hello"))
print(string.replace("abcabc", "a", "X"))
print(string.startswith("hello", "he"))
print(string.endswith("hello", "lo"))
print(string.find("hello", "ll"))
print(string.count("hello", "l"))
print(string.title("hello world"))
print(string.capitalize("hello world"))
print(string.format("hi {}, {}", "cool", 7))
"#,
    )
    .unwrap();

    assert!(result.contains("<module string>"));
    assert!(result.contains("[a, b, c]") || result.contains("[a,b,c]"));
    assert!(result.contains("a | b | c"));
    assert!(result.contains("HELLO"));
    assert!(result.contains("XbcXbc"));
    assert!(result.matches("true").count() >= 2);
    assert!(result.contains("\n2\n"));
    assert!(result.contains("Hello World"));
    assert!(result.contains("Hello world"));
    assert!(result.contains("hi cool, 7"));
}

#[test]
fn test_llvm_import_list_module() {
    let result = compile_and_run_native(
        r#"
def double(x):
    return x * 2

def gt_two(x):
    return x > 2

def add(acc, x):
    return acc + x

import list
print(list)
nums = [3, 1, 2]
print(list.sort(nums))
print(list.reverse(nums))
print(list.map(double, nums))
print(list.filter(gt_two, [1, 2, 3, 4]))
print(list.reduce(add, [1, 2, 3, 4], 0))
print(list.reduce(add, [1, 2, 3, 4]))
print(list.flatten([[1, 2], [3], 4]))
print(list.unique([1, 1, 2, 2, 3]))
"#,
    )
    .unwrap();

    assert!(result.contains("<module list>"));
    assert!(result.contains("[1, 2, 3]") || result.contains("[1,2,3]"));
    assert!(result.contains("[2, 1, 3]") || result.contains("[2,1,3]"));
    assert!(
        result.contains("[6, 2, 4]")
            || result.contains("[6,2,4]")
            || result.contains("[2, 6, 4]")
            || result.contains("[2,6,4]")
    );
    assert!(result.contains("[3, 4]") || result.contains("[3,4]"));
    assert!(result.contains("\n10\n10\n"));
    assert!(result.contains("[1, 2, 3, 4]") || result.contains("[1,2,3,4]"));
}

#[test]
fn test_llvm_import_re_module() {
    let result = compile_and_run_native(
        r#"
import re
print(re)
print(re.match("^\d+$", "123"))
print(re.search("\d+", "abc123def"))
print(re.fullmatch("\d+", "12345"))
print(re.findall("\d+", "a1 b22 c333"))
print(re.sub("\d", "a1b2c3", "X"))
print(re.split(",\s*", "a, b,  c"))
"#,
    )
    .unwrap();

    assert!(result.contains("<module re>"));
    assert!(result.contains("\n123\n"));
    assert!(result.contains("[1, 22, 333]") || result.contains("[1,22,333]"));
    assert!(result.contains("aXbXcX"));
    assert!(result.contains("[a, b, c]") || result.contains("[a,b,c]"));
}

#[test]
fn test_llvm_import_subprocess_module() {
    let result = compile_and_run_native(
        r#"
import subprocess
res = subprocess.run("printf 'out'; printf 'err' 1>&2; exit 7")
print(res["code"])
print(res["stdout"])
print(res["stderr"])
print(res["timed_out"])
print(res["ok"])
print(subprocess.call("exit 3"))
print(subprocess.check_output("printf 'hi'"))
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["7", "out", "err", "false", "false", "3", "hi"]);
}

#[test]
fn test_llvm_import_subprocess_timeout() {
    let result = compile_and_run_native(
        r#"
import subprocess
res = subprocess.run("sleep 1", 0.05)
print(res["timed_out"])
print(res["code"] == nil)
print(res["ok"])
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["true", "true", "false"]);
}

#[test]
fn test_llvm_try_except_catches_raised_value() {
    let result = compile_and_run_native(
        r#"
try:
    raise "boom"
except as err:
    print("caught")
    print(err)
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["caught", "boom"]);
}

#[test]
fn test_llvm_try_except_matches_parent_handler() {
    let result = compile_and_run_native(
        r#"
class BaseErr:
    pass

class SubErr(BaseErr):
    pass

try:
    raise SubErr()
except BaseErr:
    print("caught")
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["caught"]);
}

#[test]
fn test_llvm_try_else_finally() {
    let result = compile_and_run_native(
        r#"
try:
    print("body")
except:
    print("except")
else:
    print("else")
finally:
    print("finally")
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["body", "else", "finally"]);
}

#[test]
fn test_llvm_try_finally_cleans_on_continue() {
    let result = compile_and_run_native(
        r#"
for i in [1, 2]:
    try:
        if i == 1:
            continue
        print(i)
    finally:
        print("finally")
        print(i)
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["finally", "1", "2", "finally", "2"]);
}

#[test]
fn test_llvm_bare_raise_reraises_current_exception() {
    let result = compile_and_run_native(
        r#"
try:
    try:
        raise "boom"
    except:
        print("inner")
        raise
except as err:
    print("outer")
    print(err)
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["inner", "outer", "boom"]);
}

#[test]
fn test_llvm_with_context_manager_cleans_on_unhandled_exception() {
    let output = compile_and_run_native_expect_runtime_error(
        r#"
import collections

class C:
    def __enter__(self):
        print("enter")
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        print("exit")

with C():
    s = collections.Stack()
    s.pop()
"#,
    );

    let lines: Vec<_> = output.lines().filter(|line| !line.is_empty()).collect();
    assert!(lines.starts_with(&["enter", "exit"]));
    assert!(output.contains("Unhandled exception: Stack is empty"));
}

#[test]
fn test_llvm_with_context_manager_cleans_on_caught_exception() {
    let result = compile_and_run_native(
        r#"
class C:
    def __enter__(self):
        print("enter")
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        print("exit")

try:
    with C():
        raise "boom"
except as err:
    print("caught")
    print(err)
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["enter", "exit", "caught", "boom"]);
}

#[test]
fn test_llvm_with_context_manager_uses_enter_result() {
    let result = compile_and_run_native(
        r#"
class C:
    def __enter__(self):
        print("enter")
        return 42

    def __exit__(self, exc_type, exc_val, exc_tb):
        print("exit")

with C() as value:
    print(value)
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["enter", "42", "exit"]);
}

#[test]
fn test_llvm_with_context_manager_cleans_on_return() {
    let result = compile_and_run_native(
        r#"
class C:
    def __enter__(self):
        print("enter")
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        print("exit")

def f():
    with C():
        return 7

print(f())
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["enter", "exit", "7"]);
}

#[test]
fn test_llvm_with_context_manager_cleans_on_continue() {
    let result = compile_and_run_native(
        r#"
class C:
    def __init__(self, name):
        self.name = name

    def __enter__(self):
        print("enter " + self.name)
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        print("exit " + self.name)

for i in range(2):
    with C(str(i)):
        if i == 0:
            continue
        print("body")
print("done")
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["enter 0", "exit 0", "enter 1", "body", "exit 1", "done"]);
}

#[test]
fn test_llvm_with_context_manager_break_only_cleans_exited_scope() {
    let result = compile_and_run_native(
        r#"
class C:
    def __init__(self, name):
        self.name = name

    def __enter__(self):
        print("enter " + self.name)
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        print("exit " + self.name)

with C("outer"):
    for i in range(2):
        with C("inner"):
            break
    print("after")
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(
        lines,
        ["enter outer", "enter inner", "exit inner", "after", "exit outer"]
    );
}

#[test]
fn test_llvm_import_collections_module() {
    let result = compile_and_run_native(
        r#"
import collections
print(collections)
q = collections.Queue()
q.enqueue("first")
q.enqueue("second")
print(q.dequeue())
print(q.size())
s = collections.Stack()
s.push("a")
s.push("b")
print(s.pop())
print(s.is_empty())
"#,
    )
    .unwrap();

    assert!(result.contains("<module collections>"));
    assert!(result.contains("first"));
    assert!(result.contains("\n1\n"));
    assert!(result.contains("\nb\n"));
    assert!(result.contains("false"));
}

#[test]
fn test_llvm_open_and_file_methods() {
    let cwd = std::env::current_dir().unwrap();
    let file_path = cwd.join(unique_test_path("temp_llvm_file_io", "txt"));

    let source = format!(
        r#"
path = "{path}"
with open(path, "w") as f:
    f.write("alpha\n")
    f.writelines(["beta\n", "gamma\n"])

with open(path) as f:
    print(f.readline().strip())
    rest = f.readlines()
    print(len(rest))
    print(rest[0].strip())
    print(rest[1].strip())

f = open(path, "a")
f.write("delta\n")
f.close()

reader = open(path, "r")
print(reader.read().strip())
reader.close()
"#,
        path = file_path.display()
    );

    let result = compile_and_run_native(&source).unwrap();
    let _ = fs::remove_file(&file_path);

    assert!(result.contains("alpha"));
    assert!(result.contains("\n2\n"));
    assert!(result.contains("beta"));
    assert!(result.contains("gamma"));
    assert!(result.contains("delta"));
}

#[test]
fn test_llvm_import_path_module() {
    let cwd = std::env::current_dir().unwrap();
    let file_path = cwd.join(unique_test_path("temp_llvm_path_module", "txt"));
    fs::write(&file_path, "ok").unwrap();

    let source = format!(
        r#"
import path
print(path)
print(path.join("a", "b", "c.txt"))
print(path.basename("a/b/c.txt"))
print(path.dirname("a/b/c.txt"))
print(path.ext("a/b/c.txt"))
print(path.stem("a/b/c.txt"))
print(path.split("a/b/c.txt"))
print(path.normalize("a/./b/../c//d.txt"))
print(path.exists("{file}"))
print(path.isabs("{file}"))
"#,
        file = file_path.display()
    );

    let result = compile_and_run_native(&source).unwrap();
    let _ = fs::remove_file(&file_path);

    assert!(result.contains("<module path>"));
    assert!(result.contains("a/b/c.txt"));
    assert!(result.contains("c.txt"));
    assert!(result.contains(".txt"));
    assert!(result.contains("\nc\n") || result.contains("\nc\r\n"));
    assert!(result.contains("[a/b, c.txt]") || result.contains("[a/b,c.txt]"));
    assert!(result.contains("a/c/d.txt"));
    assert!(result.matches("true").count() >= 2);
}

#[test]
fn test_llvm_import_csv_module() {
    let result = compile_and_run_native(
        r#"
import csv
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
fn test_llvm_import_datetime_module() {
    let result = compile_and_run_native(
        r#"
import datetime
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
fn test_llvm_import_hashlib_module() {
    let result = compile_and_run_native(
        r#"
import hashlib
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
fn test_llvm_import_test_module() {
    let result = compile_and_run_native(
        r#"
import test

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
fn test_llvm_import_ffi_module() {
    let result = compile_and_run_native(
        r#"
import ffi

libm = ffi.open("libm")
sqrt_fn = ffi.func(libm, "sqrt", "f64", ["f64"])
pow_fn = ffi.func(libm, "pow", "f64", ["f64", "f64"])

libc = ffi.open("libc")
abs_fn = ffi.func(libc, "abs", "i32", ["i32"])
strlen_fn = ffi.func(libc, "strlen", "u64", ["str"])
dup_fn = ffi.func(libc, "strdup", "str", ["str"])

print(sqrt_fn(81.0))
print(pow_fn(2.0, 5.0))
print(abs_fn(-42))
print(strlen_fn("cool"))
print(dup_fn("ffi-ok"))
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["9", "32", "42", "4", "ffi-ok"]);
}

#[test]
fn test_llvm_import_dotted_module_package_path() {
    let temp_dir = unique_temp_dir("cool_llvm_import_package_test");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(temp_dir.join("foo")).unwrap();
    let source_path = temp_dir.join("main.cool");
    fs::write(
        temp_dir.join("foo").join("bar.cool"),
        "value = 42\n\ndef add(x, y=1):\n    return x + y\n\nclass Box:\n    def __init__(self, value=0):\n        self.value = value\n",
    )
    .unwrap();
    fs::write(
        &source_path,
        "import foo.bar\nprint(bar.value)\nprint(bar.add(4))\nprint(bar.add(y=3, x=4))\nprint(bar.Box(9).value)\n",
    )
    .unwrap();

    let result = compile_and_run_native_path(&source_path).unwrap();

    let _ = fs::remove_dir_all(&temp_dir);
    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["42", "5", "7", "9"]);
}

#[test]
fn test_llvm_import_file_flattens_exports() {
    let temp_dir = unique_temp_dir("cool_llvm_import_file_test");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();
    let source_path = temp_dir.join("main.cool");
    fs::write(
        temp_dir.join("helper.cool"),
        "value = 10\n\ndef add(x, y=1):\n    return x + y\n\nclass Box:\n    def __init__(self, value=0):\n        self.value = value\n",
    )
    .unwrap();
    fs::write(
        &source_path,
        "import \"helper.cool\"\nprint(value)\nprint(add(4))\nprint(add(y=3, x=4))\nprint(Box(8).value)\n",
    )
    .unwrap();

    let result = compile_and_run_native_path(&source_path).unwrap();

    let _ = fs::remove_dir_all(&temp_dir);
    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["10", "5", "7", "8"]);
}
