// Integration tests for Cool language interpreter
// Run with: cargo test --test integration

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);
const TEST_TARGET_I386: &str = "i386-unknown-linux-gnu";
const TEST_TARGET_X86_64: &str = "x86_64-unknown-linux-gnu";

fn unique_temp_path(stem: &str, ext: &str) -> std::path::PathBuf {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("{stem}_{nonce}_{seq}.{ext}"))
}

fn unique_temp_dir(stem: &str) -> std::path::PathBuf {
    unique_temp_path(stem, "dir")
}

fn cool_quote_path(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"")
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
    run_cool_path_with_args_and_env(path, extra_args, &[])
}

fn run_cool_path_with_args_and_env(
    path: &std::path::Path,
    extra_args: &[&str],
    envs: &[(&str, &str)],
) -> Result<String, String> {
    let mut cmd = Command::new(cool_bin());
    for arg in extra_args {
        cmd.arg(arg);
    }
    for (key, value) in envs {
        cmd.env(key, value);
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

fn host_pointer_bits() -> i64 {
    usize::BITS as i64
}

fn host_pointer_bytes() -> i64 {
    std::mem::size_of::<usize>() as i64
}

fn host_shared_lib_ext() -> &'static str {
    if cfg!(target_os = "windows") {
        "dll"
    } else if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    }
}

fn host_exe_ext_display() -> &'static str {
    if std::env::consts::EXE_EXTENSION.is_empty() {
        "<none>"
    } else {
        std::env::consts::EXE_EXTENSION
    }
}

fn expected_platform_lines(
    runtime: &str,
    has_ffi: bool,
    has_raw_memory: bool,
    has_extern: bool,
    has_inline_asm: bool,
) -> Vec<String> {
    vec![
        std::env::consts::OS.to_string(),
        std::env::consts::ARCH.to_string(),
        std::env::consts::FAMILY.to_string(),
        runtime.to_string(),
        host_exe_ext_display().to_string(),
        host_shared_lib_ext().to_string(),
        std::path::MAIN_SEPARATOR.to_string(),
        if cfg!(windows) { "2" } else { "1" }.to_string(),
        cfg!(windows).to_string(),
        cfg!(unix).to_string(),
        has_ffi.to_string(),
        has_raw_memory.to_string(),
        has_extern.to_string(),
        has_inline_asm.to_string(),
    ]
}

fn expected_core_lines() -> Vec<String> {
    vec![
        (std::mem::size_of::<usize>() * 8).to_string(),
        std::mem::size_of::<usize>().to_string(),
        "4096".to_string(),
        "73728".to_string(),
        "77824".to_string(),
        "837".to_string(),
        "0".to_string(),
        "1".to_string(),
        "3".to_string(),
        "18".to_string(),
        "18".to_string(),
        "0".to_string(),
        "0".to_string(),
        "0".to_string(),
    ]
}

fn expected_extended_core_lines() -> Vec<String> {
    vec![
        "4099".to_string(),
        "4104".to_string(),
        "4096".to_string(),
        "5".to_string(),
        "4096".to_string(),
        "4104".to_string(),
        "true".to_string(),
        "false".to_string(),
        "4".to_string(),
        "ababab".to_string(),
        "0xff".to_string(),
        "0b1010".to_string(),
        format!("0x{:0width$x}", 4096u64, width = std::mem::size_of::<usize>() * 2),
        "2".to_string(),
        "8".to_string(),
        "1".to_string(),
        "1".to_string(),
        "true".to_string(),
        "false".to_string(),
    ]
}

fn find_executable_for_test(candidates: &[&str], probe_args: &[&str]) -> Option<String> {
    for name in candidates {
        if Command::new(name)
            .args(probe_args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok()
        {
            return Some((*name).to_string());
        }
    }
    None
}

fn find_c_compiler_for_test() -> Option<String> {
    if let Some(configured) = std::env::var("CC").ok().filter(|value| !value.trim().is_empty()) {
        if Command::new(&configured)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok()
        {
            return Some(configured);
        }
    }
    find_executable_for_test(&["cc", "clang", "gcc"], &["--version"])
}

fn find_archiver_for_test() -> Option<String> {
    if let Some(configured) = std::env::var("AR").ok().filter(|value| !value.trim().is_empty()) {
        if Command::new(&configured)
            .arg("-V")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok()
        {
            return Some(configured);
        }
    }
    find_executable_for_test(&["ar", "llvm-ar"], &["-V"])
}

fn build_test_static_library(dir: &std::path::Path, name: &str, source: &str) -> Result<std::path::PathBuf, String> {
    let compiler = find_c_compiler_for_test().ok_or_else(|| "no C compiler found".to_string())?;
    let archiver = find_archiver_for_test().ok_or_else(|| "no archiver found".to_string())?;
    let c_path = dir.join(format!("{name}.c"));
    let object_path = dir.join(format!("{name}.o"));
    let archive_path = dir.join(format!("lib{name}.a"));

    std::fs::write(&c_path, source).map_err(|e| e.to_string())?;

    let compile_output = Command::new(&compiler)
        .args(["-c", c_path.to_str().unwrap(), "-o", object_path.to_str().unwrap()])
        .output()
        .map_err(|e| e.to_string())?;
    if !compile_output.status.success() {
        return Err(format!(
            "failed to compile test C library with '{}'\nstdout:\n{}\nstderr:\n{}",
            compiler,
            String::from_utf8_lossy(&compile_output.stdout),
            String::from_utf8_lossy(&compile_output.stderr)
        ));
    }

    let archive_output = Command::new(&archiver)
        .args(["rcs", archive_path.to_str().unwrap(), object_path.to_str().unwrap()])
        .output()
        .map_err(|e| e.to_string())?;
    if !archive_output.status.success() {
        return Err(format!(
            "failed to archive test C library with '{}'\nstdout:\n{}\nstderr:\n{}",
            archiver,
            String::from_utf8_lossy(&archive_output.stdout),
            String::from_utf8_lossy(&archive_output.stderr)
        ));
    }

    Ok(archive_path)
}

fn wrap_unsigned_host(n: i64) -> i64 {
    let mask = (1i128 << usize::BITS) - 1;
    ((n as i128) & mask) as i64
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
    run_cool_subcommand_in_dir_with_env(cwd, args, &[])
}

fn run_cool_subcommand_in_dir_with_env(
    cwd: &std::path::Path,
    args: &[&str],
    envs: &[(&str, &str)],
) -> Result<(String, String, i32), String> {
    let mut cmd = Command::new(cool_bin());
    cmd.current_dir(cwd).args(args);
    for (key, value) in envs {
        cmd.env(key, value);
    }
    let output = cmd.output().map_err(|e| e.to_string())?;
    Ok((
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    ))
}

fn write_phase6_data_suite(dir: &std::path::Path) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("main.cool"),
        r#"import base64
import bytes
import codec
import config
import html
import schema

blob = bytes.from_string("A🙂")
print(bytes.hex(blob))
print(bytes.to_string(blob))
print(bytes.read_u16_le(bytes.u16_le(513)))
print(bytes.read_u32_be(bytes.u32_be(16909060)))
print(base64.encode_text("Cool!"))
print(base64.decode_text("Q29vbCE="))
print(codec.encode("hex", [0, 255]))
decoded = codec.decode("hex", "00ff")
print(decoded[1])
print(codec.decode("utf-8", codec.encode("utf-8", "hi")))
print(html.escape("<tag &\"'>"))
print(html.extract_title("<html><title>Hi &amp; Bye</title></html>"))
print(html.extract_links("<a href='https://example.com'>x</a>")[0])
cfg = config.load("settings.env")
print(cfg["HELLO"])
print(cfg["SPACED"])
ini = config.load("settings.ini")
print(ini["db"]["port"])
print(config.expand_env("hi ${COOL_NAME}", {"COOL_NAME": "Ada"}))
rule = schema.shape({
    "name": schema.string({"min": 1}),
    "age": schema.optional(schema.integer({"min": 0})),
}, false)
print(schema.check(rule, {"name": "Ada"}))
bad = schema.validate(rule, {"name": "", "extra": true})
print(bad["ok"])
print(len(bad["errors"]) >= 2)
"#,
    )
    .unwrap();
    std::fs::write(dir.join("settings.env"), "HELLO=world\nSPACED=\"hello world\"\n").unwrap();
    std::fs::write(dir.join("settings.ini"), "[db]\nport = 5432\n").unwrap();
}

fn write_phase6_pass2_suite(dir: &std::path::Path) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("main.cool"),
        r#"import json
import locale
import unicode
import xml

docs = json.loads_lines("{\"name\":\"Ada\"}\n{\"name\":\"Lin\",\"n\":2}\n")
roundtrip = json.loads_lines(json.dumps_lines(docs))
print(roundtrip[0]["name"])
print(roundtrip[1]["n"])
data = json.loads("{\"user\":{\"id\":\"7\",\"name\":\"Ada\"},\"items\":[{\"name\":\"a\",\"count\":\"2\"},{\"name\":\"b\",\"count\":1}]}")
print(json.pointer(data, "/user/name"))
print(json.pointer(data, "/missing", "fallback"))
projected = json.transform(data, {
    "id": {"$from": "/user/id", "$coerce": "int"},
    "items": {"$from": "/items", "$each": {"name": "/name", "count": {"$from": "/count", "$coerce": "int"}}},
    "missing": {"$from": "/missing", "$default": "fallback"},
})
print(projected["id"])
print(projected["items"][0]["count"] + projected["items"][1]["count"])
print(projected["missing"])
root = xml.loads("<note priority='high'><title>Hello</title><body>Hi <![CDATA[<raw>]]></body></note>")
print(root["name"])
print(xml.find(root, "title")["children"][0]["text"])
print(xml.text_content(root))
print(xml.dumps(root).find("priority=\"high\"") >= 0)
print(unicode.category("é"))
print(unicode.normalize("e" + chr(769), "nfc"))
print(unicode.normalize("ﬁ", "nfkc"))
print(unicode.grapheme_len("👩‍💻a"))
print(unicode.width("A🙂👩‍💻"))
print(unicode.codepoints("A🙂")[1])
print(unicode.width("🇬🇧"))
info = locale.parse("fr_fr")
print(locale.normalize("fr_fr"))
print(info["language"])
print(locale.language_name("ja"))
print(locale.region_name("GB"))
print(locale.number(12345.5, "fr-FR"))
print(locale.parse_number("12 345,5", "fr-FR"))
print(locale.currency(19.5, "EUR", "fr-FR"))
print(locale.match("en-AU", ["fr-FR", "en-GB", "de-DE"]))
"#,
    )
    .unwrap();
}

fn write_phase6_pass3_suite(dir: &std::path::Path) {
    std::fs::create_dir_all(dir).unwrap();
    let base = cool_quote_path(dir);
    std::fs::write(
        dir.join("main.cool"),
        format!(
            r#"import daemon
import os
import path
import sandbox
import store
import sync

base = "{base}"

db = store.open_store(path.join(base, "nested", "state", "db"))
prefs = db.namespace("prefs")
prefs.set("theme", "amber")
prefs.increment("count", 2)
tx = prefs.transaction()
tx.set("draft", "temp")
tx.rollback()
print(prefs.get("draft", "missing"))
tx = prefs.transaction()
tx.set("draft", "kept")
tx.commit()
print(prefs.get("draft"))
print(db.namespaces()[0])
print(prefs.size())

svc = daemon.service("demo", {{"root": path.join(base, "services", "runtime"), "command": "printf service-ready"}})
pid = svc.start()
print(pid > 0)
print(svc.wait(1.0))
print(svc.status()["exit_code"] == 0)
print(svc.tail() == "service-ready")
retry = daemon.service("retry", {{
    "root": path.join(base, "services", "runtime"),
    "command": "printf retry && exit 1",
    "restart": "on-failure",
    "max_restarts": 1,
    "restart_delay": 0.01,
}})
retry.start()
retry.wait(1.0)
print(retry.should_restart())
print(retry.maintain())
retry.wait(1.0)
print(retry.status()["restart_count"] == 1)
retry.cleanup(true)
svc.cleanup(true)
print(not path.exists(svc.stdout_path))

box = sandbox.open_sandbox({{
    "root": path.join(base, "workspace"),
    "process": true,
    "commands": ["printf"],
    "env": ["COOL_PHASE6_SB"],
}})
box.write_text("notes/todo.txt", "safe")
print(box.read_text("notes/todo.txt"))
print(box.check_output("printf sand") == "sand")
print(box.getenv("COOL_PHASE6_SB") == "allowed")
try:
    box.write_text(path.join(path.dirname(base), "outside.txt"), "bad")
    print(false)
except as err:
    print(str(err).find("write denied") >= 0)
try:
    box.run("uname")
    print(false)
except as err:
    print(str(err).find("command denied") >= 0)
try:
    box.getenv("HOME")
    print(false)
except as err:
    print(str(err).find("env denied") >= 0)

src = path.join(base, "sync-src")
dst = path.join(base, "sync-dst")
os.mkdir(src)
os.mkdir(path.join(src, "dir"))
f = open(path.join(src, "dir", "item.txt"), "w")
f.write("shared-one")
f.close()
first = sync.sync_dirs(src, dst, path.join(base, "sync-state", "state.json"))
print(len(first["conflicts"]) == 0)
f = open(path.join(src, "dir", "item.txt"), "w")
f.write("source-two")
f.close()
f = open(path.join(src, "new.txt"), "w")
f.write("new")
f.close()
f = open(path.join(dst, "dir", "item.txt"), "w")
f.write("dest-two")
f.close()
f = open(path.join(dst, "extra.txt"), "w")
f.write("extra")
f.close()
plan = sync.reconcile(src, dst, path.join(base, "sync-state", "state.json"))
print(len(plan["conflicts"]) == 1)
print(plan["conflicts"][0]["path"] == "dir/item.txt")
applied = sync.sync_dirs(src, dst, path.join(base, "sync-state", "state.json"), {{"conflicts": "source"}})
after = sync.snapshot(dst)
print(sync.find(after, "new.txt") != nil)
print(sync.find(after, "extra.txt") == nil)
f = open(path.join(dst, "dir", "item.txt"), "r")
print(f.read())
f.close()
saved = sync.load_snapshot(path.join(base, "sync-state", "state.json"))
print(sync.find(saved, "new.txt") != nil)
print(len(applied["applied"]) == 3)
"#
        ),
    )
    .unwrap();
}

fn phase6_filesystem_os_source(base: &std::path::Path) -> String {
    let base = cool_quote_path(base);
    format!(
        r#"import fswatch
import glob
import os
import path
import process
import tempfile

base = "{base}"
os.mkdir(path.join(base, "src"))
os.mkdir(path.join(base, "src", "nested"))
os.mkdir(path.join(base, "docs"))
f = open(path.join(base, "src", "main.cool"), "w")
f.write("main\n")
f.close()
f = open(path.join(base, "src", "nested", "helper.cool"), "w")
f.write("helper\n")
f.close()
f = open(path.join(base, "docs", "readme.txt"), "w")
f.write("docs\n")
f.close()
f = open(path.join(base, ".hidden.cool"), "w")
f.write("hidden\n")
f.close()

print(glob.matches("src/**/*.cool", "src/nested/helper.cool"))
matches = glob.glob("**/*.cool", base)
print(len(matches))
print(path.basename(matches[0]))
print(path.basename(matches[1]))
hidden = glob.glob("*.cool", base, false, true)
print(len(hidden))
walked = glob.walk(path.join(base, "src"), true, false)
print(len(walked))
tmp_file = tempfile.named_file("note-", ".txt", base)
print(path.basename(tmp_file.path).startswith("note-"))
f = tmp_file.open_file("w")
f.write("note")
f.close()
print(tmp_file.exists())
close(tmp_file)
print(path.exists(tmp_file.path))
tmp_dir = tempfile.named_dir("work-", "", base)
f = open(tmp_dir.join("child.txt"), "w")
f.write("child")
f.close()
tmp_dir.keep()
close(tmp_dir)
print(path.exists(tmp_dir.path))
tempfile.cleanup(tmp_dir)
print(path.exists(tmp_dir.path))
plain_dir = tempfile.mkdtemp("dir-", "", base)
print(path.basename(plain_dir).startswith("dir-"))
tempfile.cleanup(plain_dir)
plain_file = tempfile.mkstemp("plain-", ".log", base)
print(path.basename(plain_file).startswith("plain-"))
tempfile.cleanup(plain_file)
print(process.pid() > 0)
parent = process.ppid()
print(parent == nil or parent >= 0)
print(process.getenv("COOL_PHASE6_TOKEN") == "present")
env_map = process.environ()
print(env_map["COOL_PHASE6_TOKEN"])
print(process.is_alive(process.pid()))
print(process.signal_number("term"))
print(process.signal_name(15))
print(process.info()["runtime"])
snap_before = fswatch.snapshot(base, {{"hidden": true, "include_dirs": false}})
tempfile.cleanup(path.join(base, "docs", "readme.txt"))
f = open(path.join(base, "src", "nested", "helper.cool"), "w")
f.write("helper updated and longer\n")
f.close()
f = open(path.join(base, "new.cool"), "w")
f.write("new\n")
f.close()
snap_after = fswatch.snapshot(base, {{"hidden": true, "include_dirs": false}})
events = fswatch.diff(snap_before, snap_after)
print(len(events))
created = false
deleted = false
modified = false
for event in events:
    kind = event["kind"]
    if kind == "created" and path.basename(event["path"]) == "new.cool":
        created = true
    if kind == "deleted" and path.basename(event["path"]) == "readme.txt":
        deleted = true
    if kind == "modified" and path.basename(event["path"]) == "helper.cool":
        modified = true
print(created)
print(deleted)
print(modified)
print(len(fswatch.watch(base, 0.01, 0.03, {{"hidden": true, "include_dirs": false}})) == 0)
"#
    )
}

