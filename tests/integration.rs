// Integration tests for Cool language interpreter
// Run with: cargo test --test integration

use std::io::Write;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_temp_path(stem: &str, ext: &str) -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
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

fn run_cool_with_args_and_env(
    source: &str,
    extra_args: &[&str],
    envs: &[(&str, &str)],
) -> Result<String, String> {
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
    let result = run_cool_vm("import math\nprint(math.round(3.5))\nprint(math.round(3.14159, 2))\nprint(math.abs(-7))").unwrap();
    assert!(result.contains("4"));
    assert!(result.contains("3.14"));
    assert!(result.contains("7"));
}

#[test]
fn test_import_random_choice_tuple() {
    let result = run_cool("import random\nrandom.seed(1)\nprint(random.choice((\"x\", \"y\")) in (\"x\", \"y\"))").unwrap();
    assert!(result.contains("true"));
}

#[test]
fn test_vm_import_random_choice_tuple() {
    let result = run_cool_vm("import random\nrandom.seed(1)\nprint(random.choice((\"x\", \"y\")) in (\"x\", \"y\"))").unwrap();
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
fn test_vm_import_list_module() {
    let result = run_cool_vm(
        "import list\nnums = [3, 1, 2]\nprint(list.sort(nums))\nprint(list.unique([1, 1, 2, 2, 3]))",
    )
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
    assert_eq!(lines, ["enter outer", "enter inner", "exit inner", "after", "exit outer"]);
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
    let output = Command::new(cool_bin())
        .arg("coolc/compiler_vm.cool")
        .output()
        .unwrap();

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
    std::fs::write(&child_path, "import sys\nprint(sys.argv[0])\nprint(sys.argv[1])\nprint(sys.argv[2])\n").unwrap();
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

    let input = format!("source {}\ncat {} | grep beta\nexit\n", script_path.display(), pipe_path.display());
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
