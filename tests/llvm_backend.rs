use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static LLVM_BUILD_LOCK: Mutex<()> = Mutex::new(());

fn unique_test_path(stem: &str, ext: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    PathBuf::from(format!("{stem}_{nonce}.{ext}"))
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

    let build_output = Command::new("./target/debug/cool")
        .args(["build", source_path.to_str().unwrap()])
        .output()
        .map_err(|e| e.to_string())?;

    if !build_output.status.success() {
        let stderr = String::from_utf8_lossy(&build_output.stderr).to_string();
        let stdout = String::from_utf8_lossy(&build_output.stdout).to_string();
        let _ = fs::remove_file(&source_path);
        let _ = fs::remove_file(&binary_path);
        return Err(format!("{stdout}{stderr}"));
    }

    let mut run_cmd = Command::new(&binary_path);
    for (k, v) in envs {
        run_cmd.env(k, v);
    }
    let run_output = run_cmd.output().map_err(|e| e.to_string())?;

    let _ = fs::remove_file(&source_path);
    let _ = fs::remove_file(&binary_path);

    if run_output.status.success() {
        Ok(String::from_utf8_lossy(&run_output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&run_output.stderr).to_string())
    }
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

    let _ = fs::remove_file(temp_dir.join("sample.txt"));
    let _ = fs::remove_dir(&temp_dir);

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