fn expected_phase6_filesystem_os_lines(runtime: &str) -> Vec<String> {
    vec![
        "true".to_string(),
        "2".to_string(),
        "main.cool".to_string(),
        "helper.cool".to_string(),
        "1".to_string(),
        "4".to_string(),
        "true".to_string(),
        "true".to_string(),
        "false".to_string(),
        "true".to_string(),
        "false".to_string(),
        "true".to_string(),
        "true".to_string(),
        "true".to_string(),
        "true".to_string(),
        "true".to_string(),
        "present".to_string(),
        "true".to_string(),
        "15".to_string(),
        "TERM".to_string(),
        runtime.to_string(),
        "3".to_string(),
        "true".to_string(),
        "true".to_string(),
        "true".to_string(),
        "true".to_string(),
    ]
}

fn copy_dir(src: &std::path::Path, dst: &std::path::Path) {
    let status = Command::new("cp")
        .args(["-R", src.to_str().unwrap(), dst.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(
        status.success(),
        "failed to copy {} -> {}",
        src.display(),
        dst.display()
    );
}

fn try_http_get_text(addr: &str, path: &str) -> Result<String, std::io::Error> {
    let mut stream = TcpStream::connect(addr)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream
        .write_all(format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").as_bytes())
        .map_err(std::io::Error::other)?;
    stream.shutdown(std::net::Shutdown::Write).ok();
    let mut response = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => response.extend_from_slice(&chunk[..n]),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock || err.kind() == std::io::ErrorKind::TimedOut => {
                break
            }
            Err(err) => return Err(err),
        }
    }
    Ok(String::from_utf8_lossy(&response).to_string())
}

fn response_contains_coolboard_health(response: &str) -> bool {
    response.contains("200 OK")
        && (response.contains("\"status\": \"ok\"") || response.contains("\"status\":\"ok\""))
        && (response.contains("\"service\": \"coolboard\"") || response.contains("\"service\":\"coolboard\""))
}

fn object_has_section(path: &std::path::Path, section: &str) -> Result<bool, String> {
    if cfg!(target_os = "macos") {
        let (segment, section_name) = section
            .split_once(',')
            .ok_or_else(|| format!("invalid Mach-O section specifier '{section}'"))?;
        let output = Command::new("otool")
            .args(["-l", path.to_str().unwrap()])
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).to_string());
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.contains(&format!("segname {segment}")) && stdout.contains(&format!("sectname {section_name}")))
    } else {
        let output = Command::new("objdump")
            .args(["-h", path.to_str().unwrap()])
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).to_string());
        }
        Ok(String::from_utf8_lossy(&output.stdout).contains(section))
    }
}

fn object_has_symbol(path: &std::path::Path, symbol: &str) -> Result<bool, String> {
    let output = Command::new("nm")
        .args(["-g", path.to_str().unwrap()])
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).contains(symbol))
}

fn archive_entries(path: &std::path::Path) -> Result<Vec<String>, String> {
    let output = Command::new("ar")
        .args(["t", path.to_str().unwrap()])
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect())
}

fn host_target_triple() -> String {
    let os = if cfg!(target_os = "macos") {
        "darwin"
    } else {
        std::env::consts::OS
    };
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        std::env::consts::ARCH
    };
    format!("{os}-{arch}")
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
fn test_pointer_width_helpers() {
    let result =
        run_cool("print(isize(-1))\nprint(usize(4294967296))\nprint(word_bits())\nprint(word_bytes())").unwrap();
    let lines: Vec<_> = result
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    let expected = vec![
        "-1".to_string(),
        wrap_unsigned_host(4_294_967_296).to_string(),
        host_pointer_bits().to_string(),
        host_pointer_bytes().to_string(),
    ];
    assert_eq!(lines, expected);
}

#[test]
fn test_vm_pointer_width_helpers() {
    let result =
        run_cool_vm("print(isize(-1))\nprint(usize(4294967296))\nprint(word_bits())\nprint(word_bytes())").unwrap();
    let lines: Vec<_> = result
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    let expected = vec![
        "-1".to_string(),
        wrap_unsigned_host(4_294_967_296).to_string(),
        host_pointer_bits().to_string(),
        host_pointer_bytes().to_string(),
    ];
    assert_eq!(lines, expected);
}

#[test]
fn test_interpreter_extern_declaration_requires_llvm() {
    let err = run_cool("extern def abs(x: i32) -> i32\nprint(abs(-1))").unwrap_err();
    assert!(err.contains("only supported in the LLVM backend"));
    assert!(err.contains("compile with `cool build`"));
}

#[test]
fn test_vm_extern_declaration_requires_llvm() {
    let err = run_cool_vm("extern def abs(x: i32) -> i32\nprint(abs(-1))").unwrap_err();
    assert!(err.contains("only supported in the LLVM backend"));
    assert!(err.contains("compile with `cool build`"));
}

#[test]
fn test_interpreter_data_declaration_requires_llvm() {
    let err = run_cool("data BOOT_MAGIC: u32 = 1\nprint(BOOT_MAGIC)").unwrap_err();
    assert!(err.contains("only supported in the LLVM backend"));
    assert!(err.contains("compile with `cool build`"));
}

#[test]
fn test_vm_data_declaration_requires_llvm() {
    let err = run_cool_vm("data BOOT_MAGIC: u32 = 1\nprint(BOOT_MAGIC)").unwrap_err();
    assert!(err.contains("only supported in the LLVM backend"));
    assert!(err.contains("compile with `cool build`"));
}

#[test]
fn test_interpreter_volatile_memory_builtin_requires_llvm() {
    let err = run_cool("ptr = 0\nprint(read_i32_volatile(ptr))").unwrap_err();
    assert!(err.contains("only supported in the LLVM backend"));
    assert!(err.contains("compile with `cool build`"));
}

#[test]
fn test_vm_volatile_memory_builtin_requires_llvm() {
    let err = run_cool_vm("ptr = 0\nprint(read_i32_volatile(ptr))").unwrap_err();
    assert!(err.contains("only supported in the LLVM backend"));
    assert!(err.contains("compile with `cool build`"));
}

#[test]
fn test_interpreter_port_io_builtins_require_llvm() {
    for src in &["outb(0x3F8, 65)", "inb(0x3F8)", "write_serial_byte(65)"] {
        let err = run_cool(src).unwrap_err();
        assert!(
            err.contains("only supported in the LLVM backend"),
            "{src}: expected LLVM-only error, got: {err}"
        );
    }
}

#[test]
fn test_vm_port_io_builtins_require_llvm() {
    for src in &["outb(0x3F8, 65)", "inb(0x3F8)", "write_serial_byte(65)"] {
        let err = run_cool_vm(src).unwrap_err();
        assert!(
            err.contains("only supported in the LLVM backend"),
            "{src}: expected LLVM-only error, got: {err}"
        );
    }
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
fn test_struct_pointer_width_aliases() {
    let result = run_cool(
        r#"struct PtrSized:
    addr: usize
    delta: isize

p = PtrSized(123, -7)
print(p.addr)
print(p.delta)
"#,
    )
    .unwrap();
    assert_eq!(result.trim(), "123\n-7");
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
fn test_vm_struct_pointer_width_aliases() {
    let result = run_cool_vm(
        r#"struct PtrSized:
    addr: usize
    delta: isize

p = PtrSized(123, -7)
print(p.addr)
print(p.delta)
"#,
    )
    .unwrap();
    assert_eq!(result.trim(), "123\n-7");
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
fn test_union_basic() {
    let result = run_cool(
        r#"union Number:
    a: i32
    b: i32

v = Number()
v.a = 42
print(v.a)
w = Number(a=100)
print(w.a)
"#,
    )
    .unwrap();
    assert_eq!(result.trim(), "42\n100");
}

#[test]
fn test_union_zero_init() {
    let result = run_cool(
        r#"union Num:
    x: i32
    y: f64

v = Num()
print(v.x)
"#,
    )
    .unwrap();
    assert_eq!(result.trim(), "0");
}

#[test]
fn test_vm_union_basic() {
    let result = run_cool_vm(
        r#"union Number:
    a: i32
    b: i32

v = Number()
v.a = 42
print(v.a)
w = Number(a=100)
print(w.a)
"#,
    )
    .unwrap();
    assert_eq!(result.trim(), "42\n100");
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
fn test_cool_build_profile_dev_runs_checks_before_compile() {
    let project_dir = unique_temp_dir("cool_build_profile_dev");
    let _ = std::fs::remove_dir_all(&project_dir);
    std::fs::create_dir_all(project_dir.join("src")).unwrap();
    std::fs::write(
        project_dir.join("cool.toml"),
        r#"[project]
name = "demo"
version = "0.2.0"
main = "src/main.cool"
output = "demo-bin"

[build]
profile = "dev"
"#,
    )
    .unwrap();
    std::fs::write(project_dir.join("src").join("main.cool"), "print(\"dev profile\")\n").unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&project_dir, &["build"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());
    assert!(stdout.contains("Checking demo v0.2.0 (src/main.cool) [dev]"));
    assert!(stdout.contains("Checked 1 module(s)"));
    assert!(stdout.contains("Compiling demo v0.2.0 (src/main.cool) [dev]"));
    assert!(project_dir.join("demo-bin").exists());

    let _ = std::fs::remove_dir_all(&project_dir);
}

#[test]
fn test_cool_build_profile_flag_strict_rejects_unannotated_top_level_defs() {
    let temp = unique_temp_path("cool_build_profile_strict", "cool");
    std::fs::write(&temp, "def greet(name):\n    return name\n\nprint(greet(\"hi\"))\n").unwrap();

    let cwd = temp.parent().unwrap();
    let file_name = temp.file_name().unwrap().to_str().unwrap();
    let (stdout, stderr, code) = run_cool_subcommand_in_dir(cwd, &["build", "--profile", "strict", file_name]).unwrap();

    let _ = std::fs::remove_file(&temp);
    let _ = std::fs::remove_file(temp.with_extension(""));

    assert_ne!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stdout.contains("Checking "));
    assert!(stderr.contains("unannotated_param"));
    assert!(stderr.contains("unannotated_return"));
    assert!(stderr.contains("strict profile check failed"));
}

#[test]
fn test_cool_build_profile_manifest_freestanding_emits_object_file() {
    let project_dir = unique_temp_dir("cool_build_profile_freestanding");
    let _ = std::fs::remove_dir_all(&project_dir);
    std::fs::create_dir_all(project_dir.join("src")).unwrap();
    std::fs::write(
        project_dir.join("cool.toml"),
        r#"[project]
name = "demo"
version = "0.2.0"
main = "src/main.cool"
output = "demo-bin"

[build]
profile = "freestanding"
"#,
    )
    .unwrap();
    std::fs::write(
        project_dir.join("src").join("main.cool"),
        "def boot_entry():\n    return 7\n",
    )
    .unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&project_dir, &["build"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());
    assert!(stdout.contains("Compiling demo v0.2.0 (src/main.cool) [freestanding, object]"));
    assert!(project_dir.join("demo-bin.o").exists());
    assert!(!project_dir.join("demo-bin").exists());

    let _ = std::fs::remove_dir_all(&project_dir);
}

#[test]
fn test_cool_build_profile_flag_overrides_manifest_profile() {
    let project_dir = unique_temp_dir("cool_build_profile_override");
    let _ = std::fs::remove_dir_all(&project_dir);
    std::fs::create_dir_all(project_dir.join("src")).unwrap();
    std::fs::write(
        project_dir.join("cool.toml"),
        r#"[project]
name = "demo"
version = "0.2.0"
main = "src/main.cool"
output = "demo-bin"

[build]
profile = "freestanding"
"#,
    )
    .unwrap();
    std::fs::write(project_dir.join("src").join("main.cool"), "print(\"override\")\n").unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&project_dir, &["build", "--profile", "release"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());
    assert!(stdout.contains("Compiling demo v0.2.0 (src/main.cool)"));
    assert!(!stdout.contains("freestanding"));
    assert!(project_dir.join("demo-bin").exists());
    assert!(!project_dir.join("demo-bin.o").exists());

    let _ = std::fs::remove_dir_all(&project_dir);
}

#[test]
fn test_cool_build_emit_object_for_single_file() {
    let temp = unique_temp_path("cool_build_emit_object", "cool");
    std::fs::write(&temp, "print(\"object output\")\n").unwrap();

    let cwd = temp.parent().unwrap();
    let file_name = temp.file_name().unwrap().to_str().unwrap();
    let object_path = temp.with_extension("o");

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(cwd, &["build", "--emit", "object", file_name]).unwrap();

    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());
    assert!(stdout.contains("[object]"));
    assert!(
        object_path.exists(),
        "expected object file at {}",
        object_path.display()
    );
    assert!(object_has_symbol(&object_path, "main").unwrap());
    assert!(!temp.with_extension("").exists());

    let _ = std::fs::remove_file(&temp);
    let _ = std::fs::remove_file(&object_path);
}

#[test]
fn test_cool_build_emit_manifest_assembly_writes_project_output() {
    let project_dir = unique_temp_dir("cool_build_emit_assembly");
    let _ = std::fs::remove_dir_all(&project_dir);
    std::fs::create_dir_all(project_dir.join("src")).unwrap();
    std::fs::write(
        project_dir.join("cool.toml"),
        r#"[project]
name = "demo"
version = "0.2.0"
main = "src/main.cool"
output = "demo-bin"

[build]
emit = "assembly"
"#,
    )
    .unwrap();
    std::fs::write(
        project_dir.join("src").join("main.cool"),
        "def helper_value() -> i32:\n    return 7\n\nprint(helper_value())\n",
    )
    .unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&project_dir, &["build"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());
    assert!(stdout.contains("[assembly]"));

    let assembly_path = project_dir.join("demo-bin.s");
    assert!(
        assembly_path.exists(),
        "expected assembly file at {}",
        assembly_path.display()
    );
    let assembly = std::fs::read_to_string(&assembly_path).unwrap();
    assert!(
        assembly.contains("helper_value"),
        "expected helper_value in assembly:\n{assembly}"
    );
    assert!(!project_dir.join("demo-bin").exists());

    let _ = std::fs::remove_dir_all(&project_dir);
}

#[test]
fn test_cool_build_emit_flag_overrides_manifest_emit() {
    let project_dir = unique_temp_dir("cool_build_emit_override");
    let _ = std::fs::remove_dir_all(&project_dir);
    std::fs::create_dir_all(project_dir.join("src")).unwrap();
    std::fs::write(
        project_dir.join("cool.toml"),
        r#"[project]
name = "demo"
version = "0.2.0"
main = "src/main.cool"
output = "demo-bin"

[build]
emit = "assembly"
"#,
    )
    .unwrap();
    std::fs::write(project_dir.join("src").join("main.cool"), "print(\"ir output\")\n").unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&project_dir, &["build", "--emit", "llvm-ir"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());
    assert!(stdout.contains("[llvm-ir]"));

    let ir_path = project_dir.join("demo-bin.ll");
    assert!(ir_path.exists(), "expected LLVM IR file at {}", ir_path.display());
    let ir = std::fs::read_to_string(&ir_path).unwrap();
    assert!(ir.contains("define i32 @main()"), "expected main in LLVM IR:\n{ir}");
    assert!(!project_dir.join("demo-bin.s").exists());

    let _ = std::fs::remove_dir_all(&project_dir);
}

#[test]
fn test_cool_build_emit_static_library_for_project() {
    let project_dir = unique_temp_dir("cool_build_emit_staticlib");
    let _ = std::fs::remove_dir_all(&project_dir);
    std::fs::create_dir_all(project_dir.join("src")).unwrap();
    std::fs::write(
        project_dir.join("cool.toml"),
        r#"[project]
name = "demo"
version = "0.2.0"
main = "src/main.cool"
output = "demo-bin"

[build]
emit = "staticlib"
"#,
    )
    .unwrap();
    std::fs::write(
        project_dir.join("src").join("main.cool"),
        "def library_value() -> i32:\n    return 42\n",
    )
    .unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&project_dir, &["build"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());
    assert!(stdout.contains("[staticlib]"));

    let archive_path = project_dir.join("libdemo-bin.a");
    assert!(
        archive_path.exists(),
        "expected static library at {}",
        archive_path.display()
    );
    let entries = archive_entries(&archive_path).unwrap();
    assert!(
        entries.iter().any(|entry| entry == "demo-bin.o"),
        "expected demo-bin.o in archive: {entries:?}"
    );
    assert!(
        entries.iter().any(|entry| entry == "cool_runtime.o"),
        "expected cool_runtime.o in archive: {entries:?}"
    );
    assert!(object_has_symbol(&archive_path, "library_value").unwrap());

    let _ = std::fs::remove_dir_all(&project_dir);
}

#[test]
fn test_cool_build_emit_shared_library_for_project() {
    let project_dir = unique_temp_dir("cool_build_emit_sharedlib");
    let _ = std::fs::remove_dir_all(&project_dir);
    std::fs::create_dir_all(project_dir.join("src")).unwrap();
    std::fs::write(
        project_dir.join("cool.toml"),
        r#"[project]
name = "demo"
version = "0.2.0"
main = "src/main.cool"
output = "demo-bin"

[build]
emit = "sharedlib"
"#,
    )
    .unwrap();
    std::fs::write(
        project_dir.join("src").join("main.cool"),
        "def library_value() -> i32:\n    return 42\n",
    )
    .unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&project_dir, &["build"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());
    assert!(stdout.contains("[sharedlib]"));

    let shared_path = project_dir.join(format!("libdemo-bin.{}", host_shared_lib_ext()));
    assert!(
        shared_path.exists(),
        "expected shared library at {}",
        shared_path.display()
    );
    assert!(object_has_symbol(&shared_path, "library_value").unwrap());

    let _ = std::fs::remove_dir_all(&project_dir);
}

#[test]
fn test_cool_build_emit_binary_rejects_freestanding_mode() {
    let temp = unique_temp_path("cool_build_emit_binary_freestanding", "cool");
    std::fs::write(&temp, "def boot_entry():\n    return 1\n").unwrap();

    let cwd = temp.parent().unwrap();
    let file_name = temp.file_name().unwrap().to_str().unwrap();
    let (stdout, stderr, code) =
        run_cool_subcommand_in_dir(cwd, &["build", "--freestanding", "--emit", "binary", file_name]).unwrap();

    let _ = std::fs::remove_file(&temp);
    let _ = std::fs::remove_file(temp.with_extension(""));

    assert_ne!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.contains("freestanding builds cannot emit hosted binaries"));
}

#[test]
fn test_cool_build_target_flag_sets_llvm_ir_triple_and_pointer_width() {
    let temp = unique_temp_path("cool_build_target_i386", "cool");
    std::fs::write(
        &temp,
        "data PTR_BITS: usize = 32\n\ndef _start():\n    entry: \"_start\"\n    return 0\n",
    )
    .unwrap();

    let cwd = temp.parent().unwrap();
    let file_name = temp.file_name().unwrap().to_str().unwrap();
    let (stdout, stderr, code) = run_cool_subcommand_in_dir(
        cwd,
        &[
            "build",
            "--freestanding",
            "--emit",
            "llvm-ir",
            "--target",
            TEST_TARGET_I386,
            file_name,
        ],
    )
    .unwrap();

    let ir_path = temp.with_extension("ll");
    let ir = std::fs::read_to_string(&ir_path).unwrap();

    let _ = std::fs::remove_file(&temp);
    let _ = std::fs::remove_file(&ir_path);

    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());
    assert!(stdout.contains(&format!("[freestanding, llvm-ir, target={}]", TEST_TARGET_I386)));
    assert!(ir.contains(&format!("target triple = \"{}\"", TEST_TARGET_I386)));
    assert!(ir.contains("@PTR_BITS"), "expected PTR_BITS global in LLVM IR:\n{ir}");
    assert!(
        ir.contains("@PTR_BITS = global i32 32") || ir.contains("@PTR_BITS = dso_local global i32 32"),
        "expected usize global to lower to i32 for 32-bit target:\n{ir}"
    );
}

