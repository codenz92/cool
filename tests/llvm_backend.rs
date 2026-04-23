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
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    PathBuf::from(format!("{stem}_{nonce}.{ext}"))
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

fn compile_native_expect_error(source: &str) -> String {
    let _guard = LLVM_BUILD_LOCK.lock().unwrap();
    let cwd = std::env::current_dir().unwrap();
    let source_path = cwd.join(unique_test_path("temp_llvm_test", "cool"));
    let binary_path = source_path.with_extension("");

    fs::write(&source_path, source).unwrap();

    let build_output = Command::new(cool_bin())
        .args(["build", source_path.to_str().unwrap()])
        .output()
        .unwrap();

    cleanup_native_artifacts(&source_path, &binary_path);

    assert!(!build_output.status.success(), "expected native build to fail");
    let stderr = String::from_utf8_lossy(&build_output.stderr).to_string();
    let stdout = String::from_utf8_lossy(&build_output.stdout).to_string();
    format!("{stdout}{stderr}")
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
print(math.isfinite(1.0))
"#,
    )
    .unwrap();

    assert!(result.contains("<module math>"));
    assert!(result.contains("3.14159"));
    assert!(result.contains("\n2\n") || result.contains("\n2.0\n"));
    assert!(result.contains("32"));
    assert!(result.contains("\n3\n") || result.contains("\n3.0\n"));
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
print(os.listdir("{dir}"))
"#,
        dir = temp_dir.display()
    );

    let result = compile_and_run_native(&source).unwrap();

    let _ = fs::remove_dir_all(&temp_dir);

    assert!(result.contains("<module os>"));
    assert!(result.contains(&cwd.display().to_string()));
    assert!(result.contains("true"));
    assert!(result.contains("sample.txt"));
}

#[test]
fn test_llvm_import_sys_module() {
    let result = compile_and_run_native_with_env(
        r#"
import sys
print(sys)
print(len(sys.argv))
print(sys.argv[1])
"#,
        &[("COOL_PROGRAM_ARGS", "alpha\x1Fbeta")],
    )
    .unwrap();

    assert!(result.contains("<module sys>"));
    assert!(result.contains("alpha"));
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
import list
print(list)
nums = [3, 1, 2]
print(list.sort(nums))
print(list.reverse(nums))
print(list.flatten([[1, 2], [3], 4]))
print(list.unique([1, 1, 2, 2, 3]))
"#,
    )
    .unwrap();

    assert!(result.contains("<module list>"));
    assert!(result.contains("[1, 2, 3]") || result.contains("[1,2,3]"));
    assert!(result.contains("[2, 1, 3]") || result.contains("[2,1,3]"));
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
fn test_llvm_rejects_try_except() {
    let output = compile_native_expect_error(
        r#"
try:
    raise "boom"
except:
    print("caught")
"#,
    );

    assert!(output.contains("try/except is not yet supported in LLVM backend"));
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
