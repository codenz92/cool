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

    let run_output = Command::new(&binary_path).output().map_err(|e| e.to_string())?;

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