#[test]
fn test_cool_build_target_flag_overrides_manifest_target() {
    let project_dir = unique_temp_dir("cool_build_target_override");
    let _ = std::fs::remove_dir_all(&project_dir);
    std::fs::create_dir_all(project_dir.join("src")).unwrap();
    std::fs::write(
        project_dir.join("cool.toml"),
        format!(
            r#"[project]
name = "demo"
version = "0.2.0"
main = "src/main.cool"
output = "demo-bin"

[build]
profile = "freestanding"
emit = "llvm-ir"
target = "{manifest_target}"
"#,
            manifest_target = TEST_TARGET_I386,
        ),
    )
    .unwrap();
    std::fs::write(
        project_dir.join("src").join("main.cool"),
        "data PTR_BITS: usize = 64\n\ndef _start():\n    entry: \"_start\"\n    return 0\n",
    )
    .unwrap();

    let (stdout, stderr, code) =
        run_cool_subcommand_in_dir(&project_dir, &["build", "--target", TEST_TARGET_X86_64]).unwrap();
    let ir_path = project_dir.join("demo-bin.ll");
    let ir = std::fs::read_to_string(&ir_path).unwrap();

    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());
    assert!(stdout.contains(&format!("[freestanding, llvm-ir, target={}]", TEST_TARGET_X86_64)));
    assert!(ir.contains(&format!("target triple = \"{}\"", TEST_TARGET_X86_64)));
    assert!(ir.contains("@PTR_BITS"), "expected PTR_BITS global in LLVM IR:\n{ir}");
    assert!(
        ir.contains("@PTR_BITS = global i64 64") || ir.contains("@PTR_BITS = dso_local global i64 64"),
        "expected usize global to lower to i64 for 64-bit target:\n{ir}"
    );

    let _ = std::fs::remove_dir_all(&project_dir);
}

#[test]
fn test_cool_build_cpu_and_feature_flags_appear_in_llvm_ir() {
    let temp = unique_temp_path("cool_build_cpu_features", "cool");
    std::fs::write(&temp, "def helper() -> i32:\n    return 7\n\nprint(helper())\n").unwrap();

    let cwd = temp.parent().unwrap();
    let file_name = temp.file_name().unwrap().to_str().unwrap();
    let (stdout, stderr, code) = run_cool_subcommand_in_dir(
        cwd,
        &[
            "build",
            "--emit",
            "llvm-ir",
            "--target",
            TEST_TARGET_X86_64,
            "--cpu",
            "x86-64",
            "--cpu-features",
            "+sse4.2,+popcnt",
            file_name,
        ],
    )
    .unwrap();

    let ir_path = temp.with_extension("ll");
    let ir = std::fs::read_to_string(&ir_path).unwrap();

    let _ = std::fs::remove_file(&temp);
    let _ = std::fs::remove_file(&ir_path);

    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());
    assert!(stdout.contains("cpu=x86-64"), "stdout:\n{stdout}");
    assert!(stdout.contains("features=+sse4.2,+popcnt"), "stdout:\n{stdout}");
    assert!(ir.contains(&format!("target triple = \"{}\"", TEST_TARGET_X86_64)));
}

#[test]
fn test_cool_build_surfaces_invalid_cpu_name_warning() {
    let temp = unique_temp_path("cool_build_bad_cpu", "cool");
    std::fs::write(&temp, "print(1)\n").unwrap();

    let cwd = temp.parent().unwrap();
    let file_name = temp.file_name().unwrap().to_str().unwrap();
    let (stdout, stderr, code) = run_cool_subcommand_in_dir(
        cwd,
        &[
            "build",
            "--emit",
            "llvm-ir",
            "--target",
            TEST_TARGET_X86_64,
            "--cpu",
            "definitely-not-a-real-cpu",
            file_name,
        ],
    )
    .unwrap();

    let _ = std::fs::remove_file(&temp);
    let _ = std::fs::remove_file(temp.with_extension("ll"));

    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.contains("not a recognized processor"), "stderr:\n{stderr}");
}

#[test]
fn test_cool_build_no_libc_linux_syscall_lowers_to_inline_asm_ir() {
    let temp = unique_temp_path("cool_build_no_libc_syscall", "cool");
    std::fs::write(
        &temp,
        r#"import core
def _start():
    entry: "_start"
    return core.syscall1(39, 0)
"#,
    )
    .unwrap();

    let cwd = temp.parent().unwrap();
    let file_name = temp.file_name().unwrap().to_str().unwrap();
    let (stdout, stderr, code) = run_cool_subcommand_in_dir(
        cwd,
        &[
            "build",
            "--emit",
            "llvm-ir",
            "--target",
            TEST_TARGET_X86_64,
            "--no-libc",
            file_name,
        ],
    )
    .unwrap();

    let ir_path = temp.with_extension("ll");
    let ir = std::fs::read_to_string(&ir_path).unwrap();

    let _ = std::fs::remove_file(&temp);
    let _ = std::fs::remove_file(&ir_path);

    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());
    assert!(stdout.contains("no-libc"), "stdout:\n{stdout}");
    assert!(
        ir.contains("asm sideeffect \"syscall\""),
        "expected syscall inline asm in LLVM IR:\n{ir}"
    );
}

#[test]
fn test_cool_build_links_native_static_library_from_extern_metadata() {
    let Some(compiler) = find_c_compiler_for_test() else {
        eprintln!("skipping: no C compiler found");
        return;
    };
    let Some(archiver) = find_archiver_for_test() else {
        eprintln!("skipping: no archiver found");
        return;
    };

    let project_dir = unique_temp_dir("cool_build_native_static_link");
    let _ = std::fs::remove_dir_all(&project_dir);
    std::fs::create_dir_all(project_dir.join("src")).unwrap();
    std::fs::create_dir_all(project_dir.join("native")).unwrap();
    build_test_static_library(
        &project_dir.join("native"),
        "nativecalc",
        "int native_sum(int left, int right) { return left + right + 5; }\n",
    )
    .unwrap();

    std::fs::write(
        project_dir.join("cool.toml"),
        format!(
            r#"[project]
name = "demo"
version = "0.2.0"
main = "src/main.cool"

[toolchain]
cc = "{compiler}"
ar = "{archiver}"

[native]
search = ["native"]
"#,
        ),
    )
    .unwrap();
    std::fs::write(
        project_dir.join("src").join("main.cool"),
        r#"extern def native_sum(left: i32, right: i32) -> i32:
    symbol: "native_sum"
    cc: "c"
    library: "nativecalc"
    link_kind: "static"
    ownership: "borrowed"
    lifetime: "caller"

print(native_sum(20, 17))
"#,
    )
    .unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&project_dir, &["build"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());

    let binary_name = if std::env::consts::EXE_EXTENSION.is_empty() {
        "demo".to_string()
    } else {
        format!("demo.{}", std::env::consts::EXE_EXTENSION)
    };
    let output = Command::new(project_dir.join(binary_name))
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "binary failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "42");

    let _ = std::fs::remove_dir_all(&project_dir);
}

#[test]
fn test_cool_bundle_packages_project_via_cool_app() {
    let project_dir = unique_temp_dir("cool_project_bundle");
    let _ = std::fs::remove_dir_all(&project_dir);
    std::fs::create_dir_all(project_dir.join("src")).unwrap();
    std::fs::create_dir_all(project_dir.join("assets")).unwrap();
    std::fs::write(
        project_dir.join("cool.toml"),
        r#"[project]
name = "demo"
version = "0.2.0"
main = "src/main.cool"
output = "demo-bin"

[bundle]
include = ["assets", "README.txt"]
"#,
    )
    .unwrap();
    std::fs::write(
        project_dir.join("src").join("main.cool"),
        "def greet() -> str:\n    return \"bundle ok\"\n\nprint(greet())\n",
    )
    .unwrap();
    std::fs::write(project_dir.join("assets").join("info.txt"), "asset\n").unwrap();
    std::fs::write(project_dir.join("README.txt"), "hello\n").unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&project_dir, &["bundle"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());
    assert!(stdout.contains("Metadata"));
    assert!(stdout.contains("Symbols"));
    assert!(stdout.contains("Bundled"));

    let artifact_name = format!("demo-0.2.0-{}", host_target_triple());
    let archive_path = project_dir.join("dist").join(format!("{artifact_name}.tar.gz"));
    let metadata_path = project_dir.join("dist").join(format!("{artifact_name}.metadata.json"));
    let symbols_path = project_dir.join("dist").join(format!("{artifact_name}.symbols.txt"));
    assert!(
        archive_path.exists(),
        "expected bundle archive at {}",
        archive_path.display()
    );
    assert!(
        metadata_path.exists(),
        "expected metadata sidecar at {}",
        metadata_path.display()
    );
    assert!(
        symbols_path.exists(),
        "expected symbol map sidecar at {}",
        symbols_path.display()
    );

    let metadata: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&metadata_path).unwrap()).unwrap();
    assert_eq!(metadata["artifact_name"].as_str().unwrap(), artifact_name);
    assert_eq!(metadata["project"]["name"].as_str().unwrap(), "demo");
    assert_eq!(metadata["project"]["version"].as_str().unwrap(), "0.2.0");
    assert_eq!(metadata["project"]["main"].as_str().unwrap(), "src/main.cool");
    assert_eq!(metadata["project"]["output"].as_str().unwrap(), "demo-bin");
    assert_eq!(metadata["build"]["artifact_kind"].as_str().unwrap(), "executable");
    assert_eq!(metadata["build"]["artifact_path"].as_str().unwrap(), "demo-bin");
    assert_eq!(metadata["bundle"]["metadata_path"].as_str().unwrap(), "metadata.json");
    assert_eq!(
        metadata["bundle"]["symbol_map_path"].as_str().unwrap(),
        "symbols/demo-bin.symbols.txt"
    );
    assert!(metadata["bundle"]["include"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item.as_str() == Some("assets")));
    assert!(metadata["bundle"]["include"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item.as_str() == Some("README.txt")));

    let symbols = std::fs::read_to_string(&symbols_path).unwrap();
    assert!(symbols.contains("greet"), "expected greet in symbol map:\n{symbols}");

    let tar_output = Command::new("tar")
        .args(["tzf", archive_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        tar_output.status.success(),
        "tar list failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&tar_output.stdout),
        String::from_utf8_lossy(&tar_output.stderr)
    );
    let listing = String::from_utf8_lossy(&tar_output.stdout);
    assert!(listing.contains(&format!("{artifact_name}/demo-bin")));
    assert!(listing.contains(&format!("{artifact_name}/assets/info.txt")));
    assert!(listing.contains(&format!("{artifact_name}/README.txt")));
    assert!(listing.contains(&format!("{artifact_name}/metadata.json")));
    assert!(listing.contains(&format!("{artifact_name}/symbols/demo-bin.symbols.txt")));

    let _ = std::fs::remove_dir_all(&project_dir);
}

#[test]
fn test_cool_bundle_packages_static_library_artifact() {
    let project_dir = unique_temp_dir("cool_project_bundle_staticlib");
    let _ = std::fs::remove_dir_all(&project_dir);
    std::fs::create_dir_all(project_dir.join("src")).unwrap();
    std::fs::write(
        project_dir.join("cool.toml"),
        r#"[project]
name = "demo"
version = "0.2.0"
main = "src/main.cool"
output = "demo-bin"

[build]
emit = "staticlib"
"#,
    )
    .unwrap();
    std::fs::write(
        project_dir.join("src").join("main.cool"),
        "def library_value() -> i32:\n    return 42\n",
    )
    .unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&project_dir, &["bundle"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());
    assert!(stdout.contains("Bundled"));

    let artifact_name = format!("demo-0.2.0-{}", host_target_triple());
    let archive_path = project_dir.join("dist").join(format!("{artifact_name}.tar.gz"));
    let metadata_path = project_dir.join("dist").join(format!("{artifact_name}.metadata.json"));
    assert!(
        archive_path.exists(),
        "expected bundle archive at {}",
        archive_path.display()
    );
    assert!(
        metadata_path.exists(),
        "expected metadata sidecar at {}",
        metadata_path.display()
    );

    let metadata: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&metadata_path).unwrap()).unwrap();
    assert_eq!(metadata["build"]["artifact_kind"].as_str().unwrap(), "staticlib");
    assert_eq!(metadata["build"]["artifact_path"].as_str().unwrap(), "libdemo-bin.a");

    let tar_output = Command::new("tar")
        .args(["tzf", archive_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        tar_output.status.success(),
        "tar list failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&tar_output.stdout),
        String::from_utf8_lossy(&tar_output.stderr)
    );
    let listing = String::from_utf8_lossy(&tar_output.stdout);
    assert!(listing.contains(&format!("{artifact_name}/libdemo-bin.a")));

    let _ = std::fs::remove_dir_all(&project_dir);
}

#[test]
fn test_cool_bundle_packages_shared_library_artifact() {
    let project_dir = unique_temp_dir("cool_project_bundle_sharedlib");
    let _ = std::fs::remove_dir_all(&project_dir);
    std::fs::create_dir_all(project_dir.join("src")).unwrap();
    std::fs::write(
        project_dir.join("cool.toml"),
        r#"[project]
name = "demo"
version = "0.2.0"
main = "src/main.cool"
output = "demo-bin"

[build]
emit = "sharedlib"
"#,
    )
    .unwrap();
    std::fs::write(
        project_dir.join("src").join("main.cool"),
        "def library_value() -> i32:\n    return 42\n",
    )
    .unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&project_dir, &["bundle"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());
    assert!(stdout.contains("Bundled"));

    let artifact_name = format!("demo-0.2.0-{}", host_target_triple());
    let archive_path = project_dir.join("dist").join(format!("{artifact_name}.tar.gz"));
    let metadata_path = project_dir.join("dist").join(format!("{artifact_name}.metadata.json"));
    let shared_name = format!("libdemo-bin.{}", host_shared_lib_ext());
    assert!(
        archive_path.exists(),
        "expected bundle archive at {}",
        archive_path.display()
    );
    assert!(
        metadata_path.exists(),
        "expected metadata sidecar at {}",
        metadata_path.display()
    );

    let metadata: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&metadata_path).unwrap()).unwrap();
    assert_eq!(metadata["build"]["artifact_kind"].as_str().unwrap(), "sharedlib");
    assert_eq!(metadata["build"]["artifact_path"].as_str().unwrap(), shared_name);

    let tar_output = Command::new("tar")
        .args(["tzf", archive_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        tar_output.status.success(),
        "tar list failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&tar_output.stdout),
        String::from_utf8_lossy(&tar_output.stderr)
    );
    let listing = String::from_utf8_lossy(&tar_output.stdout);
    assert!(listing.contains(&format!("{artifact_name}/{shared_name}")));

    let _ = std::fs::remove_dir_all(&project_dir);
}

#[test]
fn test_cool_bundle_manifest_target_names_cross_target_archive() {
    let project_dir = unique_temp_dir("cool_project_bundle_target");
    let _ = std::fs::remove_dir_all(&project_dir);
    std::fs::create_dir_all(project_dir.join("src")).unwrap();
    std::fs::write(
        project_dir.join("cool.toml"),
        format!(
            r#"[project]
name = "demo"
version = "0.2.0"
main = "src/main.cool"
output = "demo-bin"

[build]
profile = "freestanding"
target = "{target}"
"#,
            target = TEST_TARGET_I386,
        ),
    )
    .unwrap();
    std::fs::write(
        project_dir.join("src").join("main.cool"),
        "def _start():\n    entry: \"_start\"\n    return 0\n",
    )
    .unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&project_dir, &["bundle"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());
    assert!(stdout.contains("Bundled"));

    let artifact_name = format!("demo-0.2.0-{TEST_TARGET_I386}");
    let archive_path = project_dir.join("dist").join(format!("{artifact_name}.tar.gz"));
    let metadata_path = project_dir.join("dist").join(format!("{artifact_name}.metadata.json"));
    assert!(
        archive_path.exists(),
        "expected bundle archive at {}",
        archive_path.display()
    );
    assert!(
        metadata_path.exists(),
        "expected metadata sidecar at {}",
        metadata_path.display()
    );

    let metadata: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&metadata_path).unwrap()).unwrap();
    assert_eq!(metadata["target"].as_str().unwrap(), TEST_TARGET_I386);
    assert_eq!(metadata["build"]["target"].as_str().unwrap(), TEST_TARGET_I386);
    assert_eq!(metadata["build"]["artifact_kind"].as_str().unwrap(), "object");

    let _ = std::fs::remove_dir_all(&project_dir);
}

#[test]
fn test_cool_release_bumps_version_bundles_and_tags_via_cool_app() {
    let project_dir = unique_temp_dir("cool_project_release");
    let _ = std::fs::remove_dir_all(&project_dir);
    std::fs::create_dir_all(project_dir.join("src")).unwrap();
    std::fs::write(
        project_dir.join("cool.toml"),
        r#"[project]
name = "demo"
version = "0.2.0"
main = "src/main.cool"
output = "demo-bin"

[bundle]
include = ["README.txt"]
"#,
    )
    .unwrap();
    std::fs::write(project_dir.join("src").join("main.cool"), "print(\"release ok\")\n").unwrap();
    std::fs::write(project_dir.join("README.txt"), "hello\n").unwrap();

    run_git_in_dir(&project_dir, &["init"]);
    run_git_in_dir(&project_dir, &["config", "user.name", "Cool Test"]);
    run_git_in_dir(&project_dir, &["config", "user.email", "cool-test@example.com"]);
    run_git_in_dir(&project_dir, &["add", "."]);
    run_git_in_dir(&project_dir, &["commit", "-m", "init"]);

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&project_dir, &["release", "--bump", "minor"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());
    assert!(stdout.contains("Releasing demo v0.2.0 -> v0.3.0"));
    assert!(stdout.contains("Updated  cool.toml version -> 0.3.0"));
    assert!(stdout.contains("Bundled"));
    assert!(stdout.contains("Metadata"));
    assert!(stdout.contains("Symbols"));
    assert!(stdout.contains("Tagged   -> v0.3.0"));
    assert!(stdout.contains("Released demo v0.3.0"));

    let manifest = std::fs::read_to_string(project_dir.join("cool.toml")).unwrap();
    assert!(manifest.contains("version = \"0.3.0\""));

    let artifact_name = format!("demo-0.3.0-{}", host_target_triple());
    let archive_path = project_dir.join("dist").join(format!("{artifact_name}.tar.gz"));
    let metadata_path = project_dir.join("dist").join(format!("{artifact_name}.metadata.json"));
    let symbols_path = project_dir.join("dist").join(format!("{artifact_name}.symbols.txt"));
    assert!(
        archive_path.exists(),
        "expected release archive at {}",
        archive_path.display()
    );
    assert!(
        metadata_path.exists(),
        "expected metadata sidecar at {}",
        metadata_path.display()
    );
    assert!(
        symbols_path.exists(),
        "expected symbol map sidecar at {}",
        symbols_path.display()
    );

    let metadata: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&metadata_path).unwrap()).unwrap();
    assert_eq!(metadata["project"]["version"].as_str().unwrap(), "0.3.0");
    assert_eq!(metadata["archive"].as_str().unwrap(), format!("{artifact_name}.tar.gz"));

    let tags = run_git_in_dir(&project_dir, &["tag", "--list", "v0.3.0"]);
    assert_eq!(tags.trim(), "v0.3.0");

    let _ = std::fs::remove_dir_all(&project_dir);
}

#[test]
fn test_cool_release_target_flag_names_cross_target_archive() {
    let project_dir = unique_temp_dir("cool_project_release_target");
    let _ = std::fs::remove_dir_all(&project_dir);
    std::fs::create_dir_all(project_dir.join("src")).unwrap();
    std::fs::write(
        project_dir.join("cool.toml"),
        r#"[project]
name = "demo"
version = "0.2.0"
main = "src/main.cool"
output = "demo-bin"

[build]
profile = "freestanding"
"#,
    )
    .unwrap();
    std::fs::write(
        project_dir.join("src").join("main.cool"),
        "def _start():\n    entry: \"_start\"\n    return 0\n",
    )
    .unwrap();

    run_git_in_dir(&project_dir, &["init"]);
    run_git_in_dir(&project_dir, &["config", "user.name", "Cool Test"]);
    run_git_in_dir(&project_dir, &["config", "user.email", "cool-test@example.com"]);
    run_git_in_dir(&project_dir, &["add", "."]);
    run_git_in_dir(&project_dir, &["commit", "-m", "init"]);

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(
        &project_dir,
        &[
            "release",
            "--version",
            "0.2.1",
            "--no-tag",
            "--target",
            TEST_TARGET_I386,
        ],
    )
    .unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());
    assert!(stdout.contains("Released demo v0.2.1"));

    let artifact_name = format!("demo-0.2.1-{TEST_TARGET_I386}");
    let archive_path = project_dir.join("dist").join(format!("{artifact_name}.tar.gz"));
    let metadata_path = project_dir.join("dist").join(format!("{artifact_name}.metadata.json"));
    assert!(
        archive_path.exists(),
        "expected release archive at {}",
        archive_path.display()
    );
    assert!(
        metadata_path.exists(),
        "expected metadata sidecar at {}",
        metadata_path.display()
    );

    let metadata: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&metadata_path).unwrap()).unwrap();
    assert_eq!(metadata["project"]["version"].as_str().unwrap(), "0.2.1");
    assert_eq!(metadata["target"].as_str().unwrap(), TEST_TARGET_I386);
    assert_eq!(metadata["build"]["target"].as_str().unwrap(), TEST_TARGET_I386);

    let _ = std::fs::remove_dir_all(&project_dir);
}

#[test]
fn test_cool_build_freestanding_emits_project_object_file() {
    let project_dir = unique_temp_dir("cool_project_freestanding");
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

    let fn_section = if cfg!(target_os = "macos") {
        "__TEXT,__coolboot"
    } else {
        ".text.coolboot"
    };
    let data_section = if cfg!(target_os = "macos") {
        "__DATA,__coolro"
    } else {
        ".rodata.coolro"
    };
    let source = format!(
        r#"
def boot_entry():
    section: "{fn_section}"
    entry: "cool_boot_raw"
    return 7

data BOOT_MAGIC: u32 = 464367618:
    section: "{data_section}"
"#
    );
    std::fs::write(project_dir.join("src").join("main.cool"), source).unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&project_dir, &["build", "--freestanding"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());

    let object_path = project_dir.join("demo-bin.o");
    assert!(
        object_path.exists(),
        "expected object file at {}",
        object_path.display()
    );
    assert!(!project_dir.join("demo-bin").exists());
    assert!(object_has_section(&object_path, fn_section).unwrap());
    assert!(object_has_section(&object_path, data_section).unwrap());
    assert!(object_has_symbol(&object_path, "boot_entry").unwrap());
    assert!(object_has_symbol(&object_path, "cool_boot_raw").unwrap());

    let _ = std::fs::remove_dir_all(&project_dir);
}

#[test]
fn test_cool_build_freestanding_rejects_top_level_executable_statements() {
    let temp = unique_temp_path("cool_freestanding_reject", "cool");
    std::fs::write(&temp, "print(1)\n").unwrap();

    let cwd = temp.parent().unwrap();
    let file_name = temp.file_name().unwrap().to_str().unwrap();
    let (stdout, stderr, code) = run_cool_subcommand_in_dir(cwd, &["build", "--freestanding", file_name]).unwrap();
    let _ = std::fs::remove_file(&temp);
    let _ = std::fs::remove_file(temp.with_extension("o"));

    assert_ne!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.contains("freestanding build only supports top-level declarations"));
}

fn find_lld_for_test() -> Option<String> {
    let candidates = ["ld.lld", "lld", "ld.lld-19", "ld.lld-18", "ld.lld-17", "ld.lld-16"];
    for name in candidates {
        if std::process::Command::new(name)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok()
        {
            return Some(name.to_string());
        }
    }
    None
}

#[test]
fn test_cool_build_linker_script_flag_produces_elf() {
    let Some(_lld) = find_lld_for_test() else {
        eprintln!("skipping: no LLD linker found");
        return;
    };
    let dir = unique_temp_dir("cool_freestanding_link_flag");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let fn_section = if cfg!(target_os = "macos") {
        "__TEXT,__boot"
    } else {
        ".text.boot"
    };
    std::fs::write(
        dir.join("kernel.cool"),
        format!(
            r#"def _start():
    section: "{fn_section}"
    entry: "_start"
    return 0
"#
        ),
    )
    .unwrap();

    // Minimal linker script — place .text at a fixed address
    std::fs::write(
        dir.join("link.ld"),
        "ENTRY(_start)\nSECTIONS { . = 0x100000; .text : { *(.text*) } }\n",
    )
    .unwrap();

    let (stdout, stderr, code) =
        run_cool_subcommand_in_dir(&dir, &["build", "--linker-script=link.ld", "kernel.cool"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");

    let elf = dir.join("kernel.elf");
    assert!(elf.exists(), "expected kernel.elf at {}", elf.display());
    // Object file should also exist as an intermediate artifact
    assert!(dir.join("kernel.o").exists());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_cool_build_linker_script_in_toml_produces_elf() {
    let Some(_lld) = find_lld_for_test() else {
        eprintln!("skipping: no LLD linker found");
        return;
    };
    let dir = unique_temp_dir("cool_freestanding_link_toml");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();

    std::fs::write(
        dir.join("cool.toml"),
        r#"[project]
name = "mykernel"
version = "0.1.0"
main = "src/main.cool"
linker_script = "link.ld"
"#,
    )
    .unwrap();

    let fn_section = if cfg!(target_os = "macos") {
        "__TEXT,__boot"
    } else {
        ".text.boot"
    };
    std::fs::write(
        dir.join("src").join("main.cool"),
        format!(
            r#"def _start():
    section: "{fn_section}"
    entry: "_start"
    return 0
"#
        ),
    )
    .unwrap();

    std::fs::write(
        dir.join("link.ld"),
        "ENTRY(_start)\nSECTIONS { . = 0x100000; .text : { *(.text*) } }\n",
    )
    .unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&dir, &["build"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");

    let elf = dir.join("mykernel.elf");
    assert!(elf.exists(), "expected mykernel.elf at {}", elf.display());
    assert!(dir.join("mykernel.o").exists());
    // No unlinked binary should exist
    assert!(!dir.join("mykernel").exists());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_cool_build_linker_script_missing_lld_gives_clear_error() {
    // This test only runs when LLD is absent.
    if find_lld_for_test().is_some() {
        eprintln!("skipping: LLD is present");
        return;
    }
    let dir = unique_temp_dir("cool_freestanding_no_lld");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    std::fs::write(dir.join("kernel.cool"), "def _start():\n    return 0\n").unwrap();
    std::fs::write(
        dir.join("link.ld"),
        "ENTRY(_start)\nSECTIONS { . = 0x100000; .text : { *(.text*) } }\n",
    )
    .unwrap();

    let (stdout, stderr, code) =
        run_cool_subcommand_in_dir(&dir, &["build", "--linker-script=link.ld", "kernel.cool"]).unwrap();
    assert_ne!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(
        stderr.contains("no LLD linker found"),
        "expected LLD error, got:\n{stderr}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_cool_layout_inspects_static_archive_members_as_json() {
    let project_dir = unique_temp_dir("cool_layout_archive");
    let _ = std::fs::remove_dir_all(&project_dir);
    std::fs::create_dir_all(project_dir.join("src")).unwrap();
    std::fs::write(
        project_dir.join("cool.toml"),
        r#"[project]
name = "demo"
version = "0.2.0"
main = "src/main.cool"
output = "demo-bin"

[build]
emit = "staticlib"
"#,
    )
    .unwrap();
    std::fs::write(
        project_dir.join("src").join("main.cool"),
        "def library_value() -> i32:\n    return 42\n",
    )
    .unwrap();

    let (_, stderr, code) = run_cool_subcommand_in_dir(&project_dir, &["build"]).unwrap();
    assert_eq!(code, 0, "stderr:\n{stderr}");

    let archive_path = project_dir.join("libdemo-bin.a");
    let (stdout, stderr, code) =
        run_cool_subcommand_in_dir(&project_dir, &["layout", "--json", "libdemo-bin.a"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());

    let report: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(report["file_kind"].as_str().unwrap(), "archive");
    let members = report["members"].as_array().unwrap();
    assert!(members
        .iter()
        .any(|member| member["name"].as_str() == Some("demo-bin.o")));
    assert!(members
        .iter()
        .any(|member| member["name"].as_str() == Some("cool_runtime.o")));

    let _ = std::fs::remove_dir_all(&project_dir);
    let _ = std::fs::remove_file(&archive_path);
}

#[test]
fn test_cool_layout_inspects_kernel_image_entry_as_json() {
    let Some(_lld) = find_lld_for_test() else {
        eprintln!("skipping: no LLD linker found");
        return;
    };
    let dir = unique_temp_dir("cool_layout_kernel");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("kernel.cool"),
        "def _start():\n    entry: \"_start\"\n    return 0\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("link.ld"),
        "ENTRY(_start)\nSECTIONS { . = 0x100000; .text : { *(.text*) } }\n",
    )
    .unwrap();

    let (build_stdout, build_stderr, build_code) =
        run_cool_subcommand_in_dir(&dir, &["build", "--linker-script=link.ld", "kernel.cool"]).unwrap();
    assert_eq!(build_code, 0, "stdout:\n{build_stdout}\nstderr:\n{build_stderr}");

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&dir, &["layout", "--json", "kernel.elf"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());

    let report: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(report["format"].as_str().unwrap(), "elf");
    assert!(report["entry"].as_u64().unwrap() > 0);
    let symbols = report["symbols"].as_array().unwrap();
    assert!(symbols.iter().any(|symbol| symbol["name"].as_str() == Some("_start")));

    let _ = std::fs::remove_dir_all(&dir);
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
    assert!(manifest.contains("[build]"));
    assert!(manifest.contains("profile = \"dev\""));
    assert!(manifest.contains("[tasks.publish]"));
    assert!(manifest.contains("[tasks.bench]"));
    assert!(manifest.contains("[tasks.doc]"));
    assert!(manifest.contains("[tasks.fmt]"));
    let gitignore = std::fs::read_to_string(workspace_dir.join("demo").join(".gitignore")).unwrap();
    assert!(gitignore.contains(".cool/"));
    assert!(gitignore.contains("*.elf"));
    assert!(gitignore.contains("dist/"));
    let benchmark =
        std::fs::read_to_string(workspace_dir.join("demo").join("benchmarks").join("bench_main.cool")).unwrap();
    assert!(benchmark.contains("def kernel"));

    let _ = std::fs::remove_dir_all(&workspace_dir);
}

#[test]
fn test_cool_new_library_template_scaffolds_and_builds_demo() {
    let workspace_dir = unique_temp_dir("cool_new_library_template");
    let _ = std::fs::remove_dir_all(&workspace_dir);
    std::fs::create_dir_all(&workspace_dir).unwrap();

    let (stdout, stderr, code) =
        run_cool_subcommand_in_dir(&workspace_dir, &["new", "toolkit", "--template", "lib"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());
    assert!(stdout.contains("[lib]"));

    let project_dir = workspace_dir.join("toolkit");
    let manifest = std::fs::read_to_string(project_dir.join("cool.toml")).unwrap();
    assert!(manifest.contains("main = \"examples/demo.cool\""));
    assert!(manifest.contains("sources = [\"src\", \"examples\"]"));
    assert!(manifest.contains("profile = \"strict\""));
    assert!(manifest.contains("run = \"cool publish\""));
    assert!(manifest.contains("cool doc --output docs/API.md src/toolkit.cool"));

    let module_src = std::fs::read_to_string(project_dir.join("src").join("toolkit.cool")).unwrap();
    assert!(module_src.contains("public def add"));
    let test_src = std::fs::read_to_string(project_dir.join("tests").join("test_toolkit.cool")).unwrap();
    assert!(test_src.contains("import toolkit"));

    let (build_stdout, build_stderr, build_code) = run_cool_subcommand_in_dir(&project_dir, &["build"]).unwrap();
    assert_eq!(build_code, 0, "stdout:\n{build_stdout}\nstderr:\n{build_stderr}");
    assert!(build_stdout.contains("[strict]"));

    let binary = project_dir.join("toolkit");
    let output = Command::new(&binary).output().unwrap();
    assert!(output.status.success());
    let binary_stdout = String::from_utf8_lossy(&output.stdout);
    assert!(binary_stdout.contains("Hello, Cool!"));
    assert!(binary_stdout.contains("42"));

    let _ = std::fs::remove_dir_all(&workspace_dir);
}

#[test]
fn test_cool_new_service_template_scaffolds_socket_service() {
    let workspace_dir = unique_temp_dir("cool_new_service_template");
    let _ = std::fs::remove_dir_all(&workspace_dir);
    std::fs::create_dir_all(&workspace_dir).unwrap();

    let (stdout, stderr, code) =
        run_cool_subcommand_in_dir(&workspace_dir, &["new", "echoer", "--template", "service"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());
    assert!(stdout.contains("[service]"));

    let project_dir = workspace_dir.join("echoer");
    let manifest = std::fs::read_to_string(project_dir.join("cool.toml")).unwrap();
    assert!(manifest.contains("profile = \"dev\""));
    let service_src = std::fs::read_to_string(project_dir.join("src").join("main.cool")).unwrap();
    assert!(service_src.contains("import socket"));
    assert!(service_src.contains("listener = socket.listen"));

    let (build_stdout, build_stderr, build_code) = run_cool_subcommand_in_dir(&project_dir, &["build"]).unwrap();
    assert_eq!(build_code, 0, "stdout:\n{build_stdout}\nstderr:\n{build_stderr}");
    assert!(project_dir.join("echoer").exists());

    let _ = std::fs::remove_dir_all(&workspace_dir);
}

#[test]
fn test_cool_new_freestanding_template_scaffolds_kernel_project() {
    let workspace_dir = unique_temp_dir("cool_new_freestanding_template");
    let _ = std::fs::remove_dir_all(&workspace_dir);
    std::fs::create_dir_all(&workspace_dir).unwrap();

    let (stdout, stderr, code) =
        run_cool_subcommand_in_dir(&workspace_dir, &["new", "kernelkit", "--template", "freestanding"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());
    assert!(stdout.contains("[freestanding]"));

    let project_dir = workspace_dir.join("kernelkit");
    let manifest = std::fs::read_to_string(project_dir.join("cool.toml")).unwrap();
    assert!(manifest.contains("profile = \"freestanding\""));
    assert!(std::fs::read_to_string(project_dir.join("link.ld"))
        .unwrap()
        .contains("ENTRY(_start)"));
    let source = std::fs::read_to_string(project_dir.join("src").join("main.cool")).unwrap();
    assert!(source.contains("import core"));
    assert!(source.contains("entry: \"_start\""));
    assert!(source.contains("core.page_size()"));

    let (build_stdout, build_stderr, build_code) = run_cool_subcommand_in_dir(&project_dir, &["build"]).unwrap();
    assert_eq!(build_code, 0, "stdout:\n{build_stdout}\nstderr:\n{build_stderr}");
    assert!(build_stdout.contains("[freestanding, object]"));
    assert!(project_dir.join("kernelkit.o").exists());
    assert!(!project_dir.join("kernelkit").exists());

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
fn test_cool_bench_discovers_named_benchmarks() {
    let temp_dir = unique_temp_dir("cool_bench_command_discovery");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(temp_dir.join("benchmarks").join("nested")).unwrap();
    std::fs::write(
        temp_dir.join("benchmarks").join("bench_math.cool"),
        "def kernel():\n    return 2 + 2\n\nprint(kernel())\n",
    )
    .unwrap();
    std::fs::write(
        temp_dir.join("benchmarks").join("nested").join("strings_bench.cool"),
        "print(\"co\" + \"ol\")\n",
    )
    .unwrap();
    std::fs::write(temp_dir.join("benchmarks").join("helper.cool"), "print(\"skip\")\n").unwrap();

    let (stdout, stderr, code) =
        run_cool_subcommand_in_dir(&temp_dir, &["bench", "--runs", "1", "--warmups", "0"]).unwrap();
    let _ = std::fs::remove_dir_all(&temp_dir);

    assert_eq!(code, 0, "stderr was: {stderr}");
    assert!(stdout.contains("running 2 Cool benchmark file(s) with native"));
    assert!(stdout.contains("ok benchmarks/bench_math.cool"));
    assert!(stdout.contains("ok benchmarks/nested/strings_bench.cool"));
    assert!(stdout.contains("bench result: ok. 2 benchmark(s) measured; 0 failed"));
    assert!(!stdout.contains("helper.cool"));
    assert!(stderr.trim().is_empty());
}

#[test]
fn test_cool_bench_accepts_explicit_file() {
    let temp_dir = unique_temp_dir("cool_bench_command_explicit");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(temp_dir.join("perf")).unwrap();
    std::fs::write(temp_dir.join("perf").join("hotpath.cool"), "print(42)\n").unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(
        &temp_dir,
        &["bench", "--runs", "1", "--warmups", "0", "perf/hotpath.cool"],
    )
    .unwrap();
    let _ = std::fs::remove_dir_all(&temp_dir);

    assert_eq!(code, 0, "stderr was: {stderr}");
    assert!(stdout.contains("running 1 Cool benchmark file(s) with native"));
    assert!(stdout.contains("ok perf/hotpath.cool"));
    assert!(stdout.contains("bench result: ok. 1 benchmark(s) measured; 0 failed"));
    assert!(stderr.trim().is_empty());
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
fn test_cool_doc_markdown_outputs_public_api() {
    let temp_dir = unique_temp_dir("cool_doc_markdown");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    std::fs::write(
        temp_dir.join("api.cool"),
        r#""Module level docs."

public const VERSION: str = "1.0"
private const SECRET: str = "hidden"

public def greet(name: str) -> str:
    "Return a friendly greeting."
    return "Hello, " + name

private def hidden() -> str:
    "Internal helper."
    return SECRET

public class Greeter:
    "Friendly greeter."

    title = "greeter"

    public def hello(self, name: str) -> str:
        "Return a greeting from the class."
        return "Hello, " + name

public struct Pair:
    left: i32
    right: i32
"#,
    )
    .unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&temp_dir, &["doc", "api.cool"]).unwrap();
    let _ = std::fs::remove_dir_all(&temp_dir);

    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());
    assert!(stdout.contains("# Cool API Docs"));
    assert!(stdout.contains("## Module `api`"));
    assert!(stdout.contains("Module level docs."));
    assert!(stdout.contains("#### `def greet(name: str) -> str`"));
    assert!(stdout.contains("Return a friendly greeting."));
    assert!(stdout.contains("#### `class Greeter`"));
    assert!(stdout.contains("###### `def hello(self, name: str) -> str`"));
    assert!(stdout.contains("Return a greeting from the class."));
    assert!(stdout.contains("#### `struct Pair`"));
    assert!(stdout.contains("- `const VERSION: str`"));
    assert!(!stdout.contains("SECRET"));
    assert!(!stdout.contains("def hidden() -> str"));
}

#[test]
fn test_cool_doc_private_json_includes_private_symbols() {
    let temp_dir = unique_temp_dir("cool_doc_private_json");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    std::fs::write(
        temp_dir.join("api.cool"),
        r#""API docs."

public def greet(name: str) -> str:
    return name

private def hidden() -> str:
    return "secret"

private const SECRET: str = "hidden"
"#,
    )
    .unwrap();

    let (stdout, stderr, code) =
        run_cool_subcommand_in_dir(&temp_dir, &["doc", "--private", "--json", "api.cool"]).unwrap();
    let _ = std::fs::remove_dir_all(&temp_dir);

    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());

    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let modules = parsed["modules"].as_array().unwrap();
    assert_eq!(modules.len(), 1);

    let functions = modules[0]["functions"].as_array().unwrap();
    assert!(functions.iter().any(|function| {
        function["name"].as_str() == Some("greet") && function["visibility"].as_str() == Some("public")
    }));
    assert!(functions.iter().any(|function| {
        function["name"].as_str() == Some("hidden") && function["visibility"].as_str() == Some("private")
    }));

    let bindings = modules[0]["bindings"].as_array().unwrap();
    assert!(bindings.iter().any(|binding| {
        binding["name"].as_str() == Some("SECRET") && binding["visibility"].as_str() == Some("private")
    }));
}

#[test]
fn test_cool_doc_uses_project_main_and_reachable_modules() {
    let temp_dir = unique_temp_dir("cool_doc_project");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(temp_dir.join("app")).unwrap();
    std::fs::create_dir_all(temp_dir.join("lib")).unwrap();

    std::fs::write(
        temp_dir.join("cool.toml"),
        r#"[project]
name = "docdemo"
version = "0.1.0"
main = "app/main.cool"
sources = ["app", "lib"]
"#,
    )
    .unwrap();
    std::fs::write(
        temp_dir.join("app").join("main.cool"),
        r#""Entry docs."

import helper

print(helper.answer())
"#,
    )
    .unwrap();
    std::fs::write(
        temp_dir.join("lib").join("helper.cool"),
        r#""Helper docs."

public def answer() -> i32:
    "Return the answer."
    return 42
"#,
    )
    .unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&temp_dir, &["doc"]).unwrap();
    let _ = std::fs::remove_dir_all(&temp_dir);

    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());
    assert!(stdout.contains("## Module `main`"));
    assert!(stdout.contains("Entry docs."));
    assert!(stdout.contains("## Module `helper`"));
    assert!(stdout.contains("Helper docs."));
    assert!(stdout.contains("def answer() -> i32"));
    assert!(stdout.contains("Return the answer."));
}

#[test]
fn test_cool_doc_html_output_writes_file() {
    let temp_dir = unique_temp_dir("cool_doc_html");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    std::fs::write(
        temp_dir.join("api.cool"),
        r#""HTML docs."

public def greet(name: str) -> str:
    "Render greeting."
    return "Hello, " + name
"#,
    )
    .unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(
        &temp_dir,
        &["doc", "--format", "html", "--output", "docs/API.html", "api.cool"],
    )
    .unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());
    assert!(stdout.contains("Wrote docs"));

    let html = std::fs::read_to_string(temp_dir.join("docs").join("API.html")).unwrap();
    let _ = std::fs::remove_dir_all(&temp_dir);

    assert!(html.contains("<!doctype html>"));
    assert!(html.contains("<h1>Cool API Docs</h1>"));
    assert!(html.contains("Module <code>api</code>"));
    assert!(html.contains("Render greeting."));
}

#[test]
fn test_cool_doc_json_reports_extern_metadata() {
    let temp_dir = unique_temp_dir("cool_doc_extern_metadata");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    std::fs::write(
        temp_dir.join("ffi.cool"),
        r#"extern def native_sum(left: i32, right: i32) -> i32:
    symbol: "native_sum"
    cc: "c"
    section: ".text.native"
    library: "nativecalc"
    link_kind: "static"
    weak: true
    ownership: "borrowed"
    lifetime: "caller"
"#,
    )
    .unwrap();

    let (stdout, stderr, code) =
        run_cool_subcommand_in_dir(&temp_dir, &["doc", "--format", "json", "ffi.cool"]).unwrap();
    let _ = std::fs::remove_dir_all(&temp_dir);

    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());

    let report: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let function = report["modules"][0]["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|function| function["name"].as_str() == Some("native_sum"))
        .unwrap();
    let metadata = &function["extern_metadata"];
    assert_eq!(function["kind"].as_str().unwrap(), "extern");
    assert_eq!(metadata["symbol"].as_str().unwrap(), "native_sum");
    assert_eq!(metadata["callconv"].as_str().unwrap(), "c");
    assert_eq!(metadata["section"].as_str().unwrap(), ".text.native");
    assert_eq!(metadata["library"].as_str().unwrap(), "nativecalc");
    assert_eq!(metadata["link_kind"].as_str().unwrap(), "static");
    assert!(metadata["weak"].as_bool().unwrap());
    assert_eq!(metadata["ownership"].as_str().unwrap(), "borrowed");
    assert_eq!(metadata["lifetime"].as_str().unwrap(), "caller");
}

#[test]
fn test_cool_bindgen_generates_bindings_and_writes_output() {
    let temp_dir = unique_temp_dir("cool_bindgen");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    std::fs::write(
        temp_dir.join("demo.h"),
        r#"typedef unsigned long size_t;
#define LIMIT 16
enum Mode { MODE_A = 1, MODE_B };
typedef struct DemoPoint {
    int x;
    int y;
} DemoPoint;
size_t native_len(const char* text);
int native_sum(int left, int right);
"#,
    )
    .unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(
        &temp_dir,
        &[
            "bindgen",
            "--library",
            "nativecalc",
            "--link-kind",
            "static",
            "--output",
            "bindings.cool",
            "demo.h",
        ],
    )
    .unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty());
    assert!(stdout.contains("Wrote bindings"));

    let bindings = std::fs::read_to_string(temp_dir.join("bindings.cool")).unwrap();
    assert!(bindings.contains("const LIMIT = 16"), "bindings:\n{bindings}");
    assert!(bindings.contains("const MODE_A = 1"), "bindings:\n{bindings}");
    assert!(bindings.contains("struct DemoPoint:"), "bindings:\n{bindings}");
    assert!(bindings.contains("x: i32"), "bindings:\n{bindings}");
    assert!(
        bindings.contains("extern def native_len(text: str) -> usize:"),
        "bindings:\n{bindings}"
    );
    assert!(bindings.contains("library: \"nativecalc\""), "bindings:\n{bindings}");
    assert!(bindings.contains("link_kind: \"static\""), "bindings:\n{bindings}");

    let (check_stdout, check_stderr, check_code) =
        run_cool_subcommand_in_dir(&temp_dir, &["check", "bindings.cool"]).unwrap();
    let _ = std::fs::remove_dir_all(&temp_dir);

    assert_eq!(check_code, 0, "stdout:\n{check_stdout}\nstderr:\n{check_stderr}");
    assert!(check_stderr.trim().is_empty());
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
fn test_cool_check_subcommand_accepts_platform_builtin_module() {
    let temp_dir = unique_temp_dir("cool_check_platform_builtin");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(temp_dir.join("app")).unwrap();
    std::fs::write(
        temp_dir.join("app").join("main.cool"),
        "import platform\nprint(platform.runtime())\n",
    )
    .unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&temp_dir, &["check", "app/main.cool"]).unwrap();
    let _ = std::fs::remove_dir_all(&temp_dir);

    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stdout.contains("check ok: 1 module(s) checked, 0 issue(s)"));
    assert!(stderr.trim().is_empty());
}

#[test]
fn test_cool_check_subcommand_accepts_core_builtin_module() {
    let temp_dir = unique_temp_dir("cool_check_core_builtin");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(temp_dir.join("app")).unwrap();
    std::fs::write(
        temp_dir.join("app").join("main.cool"),
        "import core\nprint(core.page_size())\n",
    )
    .unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&temp_dir, &["check", "app/main.cool"]).unwrap();
    let _ = std::fs::remove_dir_all(&temp_dir);

    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stdout.contains("check ok: 1 module(s) checked, 0 issue(s)"));
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
fn test_cool_check_type_checker_catches_literal_mismatches() {
    let temp = unique_temp_path("cool_check_type_errors", "cool");
    std::fs::write(
        &temp,
        r#"def add(x: i32, y: i32) -> i32:
    return x

def greet(msg: str) -> str:
    return msg

add("hello", 2)
greet(99)
"#,
    )
    .unwrap();
    let cwd = temp.parent().unwrap();
    let file_name = temp.file_name().unwrap().to_str().unwrap();
    let (stdout, stderr, code) = run_cool_subcommand_in_dir(cwd, &["check", file_name]).unwrap();
    let _ = std::fs::remove_file(&temp);

    assert_ne!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.contains("type_error"), "expected type_error in:\n{stderr}");
    assert!(
        stderr.contains("argument 1 to 'add'"),
        "expected add error in:\n{stderr}"
    );
    assert!(
        stderr.contains("argument 1 to 'greet'"),
        "expected greet error in:\n{stderr}"
    );
}

#[test]
fn test_cool_check_type_checker_passes_compatible_literals() {
    let temp = unique_temp_path("cool_check_type_ok", "cool");
    std::fs::write(
        &temp,
        r#"def add(x: i32, y: i32) -> i32:
    return x

def scale(v: f64) -> f64:
    return v

add(1, 2)
scale(3.14)
"#,
    )
    .unwrap();
    let cwd = temp.parent().unwrap();
    let file_name = temp.file_name().unwrap().to_str().unwrap();
    let (stdout, stderr, code) = run_cool_subcommand_in_dir(cwd, &["check", file_name]).unwrap();
    let _ = std::fs::remove_file(&temp);

    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stdout.contains("check ok"));
}

#[test]
fn test_cool_check_type_checker_flags_return_type_mismatch() {
    let temp = unique_temp_path("cool_check_return_type", "cool");
    std::fs::write(
        &temp,
        r#"def get_name() -> i32:
    return "oops"
"#,
    )
    .unwrap();
    let cwd = temp.parent().unwrap();
    let file_name = temp.file_name().unwrap().to_str().unwrap();
    let (stdout, stderr, code) = run_cool_subcommand_in_dir(cwd, &["check", file_name]).unwrap();
    let _ = std::fs::remove_file(&temp);

    assert_ne!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(
        stderr.contains("return type mismatch"),
        "expected return type error in:\n{stderr}"
    );
}

#[test]
fn test_cool_check_type_checker_flags_typed_binding_mismatch() {
    let temp = unique_temp_path("cool_check_typed_binding", "cool");
    std::fs::write(&temp, "count: i32 = \"oops\"\n").unwrap();
    let cwd = temp.parent().unwrap();
    let file_name = temp.file_name().unwrap().to_str().unwrap();
    let (_, stderr, code) = run_cool_subcommand_in_dir(cwd, &["check", file_name]).unwrap();
    let _ = std::fs::remove_file(&temp);

    assert_ne!(code, 0, "expected error, got:\n{stderr}");
    assert!(
        stderr.contains("binding 'count'"),
        "expected typed binding error in:\n{stderr}"
    );
    assert!(stderr.contains("type_error"), "expected type_error in:\n{stderr}");
}

#[test]
fn test_cool_check_type_checker_flags_const_reassign() {
    let temp = unique_temp_path("cool_check_const_reassign", "cool");
    std::fs::write(&temp, "const LIMIT: i32 = 3\nLIMIT = 4\n").unwrap();
    let cwd = temp.parent().unwrap();
    let file_name = temp.file_name().unwrap().to_str().unwrap();
    let (_, stderr, code) = run_cool_subcommand_in_dir(cwd, &["check", file_name]).unwrap();
    let _ = std::fs::remove_file(&temp);

    assert_ne!(code, 0, "expected error, got:\n{stderr}");
    assert!(
        stderr.contains("immutable_reassign"),
        "expected immutable_reassign in:\n{stderr}"
    );
    assert!(stderr.contains("LIMIT"), "expected const name in:\n{stderr}");
}

#[test]
fn test_cool_check_type_checker_flags_missing_return_path() {
    let temp = unique_temp_path("cool_check_missing_return", "cool");
    std::fs::write(
        &temp,
        "def choose(flag: bool) -> i32:\n    if flag:\n        return 1\n",
    )
    .unwrap();
    let cwd = temp.parent().unwrap();
    let file_name = temp.file_name().unwrap().to_str().unwrap();
    let (_, stderr, code) = run_cool_subcommand_in_dir(cwd, &["check", file_name]).unwrap();
    let _ = std::fs::remove_file(&temp);

    assert_ne!(code, 0, "expected error, got:\n{stderr}");
    assert!(
        stderr.contains("missing_return"),
        "expected missing_return in:\n{stderr}"
    );
    assert!(stderr.contains("choose"), "expected function name in:\n{stderr}");
}

#[test]
fn test_cool_check_type_checker_catches_variable_type_mismatch() {
    let temp = unique_temp_path("cool_check_var_type", "cool");
    std::fs::write(
        &temp,
        r#"def add(x: i32, y: i32) -> i32:
    return x

bad = "hello"
add(bad, 2)
"#,
    )
    .unwrap();
    let cwd = temp.parent().unwrap();
    let file_name = temp.file_name().unwrap().to_str().unwrap();
    let (_, stderr, code) = run_cool_subcommand_in_dir(cwd, &["check", file_name]).unwrap();
    let _ = std::fs::remove_file(&temp);

    assert_ne!(code, 0, "expected error, got:\n{stderr}");
    assert!(
        stderr.contains("argument 1 to 'add'") && stderr.contains("str"),
        "expected str mismatch for 'add' in:\n{stderr}"
    );
}

#[test]
fn test_cool_check_type_checker_passes_variable_of_compatible_type() {
    let temp = unique_temp_path("cool_check_var_compat", "cool");
    std::fs::write(
        &temp,
        r#"def add(x: i32, y: i32) -> i32:
    return x

a = 1
b = 2
add(a, b)
"#,
    )
    .unwrap();
    let cwd = temp.parent().unwrap();
    let file_name = temp.file_name().unwrap().to_str().unwrap();
    let (stdout, stderr, code) = run_cool_subcommand_in_dir(cwd, &["check", file_name]).unwrap();
    let _ = std::fs::remove_file(&temp);

    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stdout.contains("check ok"));
}

#[test]
fn test_cool_check_type_checker_catches_return_variable_mismatch() {
    let temp = unique_temp_path("cool_check_return_var", "cool");
    std::fs::write(
        &temp,
        r#"def get_count() -> i32:
    msg = "oops"
    return msg
"#,
    )
    .unwrap();
    let cwd = temp.parent().unwrap();
    let file_name = temp.file_name().unwrap().to_str().unwrap();
    let (_, stderr, code) = run_cool_subcommand_in_dir(cwd, &["check", file_name]).unwrap();
    let _ = std::fs::remove_file(&temp);

    assert_ne!(code, 0, "expected error, got:\n{stderr}");
    assert!(
        stderr.contains("return type mismatch"),
        "expected return type error in:\n{stderr}"
    );
}

#[test]
fn test_cool_inspect_includes_param_types_and_return_type() {
    let temp = unique_temp_path("cool_inspect_typed", "cool");
    std::fs::write(
        &temp,
        "def add(x: i32, y: i32) -> i32:\n    return x\n\ndef greet(name):\n    return name\n",
    )
    .unwrap();
    let cwd = temp.parent().unwrap();
    let file_name = temp.file_name().unwrap().to_str().unwrap();
    let (stdout, stderr, code) = run_cool_subcommand_in_dir(cwd, &["inspect", file_name]).unwrap();
    let _ = std::fs::remove_file(&temp);

    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("inspect output must be JSON");
    let fns = json["functions"].as_array().unwrap();

    let add_fn = fns
        .iter()
        .find(|f| f["name"] == "add")
        .expect("add must be in functions");
    assert_eq!(add_fn["return_type"], "i32", "add must have return_type i32");
    let x_param = &add_fn["params"][0];
    assert_eq!(x_param["type_name"], "i32", "x param must have type_name i32");

    let greet_fn = fns
        .iter()
        .find(|f| f["name"] == "greet")
        .expect("greet must be in functions");
    assert!(
        greet_fn["return_type"].is_null(),
        "untyped greet must have no return_type"
    );
    assert!(
        greet_fn["params"][0]["type_name"].is_null(),
        "untyped param must have no type_name"
    );
}

#[test]
fn test_cool_check_type_checker_fix_suggestions_mention_conversion() {
    let temp = unique_temp_path("cool_check_fix_hint", "cool");
    std::fs::write(&temp, "def process(n: i32) -> i32:\n    return n\n\nprocess(\"bad\")\n").unwrap();
    let cwd = temp.parent().unwrap();
    let file_name = temp.file_name().unwrap().to_str().unwrap();
    let (_, stderr, code) = run_cool_subcommand_in_dir(cwd, &["check", file_name]).unwrap();
    let _ = std::fs::remove_file(&temp);

    assert_ne!(code, 0);
    assert!(
        stderr.contains("int(") || stderr.contains("convert"),
        "error should include a fix suggestion, got:\n{stderr}"
    );
}

#[test]
fn test_cool_check_type_checker_ignores_untyped_functions() {
    let temp = unique_temp_path("cool_check_untyped", "cool");
    std::fs::write(
        &temp,
        r#"def add(x, y):
    return x

add("a", "b")
add(1, 2)
add(3.14, "mixed")
"#,
    )
    .unwrap();
    let cwd = temp.parent().unwrap();
    let file_name = temp.file_name().unwrap().to_str().unwrap();
    let (stdout, stderr, code) = run_cool_subcommand_in_dir(cwd, &["check", file_name]).unwrap();
    let _ = std::fs::remove_file(&temp);

    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stdout.contains("check ok"));
}

#[test]
fn test_cool_check_strict_passes_fully_typed_program() {
    let temp = unique_temp_path("cool_check_strict_ok", "cool");
    std::fs::write(
        &temp,
        "def add(x: i32, y: i32) -> i32:\n    return x\n\nprint(add(1, 2))\n",
    )
    .unwrap();
    let cwd = temp.parent().unwrap();
    let file_name = temp.file_name().unwrap().to_str().unwrap();
    let (stdout, stderr, code) = run_cool_subcommand_in_dir(cwd, &["check", "--strict", file_name]).unwrap();
    let _ = std::fs::remove_file(&temp);
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stdout.contains("check ok"));
}

#[test]
fn test_cool_check_strict_flags_unannotated_params_and_returns() {
    let temp = unique_temp_path("cool_check_strict_missing", "cool");
    std::fs::write(
        &temp,
        "def greet(name):\n    return name\n\ndef process(data, count: i32):\n    return data\n",
    )
    .unwrap();
    let cwd = temp.parent().unwrap();
    let file_name = temp.file_name().unwrap().to_str().unwrap();
    let (stdout, stderr, code) = run_cool_subcommand_in_dir(cwd, &["check", "--strict", file_name]).unwrap();
    let _ = std::fs::remove_file(&temp);
    assert_ne!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(
        stderr.contains("unannotated_param"),
        "expected unannotated_param in:\n{stderr}"
    );
    assert!(
        stderr.contains("unannotated_return"),
        "expected unannotated_return in:\n{stderr}"
    );
    assert!(
        stderr.contains("'name' of 'greet'"),
        "expected name/greet in:\n{stderr}"
    );
}

#[test]
fn test_cool_check_strict_ignores_dunder_methods() {
    let temp = unique_temp_path("cool_check_strict_dunder", "cool");
    std::fs::write(&temp, "def __init__(self):\n    pass\n").unwrap();
    let cwd = temp.parent().unwrap();
    let file_name = temp.file_name().unwrap().to_str().unwrap();
    let (stdout, stderr, code) = run_cool_subcommand_in_dir(cwd, &["check", "--strict", file_name]).unwrap();
    let _ = std::fs::remove_file(&temp);
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stdout.contains("check ok"));
}

#[test]
fn test_cool_check_nonstrict_passes_untyped_functions() {
    let temp = unique_temp_path("cool_check_nonstrict", "cool");
    std::fs::write(&temp, "def foo(x, y):\n    return x\n").unwrap();
    let cwd = temp.parent().unwrap();
    let file_name = temp.file_name().unwrap().to_str().unwrap();
    let (stdout, _, code) = run_cool_subcommand_in_dir(cwd, &["check", file_name]).unwrap();
    let _ = std::fs::remove_file(&temp);
    assert_eq!(code, 0);
    assert!(stdout.contains("check ok"));
}

#[test]
fn test_cool_check_import_validation_flags_private_exports() {
    let temp_dir = unique_temp_dir("cool_check_private_export");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    std::fs::write(
        temp_dir.join("helper.cool"),
        "private const hidden: i32 = 1\npublic const shown: i32 = 2\n",
    )
    .unwrap();
    std::fs::write(
        temp_dir.join("main.cool"),
        "import helper\nprint(helper.shown)\nprint(helper.hidden)\n",
    )
    .unwrap();

    let (_, stderr, code) = run_cool_subcommand_in_dir(&temp_dir, &["check", "main.cool"]).unwrap();
    let _ = std::fs::remove_dir_all(&temp_dir);

    assert_ne!(code, 0, "expected error, got:\n{stderr}");
    assert!(
        stderr.contains("import_validation"),
        "expected import_validation in:\n{stderr}"
    );
    assert!(
        stderr.contains("does not export 'hidden'"),
        "expected hidden export error in:\n{stderr}"
    );
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
fn test_import_visibility_hides_private_file_exports_in_interpreter() {
    let temp_dir = unique_temp_dir("cool_import_visibility_interp");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    std::fs::write(
        temp_dir.join("helper.cool"),
        "private const hidden: i32 = 1\npublic const shown: i32 = 2\n",
    )
    .unwrap();
    std::fs::write(
        temp_dir.join("main.cool"),
        "import \"helper.cool\"\nprint(shown)\nprint(hidden)\n",
    )
    .unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&temp_dir, &["main.cool"]).unwrap();
    let _ = std::fs::remove_dir_all(&temp_dir);

    assert_ne!(
        code, 0,
        "expected runtime error, got stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("2"), "expected public export in stdout:\n{stdout}");
    assert!(
        stderr.contains("hidden") || stderr.contains("undefined variable"),
        "expected hidden export failure in:\n{stderr}"
    );
}

#[test]
fn test_import_visibility_hides_private_file_exports_in_vm() {
    let temp_dir = unique_temp_dir("cool_import_visibility_vm");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    std::fs::write(
        temp_dir.join("helper.cool"),
        "private const hidden: i32 = 1\npublic const shown: i32 = 2\n",
    )
    .unwrap();
    std::fs::write(
        temp_dir.join("main.cool"),
        "import \"helper.cool\"\nprint(shown)\nprint(hidden)\n",
    )
    .unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&temp_dir, &["--vm", "main.cool"]).unwrap();
    let _ = std::fs::remove_dir_all(&temp_dir);

    assert_ne!(
        code, 0,
        "expected runtime error, got stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("2"), "expected public export in stdout:\n{stdout}");
    assert!(
        stderr.contains("hidden") || stderr.contains("undefined"),
        "expected hidden export failure in:\n{stderr}"
    );
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
fn test_import_platform_module() {
    let result = run_cool(
        r#"import platform
print(platform.os())
print(platform.arch())
print(platform.family())
print(platform.runtime())
ext = platform.exe_ext()
print("<none>" if ext == "" else ext)
print(platform.shared_lib_ext())
print(platform.path_sep())
print(len(platform.newline()))
print(platform.is_windows())
print(platform.is_unix())
print(platform.has_ffi())
print(platform.has_raw_memory())
print(platform.has_extern())
print(platform.has_inline_asm())
"#,
    )
    .unwrap();

    let lines: Vec<_> = result
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    assert_eq!(lines, expected_platform_lines("interpreter", true, false, false, false));
}

#[test]
fn test_vm_import_platform_module() {
    let result = run_cool_vm(
        r#"import platform
print(platform.os())
print(platform.arch())
print(platform.family())
print(platform.runtime())
ext = platform.exe_ext()
print("<none>" if ext == "" else ext)
print(platform.shared_lib_ext())
print(platform.path_sep())
print(len(platform.newline()))
print(platform.is_windows())
print(platform.is_unix())
print(platform.has_ffi())
print(platform.has_raw_memory())
print(platform.has_extern())
print(platform.has_inline_asm())
"#,
    )
    .unwrap();

    let lines: Vec<_> = result
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    assert_eq!(lines, expected_platform_lines("vm", false, false, false, false));
}

#[test]
fn test_runtime_metadata_and_std_memory_in_interpreter() {
    let output = run_cool(
        r#"import platform
import std.memory
import std.runtime

class Resource:
    def __init__(self, name):
        self.name = name

    def __close__(self):
        print("close " + self.name)

items = [{"nums": [1, 2]}, 9]
dup = memory.deep_clone(items)
items[0]["nums"][0] = 77
print(dup[0]["nums"][0])

vals = [1, 2]
other = copy(vals)
vals[0] = 99
print(other[0])

scope = memory.Scope()
scope.track(Resource("a"))
scope.track(Resource("b"))
close(scope)

print(platform.runtime_profile())
print(platform.memory_model()["raw_memory"])
print(platform.panic_policy()["stack_trace"])
print(platform.thread_safety()["mode"])
print(platform.stdlib_split()["legacy_flat_imports"])
print(runtime.runtime_profile())
"#,
    )
    .unwrap();

    let lines: Vec<_> = output.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(
        lines,
        vec![
            "1",
            "1",
            "close b",
            "close a",
            "hosted",
            "false",
            "false",
            "single-threaded",
            "true",
            "hosted",
        ]
    );
}

#[test]
fn test_runtime_metadata_and_std_memory_in_vm() {
    let output = run_cool_vm(
        r#"import platform
import std.memory
import std.runtime

class Resource:
    def __init__(self, name):
        self.name = name

    def __close__(self):
        print("close " + self.name)

items = [{"nums": [1, 2]}, 9]
dup = memory.deep_clone(items)
items[0]["nums"][0] = 77
print(dup[0]["nums"][0])

vals = [1, 2]
other = copy(vals)
vals[0] = 99
print(other[0])

scope = memory.Scope()
scope.track(Resource("a"))
scope.track(Resource("b"))
close(scope)

print(platform.runtime_profile())
print(platform.memory_model()["raw_memory"])
print(platform.panic_policy()["stack_trace"])
print(platform.thread_safety()["mode"])
print(platform.stdlib_split()["legacy_flat_imports"])
print(runtime.runtime_profile())
"#,
    )
    .unwrap();

    let lines: Vec<_> = output.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(
        lines,
        vec![
            "1",
            "1",
            "close b",
            "close a",
            "hosted",
            "false",
            "false",
            "single-threaded",
            "true",
            "hosted",
        ]
    );
}

#[test]
fn test_panic_and_abort_builtins_are_fatal_in_interpreter() {
    let panic_err = run_cool("panic(\"boom\")\n").unwrap_err();
    assert!(panic_err.contains("Panic: boom"), "stderr:\n{panic_err}");

    let abort_err = run_cool("abort()\n").unwrap_err();
    assert!(abort_err.contains("Abort: program terminated"), "stderr:\n{abort_err}");
}

#[test]
fn test_panic_and_abort_builtins_are_fatal_in_vm() {
    let panic_err = run_cool_vm("panic(\"boom\")\n").unwrap_err();
    assert!(panic_err.contains("Panic: boom"), "stderr:\n{panic_err}");

    let abort_err = run_cool_vm("abort()\n").unwrap_err();
    assert!(abort_err.contains("Abort: program terminated"), "stderr:\n{abort_err}");
}

#[test]
fn test_import_core_module() {
    let result = run_cool(
        r#"import core
addr = 74565
print(core.word_bits())
print(core.word_bytes())
print(core.page_size())
print(core.page_align_down(addr))
print(core.page_align_up(addr))
print(core.page_offset(addr))
print(core.page_count(0))
print(core.page_count(1))
print(core.page_count(8193))
print(core.page_index(addr))
print(core.pt_index(addr))
print(core.pd_index(addr))
print(core.pdpt_index(addr))
print(core.pml4_index(addr))
"#,
    )
    .unwrap();

    let lines: Vec<_> = result
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    assert_eq!(lines, expected_core_lines());
}

#[test]
fn test_vm_import_core_module() {
    let result = run_cool_vm(
        r#"import core
addr = 74565
print(core.word_bits())
print(core.word_bytes())
print(core.page_size())
print(core.page_align_down(addr))
print(core.page_align_up(addr))
print(core.page_offset(addr))
print(core.page_count(0))
print(core.page_count(1))
print(core.page_count(8193))
print(core.page_index(addr))
print(core.pt_index(addr))
print(core.pd_index(addr))
print(core.pdpt_index(addr))
print(core.pml4_index(addr))
"#,
    )
    .unwrap();

    let lines: Vec<_> = result
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    assert_eq!(lines, expected_core_lines());
}

#[test]
fn test_import_core_extended_module() {
    let result = run_cool(
        r#"import core
addr = core.addr(4099)
print(addr)
print(core.addr_add(addr, 5))
print(core.addr_sub(addr, 3))
print(core.addr_diff(core.addr_add(addr, 5), addr))
print(core.addr_align_down(4099, 8))
print(core.addr_align_up(4099, 8))
print(core.addr_is_aligned(4096, 256))
print(core.addr_is_aligned(4097, 256))
print(core.string_len("cool"))
print(core.string_repeat("ab", 3))
print(core.format_hex(255))
print(core.format_bin(10))
print(core.format_ptr(4096))
items = core.list_new(2)
core.list_push(items, 7)
core.list_push(items, 8)
print(core.list_len(items))
print(core.list_pop(items))
print(core.list_len(items))
mapping = core.dict_new()
mapping["ready"] = true
print(core.dict_len(mapping))
print(core.dict_has(mapping, "ready"))
print(core.dict_has(mapping, "missing"))
"#,
    )
    .unwrap();

    let lines: Vec<_> = result
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    assert_eq!(lines, expected_extended_core_lines());
}

#[test]
fn test_vm_import_core_extended_module() {
    let result = run_cool_vm(
        r#"import core
addr = core.addr(4099)
print(addr)
print(core.addr_add(addr, 5))
print(core.addr_sub(addr, 3))
print(core.addr_diff(core.addr_add(addr, 5), addr))
print(core.addr_align_down(4099, 8))
print(core.addr_align_up(4099, 8))
print(core.addr_is_aligned(4096, 256))
print(core.addr_is_aligned(4097, 256))
print(core.string_len("cool"))
print(core.string_repeat("ab", 3))
print(core.format_hex(255))
print(core.format_bin(10))
print(core.format_ptr(4096))
items = core.list_new(2)
core.list_push(items, 7)
core.list_push(items, 8)
print(core.list_len(items))
print(core.list_pop(items))
print(core.list_len(items))
mapping = core.dict_new()
mapping["ready"] = true
print(core.dict_len(mapping))
print(core.dict_has(mapping, "ready"))
print(core.dict_has(mapping, "missing"))
"#,
    )
    .unwrap();

    let lines: Vec<_> = result
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    assert_eq!(lines, expected_extended_core_lines());
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
        .args(["apps/http.cool", "get", &url])
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
        .args(["apps/http.cool", "getjson", &json_url])
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
        .args(["apps/http.cool", "head", &body_url])
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
    let result = run_cool_stdin_with_args("apps/shell.cool", &[], "http help\nexit\n").unwrap();
    assert!(result.contains("http v1.0 — simple HTTP client"));
    assert!(result.contains("http get <url>"));
}

#[test]
fn test_shell_alias_env_and_history() {
    let input = "set NAME=Cool\necho $NAME\nalias hi echo hello\nhi\necho one\necho two\nhistory\nexit\n";
    let result = run_cool_stdin_with_args("apps/shell.cool", &[], input).unwrap();
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
    let result = run_cool_stdin_with_args("apps/shell.cool", &[], &input).unwrap();

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
    let result = run_cool_stdin_with_args("apps/shell.cool", &[], &input).unwrap();

    let _ = std::fs::remove_dir_all(&temp_dir);
    assert!(result.contains("one"));
    assert!(result.contains("two"));
}

#[test]
fn test_shell_calc_app_launch() {
    let result = run_cool_stdin_with_args("apps/shell.cool", &[], "calc\n2 + 3\nexit\nexit\n").unwrap();
    assert!(result.contains("calc v1.0 — expression calculator"));
    assert!(result.contains("= 5"));
}

#[test]
fn test_calc_app_persistent_variables() {
    let input = "x = 5\nx * 2\nexit\n";
    let result = run_cool_stdin_with_args("apps/calc.cool", &[], input).unwrap();
    assert!(result.contains("= 10"));
}

#[test]
fn test_shell_notes_app_launch() {
    let result = run_cool_stdin_with_args("apps/shell.cool", &[], "notes\nexit\nexit\n").unwrap();
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
    cmd.arg("apps/notes.cool");
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
        run_cool_with_pty_input("apps/edit.cool", &[file_path.to_str().unwrap()], b"abc\x18y").unwrap();

    let saved = std::fs::read_to_string(&file_path).unwrap();
    let _ = std::fs::remove_file(&file_path);

    assert_eq!(status, 0);
    assert!(stdout.contains("Save before exit? (y/n)"));
    assert_eq!(saved, "abc\n");
}

#[test]
fn test_snake_app_quits_on_q() {
    let (stdout, _stderr, status) = run_cool_with_pty_input("apps/snake.cool", &[], b"q").unwrap();
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
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(&lsp_message(&did_open))
        .unwrap();
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

#[test]
fn test_lsp_hover_and_completion_are_type_aware() {
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

    let source =
        "const LIMIT: i32 = 3\n\ndef add(x: i32, y: i32) -> i32:\n    return x + y\n\nanswer: i32 = add(LIMIT, 2)\n";
    let did_open_val = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": "file:///tmp/type_lsp.cool",
                "languageId": "cool",
                "version": 1,
                "text": source
            }
        }
    });
    let did_open = serde_json::to_string(&did_open_val).unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(&lsp_message(&did_open))
        .unwrap();
    let _diag_notif = read_lsp_response(&mut reader);

    let hover_req = r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/hover","params":{"textDocument":{"uri":"file:///tmp/type_lsp.cool"},"position":{"line":0,"character":7}}}"#;
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(&lsp_message(hover_req))
        .unwrap();
    let hover_resp = read_lsp_response(&mut reader);
    let hover_text = hover_resp["result"]["contents"]["value"].as_str().unwrap_or("");
    assert!(
        hover_text.contains("const LIMIT: i32"),
        "expected typed const hover in:\n{hover_text}"
    );

    let completion_req = r#"{"jsonrpc":"2.0","id":3,"method":"textDocument/completion","params":{"textDocument":{"uri":"file:///tmp/type_lsp.cool"},"position":{"line":5,"character":15}}}"#;
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(&lsp_message(completion_req))
        .unwrap();
    let completion_resp = read_lsp_response(&mut reader);
    let items = completion_resp["result"].as_array().unwrap();
    let add_item = items
        .iter()
        .find(|item| item["label"] == "add")
        .expect("add completion missing");
    let detail = add_item["detail"].as_str().unwrap_or("");
    assert!(
        detail.contains("def add(x: i32, y: i32) -> i32"),
        "expected typed function signature in completion detail, got:\n{detail}"
    );

    let exit = r#"{"jsonrpc":"2.0","method":"exit","params":null}"#;
    child.stdin.as_mut().unwrap().write_all(&lsp_message(exit)).unwrap();
    let _ = child.wait();
}

#[test]
fn test_cool_build_incremental_reproducible_debug_manifest_flags() {
    let project_dir = unique_temp_dir("cool_build_phase15_flags");
    let _ = std::fs::remove_dir_all(&project_dir);
    std::fs::create_dir_all(project_dir.join("src")).unwrap();
    std::fs::write(
        project_dir.join("cool.toml"),
        format!(
            r#"[project]
name = "demo"
version = "0.1.0"
main = "src/main.cool"

[build]
emit = "llvm-ir"
incremental = true
reproducible = true
debug = true

[toolchain]
cool = "{version}"
"#,
            version = env!("CARGO_PKG_VERSION"),
        ),
    )
    .unwrap();
    std::fs::write(
        project_dir.join("src").join("main.cool"),
        "def helper(x: i32) -> i32:\n    return x + 1\n\nprint(helper(1))\n",
    )
    .unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&project_dir, &["build"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stdout.contains("debug"), "stdout:\n{stdout}");
    assert!(stdout.contains("reproducible"), "stdout:\n{stdout}");

    let ir_path = project_dir.join("demo.ll");
    let ir = std::fs::read_to_string(&ir_path).unwrap();
    assert!(ir.contains("!DIFile"), "expected debug file metadata:\n{ir}");
    assert!(ir.contains("!dbg"), "expected debug locations:\n{ir}");
    assert!(
        !ir.contains(project_dir.to_string_lossy().as_ref()),
        "expected reproducible debug paths, got:\n{ir}"
    );

    let (stdout2, stderr2, code2) = run_cool_subcommand_in_dir(&project_dir, &["build"]).unwrap();
    assert_eq!(code2, 0, "stdout:\n{stdout2}\nstderr:\n{stderr2}");
    assert!(stdout2.contains("Finished from cache"), "stdout:\n{stdout2}");

    let _ = std::fs::remove_dir_all(&project_dir);
}

#[test]
fn test_cool_build_rejects_mismatched_toolchain_pin() {
    let project_dir = unique_temp_dir("cool_build_bad_toolchain_pin");
    let _ = std::fs::remove_dir_all(&project_dir);
    std::fs::create_dir_all(project_dir.join("src")).unwrap();
    std::fs::write(
        project_dir.join("cool.toml"),
        r#"[project]
name = "demo"
version = "0.1.0"
main = "src/main.cool"

[build]
emit = "llvm-ir"

[toolchain]
cool = "0.0.0"
"#,
    )
    .unwrap();
    std::fs::write(project_dir.join("src").join("main.cool"), "print(1)\n").unwrap();

    let (_, stderr, code) = run_cool_subcommand_in_dir(&project_dir, &["build"]).unwrap();
    assert_ne!(code, 0);
    assert!(stderr.contains("pinned Cool toolchain version"), "stderr:\n{stderr}");

    let _ = std::fs::remove_dir_all(&project_dir);
}

#[test]
fn test_native_unhandled_exception_prints_stack_trace() {
    let project_dir = unique_temp_dir("cool_build_stacktrace");
    let _ = std::fs::remove_dir_all(&project_dir);
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("cool.toml"),
        r#"[project]
name = "demo"
version = "0.1.0"
main = "main.cool"
"#,
    )
    .unwrap();
    std::fs::write(
        project_dir.join("main.cool"),
        "def leaf():\n    raise \"boom\"\n\ndef mid():\n    leaf()\n\nmid()\n",
    )
    .unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&project_dir, &["build"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");

    let binary_name = if std::env::consts::EXE_EXTENSION.is_empty() {
        "demo".to_string()
    } else {
        format!("demo.{}", std::env::consts::EXE_EXTENSION)
    };
    let output = Command::new(project_dir.join(binary_name))
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(!output.status.success(), "binary unexpectedly succeeded");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Unhandled exception: boom"), "stderr:\n{stderr}");
    assert!(stderr.contains("Stack trace"), "stderr:\n{stderr}");
    assert!(stderr.contains("leaf"), "stderr:\n{stderr}");
    assert!(stderr.contains("mid"), "stderr:\n{stderr}");
    assert!(stderr.contains("main"), "stderr:\n{stderr}");

    let _ = std::fs::remove_dir_all(&project_dir);
}

#[test]
fn test_cool_bench_profile_reports_hotspots() {
    let temp_dir = unique_temp_dir("cool_bench_profile");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(temp_dir.join("benchmarks")).unwrap();
    std::fs::write(
        temp_dir.join("benchmarks").join("bench_hot.cool"),
        r#"def hot(n):
    total = 0
    i = 0
    while i < n:
        total = total + i
        i = i + 1
    return total

print(hot(2000))
"#,
    )
    .unwrap();

    let (stdout, stderr, code) =
        run_cool_subcommand_in_dir(&temp_dir, &["bench", "--runs", "1", "--warmups", "0", "--profile"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stdout.contains("profile summary"), "stdout:\n{stdout}");
    assert!(stdout.contains("hot"), "stdout:\n{stdout}");

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_cool_fmt_reformats_code_and_preserves_comments() {
    let temp_dir = unique_temp_dir("cool_fmt_phase15");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    let file_path = temp_dir.join("main.cool");
    std::fs::write(&file_path, "# heading\nx=1+2 # sum\nif x>1:\n  print(\"ok\")\n").unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&temp_dir, &["fmt", "main.cool"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    let formatted = std::fs::read_to_string(&file_path).unwrap();
    assert!(formatted.contains("# heading"));
    assert!(formatted.contains("x = 1 + 2  # sum"), "formatted:\n{formatted}");
    assert!(formatted.contains("if x > 1:"), "formatted:\n{formatted}");
    assert!(formatted.contains("    print(\"ok\")"), "formatted:\n{formatted}");

    let (_, stderr_check, code_check) =
        run_cool_subcommand_in_dir(&temp_dir, &["fmt", "--check", "main.cool"]).unwrap();
    assert_eq!(code_check, 0, "stderr:\n{stderr_check}");

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_cool_install_locked_detects_dependency_checksum_changes() {
    let workspace_dir = unique_temp_dir("cool_install_locked_checksum");
    let _ = std::fs::remove_dir_all(&workspace_dir);
    let toolkit_dir = workspace_dir.join("toolkit");
    let app_dir = workspace_dir.join("app");
    std::fs::create_dir_all(&toolkit_dir).unwrap();
    std::fs::create_dir_all(&app_dir).unwrap();

    std::fs::write(
        toolkit_dir.join("cool.toml"),
        r#"[project]
name = "toolkit"
version = "0.1.0"
main = "main.cool"
"#,
    )
    .unwrap();
    std::fs::write(toolkit_dir.join("main.cool"), "def value() -> i32:\n    return 1\n").unwrap();

    std::fs::write(
        app_dir.join("cool.toml"),
        r#"[project]
name = "app"
version = "0.1.0"
main = "main.cool"

[dependencies]
toolkit = { path = "../toolkit" }
"#,
    )
    .unwrap();
    std::fs::write(app_dir.join("main.cool"), "print(1)\n").unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&app_dir, &["install"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    let lockfile = std::fs::read_to_string(app_dir.join("cool.lock")).unwrap();
    assert!(lockfile.contains("checksum = "), "lockfile:\n{lockfile}");

    std::fs::write(toolkit_dir.join("main.cool"), "def value() -> i32:\n    return 2\n").unwrap();
    let (_, stderr_locked, code_locked) = run_cool_subcommand_in_dir(&app_dir, &["install", "--locked"]).unwrap();
    assert_ne!(code_locked, 0);
    assert!(stderr_locked.contains("checksum changed"), "stderr:\n{stderr_locked}");

    let _ = std::fs::remove_dir_all(&workspace_dir);
}

#[test]
fn test_cool_publish_creates_source_package_and_metadata() {
    let project_dir = unique_temp_dir("cool_publish_phase15");
    let _ = std::fs::remove_dir_all(&project_dir);
    std::fs::create_dir_all(project_dir.join("src")).unwrap();
    std::fs::write(
        project_dir.join("cool.toml"),
        r#"[project]
name = "demo"
version = "0.4.0"
main = "src/main.cool"

[capabilities]
file = true
network = false
env = true
process = false
"#,
    )
    .unwrap();
    std::fs::write(project_dir.join("src").join("main.cool"), "print(\"publish\")\n").unwrap();
    std::fs::write(project_dir.join("README.md"), "demo\n").unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&project_dir, &["publish"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stdout.contains("Published source package"), "stdout:\n{stdout}");

    let archive_path = project_dir.join("dist").join("demo-0.4.0.coolpkg.tar.gz");
    let metadata_path = project_dir.join("dist").join("demo-0.4.0.publish.json");
    let lock_path = project_dir.join("cool.lock");
    assert!(archive_path.exists(), "missing {}", archive_path.display());
    assert!(metadata_path.exists(), "missing {}", metadata_path.display());
    assert!(lock_path.exists(), "missing {}", lock_path.display());

    let tar_output = Command::new("tar")
        .args(["tzf", archive_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        tar_output.status.success(),
        "tar list failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&tar_output.stdout),
        String::from_utf8_lossy(&tar_output.stderr)
    );
    let listing = String::from_utf8_lossy(&tar_output.stdout);
    assert!(listing.contains("cool.toml"), "listing:\n{listing}");
    assert!(listing.contains("cool.lock"), "listing:\n{listing}");
    assert!(listing.contains("src/main.cool"), "listing:\n{listing}");

    let metadata: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&metadata_path).unwrap()).unwrap();
    assert_eq!(metadata["package_name"].as_str().unwrap(), "demo-0.4.0");
    assert_eq!(metadata["project"]["name"].as_str().unwrap(), "demo");
    assert_eq!(metadata["project"]["version"].as_str().unwrap(), "0.4.0");
    assert_eq!(metadata["capabilities"]["network"].as_bool(), Some(false));
    assert_eq!(metadata["capabilities"]["process"].as_bool(), Some(false));
    assert!(metadata["source_hash"].as_str().unwrap().len() >= 32);

    let _ = std::fs::remove_dir_all(&project_dir);
}

#[test]
fn test_project_capabilities_deny_file_in_interpreter() {
    let project_dir = unique_temp_dir("cool_capabilities_file_interp");
    let _ = std::fs::remove_dir_all(&project_dir);
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("cool.toml"),
        "name = \"captest\"\nmain = \"main.cool\"\n\n[capabilities]\nfile = false\n",
    )
    .unwrap();
    std::fs::write(project_dir.join("main.cool"), "open(\"blocked.txt\", \"w\")\n").unwrap();

    let (_, stderr, code) = run_cool_subcommand_in_dir(&project_dir, &["main.cool"]).unwrap();
    assert_ne!(code, 0);
    assert!(
        stderr.contains("CapabilityError: file access denied"),
        "stderr:\n{stderr}"
    );

    let _ = std::fs::remove_dir_all(&project_dir);
}

#[test]
fn test_project_capabilities_deny_process_in_vm() {
    let project_dir = unique_temp_dir("cool_capabilities_process_vm");
    let _ = std::fs::remove_dir_all(&project_dir);
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("cool.toml"),
        "name = \"captest\"\nmain = \"main.cool\"\n\n[capabilities]\nprocess = false\n",
    )
    .unwrap();
    std::fs::write(
        project_dir.join("main.cool"),
        "import subprocess\nprint(subprocess.run(\"printf nope\"))\n",
    )
    .unwrap();

    let (_, stderr, code) = run_cool_subcommand_in_dir(&project_dir, &["--vm", "main.cool"]).unwrap();
    assert_ne!(code, 0);
    assert!(
        stderr.contains("CapabilityError: process access denied"),
        "stderr:\n{stderr}"
    );

    let _ = std::fs::remove_dir_all(&project_dir);
}

#[test]
fn test_platform_capabilities_reports_manifest_policy() {
    let project_dir = unique_temp_dir("cool_platform_capabilities");
    let _ = std::fs::remove_dir_all(&project_dir);
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("cool.toml"),
        "name = \"captest\"\nmain = \"main.cool\"\n\n[capabilities]\nfile = false\nnetwork = false\nenv = true\nprocess = false\n",
    )
    .unwrap();
    std::fs::write(
        project_dir.join("main.cool"),
        "import platform\nprint(platform.capabilities())\n",
    )
    .unwrap();

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&project_dir, &["main.cool"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stdout.contains("\"file\": false"), "stdout:\n{stdout}");
    assert!(stdout.contains("\"network\": false"), "stdout:\n{stdout}");
    assert!(stdout.contains("\"env\": true"), "stdout:\n{stdout}");
    assert!(stdout.contains("\"process\": false"), "stdout:\n{stdout}");

    let _ = std::fs::remove_dir_all(&project_dir);
}

#[test]
fn test_jobs_stdlib_supports_tasks_channels_and_await_all_in_interpreter() {
    let output = run_cool(
        r#"import jobs
g = jobs.group()
ch = jobs.channel(g)
jobs.send(ch, "hello")
print(jobs.recv(ch))
jobs.command(g, "printf ok", 2)
jobs.sleep(g, 0.02)
for result in jobs.await_all(g):
    print(result["kind"] + ":" + str(result["ok"]))
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines, vec!["hello", "command:true", "sleep:true"]);
}

#[test]
fn test_jobs_stdlib_supports_tasks_channels_and_await_all_in_vm() {
    let output = run_cool_vm(
        r#"import jobs
g = jobs.group()
ch = jobs.channel(g)
jobs.send(ch, "hello")
print(jobs.recv(ch))
jobs.command(g, "printf ok", 2)
jobs.sleep(g, 0.02)
for result in jobs.await_all(g):
    print(result["kind"] + ":" + str(result["ok"]))
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines, vec!["hello", "command:true", "sleep:true"]);
}

#[test]
fn test_phase6_data_stdlib_modules_in_interpreter() {
    let temp_dir = unique_temp_dir("cool_phase6_data_interp");
    let _ = std::fs::remove_dir_all(&temp_dir);
    write_phase6_data_suite(&temp_dir);
    let output = run_cool_path_with_args(&temp_dir.join("main.cool"), &[]).unwrap();
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(
        lines,
        vec![
            "41f09f9982",
            "A🙂",
            "513",
            "16909060",
            "Q29vbCE=",
            "Cool!",
            "00ff",
            "255",
            "hi",
            "&lt;tag &amp;&quot;&#39;&gt;",
            "Hi & Bye",
            "https://example.com",
            "world",
            "hello world",
            "5432",
            "hi Ada",
            "true",
            "false",
            "true",
        ]
    );
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_phase6_data_stdlib_modules_in_vm() {
    let temp_dir = unique_temp_dir("cool_phase6_data_vm");
    let _ = std::fs::remove_dir_all(&temp_dir);
    write_phase6_data_suite(&temp_dir);
    let output = run_cool_path_with_args(&temp_dir.join("main.cool"), &["--vm"]).unwrap();
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(
        lines,
        vec![
            "41f09f9982",
            "A🙂",
            "513",
            "16909060",
            "Q29vbCE=",
            "Cool!",
            "00ff",
            "255",
            "hi",
            "&lt;tag &amp;&quot;&#39;&gt;",
            "Hi & Bye",
            "https://example.com",
            "world",
            "hello world",
            "5432",
            "hi Ada",
            "true",
            "false",
            "true",
        ]
    );
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_phase6_pass2_stdlib_modules_in_interpreter() {
    let temp_dir = unique_temp_dir("cool_phase6_pass2_interp");
    let _ = std::fs::remove_dir_all(&temp_dir);
    write_phase6_pass2_suite(&temp_dir);
    let output = run_cool_path_with_args(&temp_dir.join("main.cool"), &[]).unwrap();
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(
        lines,
        vec![
            "Ada",
            "2",
            "Ada",
            "fallback",
            "7",
            "3",
            "fallback",
            "note",
            "Hello",
            "HelloHi <raw>",
            "true",
            "Ll",
            "é",
            "fi",
            "2",
            "5",
            "128578",
            "2",
            "fr-FR",
            "fr",
            "Japanese",
            "United Kingdom",
            "12 345,50",
            "12345.5",
            "19,50 €",
            "en-GB",
        ]
    );
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_phase6_pass2_stdlib_modules_in_vm() {
    let temp_dir = unique_temp_dir("cool_phase6_pass2_vm");
    let _ = std::fs::remove_dir_all(&temp_dir);
    write_phase6_pass2_suite(&temp_dir);
    let output = run_cool_path_with_args(&temp_dir.join("main.cool"), &["--vm"]).unwrap();
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(
        lines,
        vec![
            "Ada",
            "2",
            "Ada",
            "fallback",
            "7",
            "3",
            "fallback",
            "note",
            "Hello",
            "HelloHi <raw>",
            "true",
            "Ll",
            "é",
            "fi",
            "2",
            "5",
            "128578",
            "2",
            "fr-FR",
            "fr",
            "Japanese",
            "United Kingdom",
            "12 345,50",
            "12345.5",
            "19,50 €",
            "en-GB",
        ]
    );
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_phase6_pass3_stdlib_modules_in_interpreter() {
    let temp_dir = unique_temp_dir("cool_phase6_pass3_interp");
    let _ = std::fs::remove_dir_all(&temp_dir);
    write_phase6_pass3_suite(&temp_dir);
    let output =
        run_cool_path_with_args_and_env(&temp_dir.join("main.cool"), &[], &[("COOL_PHASE6_SB", "allowed")]).unwrap();
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(
        lines,
        vec![
            "missing",
            "kept",
            "prefs",
            "3",
            "true",
            "true",
            "true",
            "true",
            "true",
            "true",
            "true",
            "true",
            "safe",
            "true",
            "true",
            "true",
            "true",
            "true",
            "true",
            "true",
            "true",
            "true",
            "true",
            "source-two",
            "true",
            "true",
        ]
    );
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_phase6_pass3_stdlib_modules_in_vm() {
    let temp_dir = unique_temp_dir("cool_phase6_pass3_vm");
    let _ = std::fs::remove_dir_all(&temp_dir);
    write_phase6_pass3_suite(&temp_dir);
    let output =
        run_cool_path_with_args_and_env(&temp_dir.join("main.cool"), &["--vm"], &[("COOL_PHASE6_SB", "allowed")])
            .unwrap();
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(
        lines,
        vec![
            "missing",
            "kept",
            "prefs",
            "3",
            "true",
            "true",
            "true",
            "true",
            "true",
            "true",
            "true",
            "true",
            "safe",
            "true",
            "true",
            "true",
            "true",
            "true",
            "true",
            "true",
            "true",
            "true",
            "true",
            "source-two",
            "true",
            "true",
        ]
    );
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_phase6_filesystem_os_modules_in_interpreter() {
    let temp_dir = unique_temp_dir("cool_phase6_fs_interp");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    let output = run_cool_with_args_and_env(
        &phase6_filesystem_os_source(&temp_dir),
        &[],
        &[("COOL_PHASE6_TOKEN", "present")],
    )
    .unwrap();
    let lines: Vec<String> = output.lines().map(|line| line.to_string()).collect();
    assert_eq!(lines, expected_phase6_filesystem_os_lines("interpreter"));
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_phase6_filesystem_os_modules_in_vm() {
    let temp_dir = unique_temp_dir("cool_phase6_fs_vm");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    let output = run_cool_with_args_and_env(
        &phase6_filesystem_os_source(&temp_dir),
        &["--vm"],
        &[("COOL_PHASE6_TOKEN", "present")],
    )
    .unwrap();
    let lines: Vec<String> = output.lines().map(|line| line.to_string()).collect();
    assert_eq!(lines, expected_phase6_filesystem_os_lines("vm"));
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_cool_pkg_reports_project_info_and_capabilities() {
    let project_dir = unique_temp_dir("cool_pkg_phase17");
    let _ = std::fs::remove_dir_all(&project_dir);
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("cool.toml"),
        "name = \"pkgdemo\"\nversion = \"1.2.3\"\nmain = \"main.cool\"\n\n[capabilities]\nprocess = false\n",
    )
    .unwrap();
    std::fs::write(project_dir.join("main.cool"), "print(\"pkgdemo\")\n").unwrap();

    let (stdout_info, stderr_info, code_info) = run_cool_subcommand_in_dir(&project_dir, &["pkg", "info"]).unwrap();
    assert_eq!(code_info, 0, "stdout:\n{stdout_info}\nstderr:\n{stderr_info}");
    assert!(stdout_info.contains("pkgdemo v1.2.3"), "stdout:\n{stdout_info}");
    assert!(stdout_info.contains("Capabilities process"), "stdout:\n{stdout_info}");

    let (stdout_doctor, stderr_doctor, code_doctor) =
        run_cool_subcommand_in_dir(&project_dir, &["pkg", "doctor"]).unwrap();
    assert_eq!(code_doctor, 0, "stdout:\n{stdout_doctor}\nstderr:\n{stderr_doctor}");
    assert!(stdout_doctor.contains("OK   main file"), "stdout:\n{stdout_doctor}");

    let _ = std::fs::remove_dir_all(&project_dir);
}

#[test]
fn test_pulse_and_control_apps_run_checks_from_manifest() {
    let temp_dir = unique_temp_dir("cool_pulse_apps");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    let manifest_path = temp_dir.join("pulse.toml");
    std::fs::write(
        &manifest_path,
        "[checks.ok]\ncommand = \"printf ok\"\ntimeout = 2\n\n[checks.pause]\nsleep = 0.02\n",
    )
    .unwrap();

    let repo_root = std::env::current_dir().unwrap();
    let pulse_path = repo_root.join("apps").join("pulse.cool");
    let control_path = repo_root.join("apps").join("control.cool");

    let pulse_output = Command::new(cool_bin())
        .args([pulse_path.to_str().unwrap(), "--file", manifest_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        pulse_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&pulse_output.stdout),
        String::from_utf8_lossy(&pulse_output.stderr)
    );
    let pulse_output = String::from_utf8_lossy(&pulse_output.stdout).to_string();
    assert!(
        pulse_output.contains("2 total, 2 ok, 0 failed"),
        "stdout:\n{pulse_output}"
    );
    assert!(pulse_output.contains("command"), "stdout:\n{pulse_output}");
    assert!(pulse_output.contains("sleep"), "stdout:\n{pulse_output}");

    let control_output = Command::new(cool_bin())
        .args([
            control_path.to_str().unwrap(),
            "--file",
            manifest_path.to_str().unwrap(),
            "--once",
        ])
        .output()
        .unwrap();
    assert!(
        control_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&control_output.stdout),
        String::from_utf8_lossy(&control_output.stderr)
    );
    let control_output = String::from_utf8_lossy(&control_output.stdout).to_string();
    assert!(control_output.contains("CONTROL"), "stdout:\n{control_output}");
    assert!(control_output.contains("command"), "stdout:\n{control_output}");

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_example_coolboard_builds_and_serves_health() {
    let repo_root = std::env::current_dir().unwrap();
    let source_dir = repo_root.join("examples").join("coolboard");
    let project_dir = unique_temp_dir("coolboard_example_copy");
    let _ = std::fs::remove_dir_all(&project_dir);
    copy_dir(&source_dir, &project_dir);

    let (build_stdout, build_stderr, build_code) = run_cool_subcommand_in_dir(&project_dir, &["build"]).unwrap();
    assert_eq!(build_code, 0, "stdout:\n{build_stdout}\nstderr:\n{build_stderr}");

    let binary_path = project_dir.join("coolboard");
    assert!(binary_path.exists(), "missing {}", binary_path.display());

    let port_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = port_listener.local_addr().unwrap().port();
    drop(port_listener);

    let db_path = project_dir.join("coolboard-test.sqlite");
    let mut child = Command::new(&binary_path)
        .current_dir(&project_dir)
        .env("COOLBOARD_PORT", port.to_string())
        .env("COOLBOARD_DB", &db_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    let mut response = String::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    let addr = format!("127.0.0.1:{port}");
    while Instant::now() < deadline {
        match try_http_get_text(&addr, "/health") {
            Ok(text) => {
                response = text;
                if response_contains_coolboard_health(&response) {
                    break;
                }
            }
            Err(_) => {
                thread::sleep(Duration::from_millis(50));
            }
        }
    }

    child.kill().ok();
    let output = child.wait_with_output().unwrap();
    let child_stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let child_stderr = String::from_utf8_lossy(&output.stderr).to_string();

    assert!(
        response_contains_coolboard_health(&response),
        "response:\n{response}\nstdout:\n{child_stdout}\nstderr:\n{child_stderr}"
    );

    let _ = std::fs::remove_dir_all(&project_dir);
}

#[test]
fn test_example_kernel_demo_builds_object_artifact() {
    let repo_root = std::env::current_dir().unwrap();
    let source_dir = repo_root.join("examples").join("kernel_demo");
    let project_dir = unique_temp_dir("kernel_demo_example_copy");
    let _ = std::fs::remove_dir_all(&project_dir);
    copy_dir(&source_dir, &project_dir);

    let (stdout, stderr, code) = run_cool_subcommand_in_dir(&project_dir, &["build", "--emit", "object"]).unwrap();
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(project_dir.join("kernel_demo.o").exists());

    let _ = std::fs::remove_dir_all(&project_dir);
}

#[test]
fn test_phase13_typed_language_features_interpreter_and_vm() {
    let source = r#"
trait Named:
    def name(self) -> str

class User implements Named:
    def __init__(self, name: str):
        self.value = name

    def name(self) -> str:
        return self.value

enum Option[T]:
    Some(value: T)
    None

struct Box[T]:
    value: T

def identity[T](value: T) -> T:
    return value

def show(value: Option[int]) -> int:
    match value:
        Option.Some(inner):
            print(inner)
            return inner
        Option.None:
            print("none")
            return 0

print(identity(Box(7)).value)
print(User("Ada").name())
show(Option.Some(41))
show(Option.None)
"#;
    let interp = run_cool(source).unwrap();
    let interp_lines: Vec<_> = interp.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(interp_lines, vec!["7", "Ada", "41", "none"]);

    let vm = run_cool_vm(source).unwrap();
    let vm_lines: Vec<_> = vm.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(vm_lines, vec!["7", "Ada", "41", "none"]);
}

#[test]
fn test_cool_check_phase13_flags_non_exhaustive_match() {
    let source = r#"
enum Result[T, E]:
    Ok(value: T)
    Err(error: E)

def unwrap_or_zero(value: Result[int, str]) -> int:
    match value:
        Result.Ok(inner):
            return inner
"#;
    let err = run_cool_with_args(source, &["check"]).unwrap_err();
    assert!(err.contains("non_exhaustive_match"), "stderr:\n{err}");
    assert!(err.contains("missing_return"), "stderr:\n{err}");
}

#[test]
fn test_cool_check_phase13_flags_trait_bound_failure() {
    let source = r#"
trait Named:
    def name(self) -> str

class User implements Named:
    def name(self) -> str:
        return "ok"

class Thing:
    pass

def pick_name[T: Named](value: T) -> T:
    return value

pick_name(User())
pick_name(Thing())
"#;
    let err = run_cool_with_args(source, &["check"]).unwrap_err();
    assert!(err.contains("does not satisfy bound 'T: Named'"), "stderr:\n{err}");
}

#[test]
fn test_cool_check_phase13_flags_typed_collection_mismatch() {
    let source = r#"
def total(items: list[int]) -> int:
    return len(items)

total([1, 2, 3])
total(["x"])
"#;
    let err = run_cool_with_args(source, &["check"]).unwrap_err();
    assert!(err.contains("expected 'list[int]', got 'list[str]'"), "stderr:\n{err}");
}
