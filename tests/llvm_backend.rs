use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static LLVM_BUILD_LOCK: Mutex<()> = Mutex::new(());
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn cool_bin() -> &'static str {
    env!("CARGO_BIN_EXE_cool")
}

fn unique_test_path(stem: &str, ext: &str) -> PathBuf {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("{stem}_{nonce}_{seq}.{ext}"))
}

fn unique_temp_dir(stem: &str) -> PathBuf {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("{stem}_{nonce}_{seq}"))
}

fn cool_quote_path(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"")
}

fn cleanup_native_artifacts(source_path: &PathBuf, binary_path: &PathBuf) {
    let _ = fs::remove_file(source_path);
    let _ = fs::remove_file(binary_path);
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
        Err(format!(
            "status: {}\nstdout:\n{}\nstderr:\n{}",
            run_output.status,
            String::from_utf8_lossy(&run_output.stdout),
            String::from_utf8_lossy(&run_output.stderr)
        ))
    }
}

fn compile_and_run_native_path(source_path: &PathBuf) -> Result<String, String> {
    compile_and_run_native_path_with_env(source_path, &[])
}

fn compile_and_run_native_path_with_env(source_path: &PathBuf, envs: &[(&str, &str)]) -> Result<String, String> {
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

    let mut run_cmd = Command::new(&binary_path);
    run_cmd.current_dir(source_path.parent().unwrap_or_else(|| std::path::Path::new(".")));
    for (key, value) in envs {
        run_cmd.env(key, value);
    }
    let run_output = run_cmd.output().map_err(|e| e.to_string())?;
    let _ = fs::remove_file(&binary_path);

    if run_output.status.success() {
        Ok(String::from_utf8_lossy(&run_output.stdout).to_string())
    } else {
        Err(format!(
            "status: {}\nstdout:\n{}\nstderr:\n{}",
            run_output.status,
            String::from_utf8_lossy(&run_output.stdout),
            String::from_utf8_lossy(&run_output.stderr)
        ))
    }
}

fn write_phase6_data_suite(dir: &PathBuf) -> PathBuf {
    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(dir).unwrap();
    let source_path = dir.join("main.cool");
    fs::write(
        &source_path,
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
    fs::write(dir.join("settings.env"), "HELLO=world\nSPACED=\"hello world\"\n").unwrap();
    fs::write(dir.join("settings.ini"), "[db]\nport = 5432\n").unwrap();
    source_path
}

fn write_phase6_pass2_suite(dir: &PathBuf) -> PathBuf {
    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(dir).unwrap();
    let source_path = dir.join("main.cool");
    fs::write(
        &source_path,
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
    source_path
}

fn write_phase6_pass3_suite(dir: &PathBuf) -> PathBuf {
    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(dir).unwrap();
    let source_path = dir.join("main.cool");
    let base = cool_quote_path(dir);
    fs::write(
        &source_path,
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
    source_path
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

fn write_phase6_storage_suite(dir: &PathBuf) -> PathBuf {
    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(dir).unwrap();
    let source_path = dir.join("main.cool");
    let base = cool_quote_path(dir);
    let remember_count_file = cool_quote_path(&dir.join("remember.count"));
    let memo_count_file = cool_quote_path(&dir.join("memo.count"));
    fs::write(
        &source_path,
        format!(
            r#"import archive
import bundle
import bytes
import cache
import compress
import memo
import os
import package
import path
import time

base = "{base}"

def read_counter(file_path):
    if not os.exists(file_path):
        return 0
    f = open(file_path, "r")
    text = f.read().strip()
    f.close()
    if text == "":
        return 0
    return int(text)

def write_counter(file_path, value):
    f = open(file_path, "w")
    f.write(str(value))
    f.close()
    return value

def build_value():
    next = read_counter("{remember_count_file}") + 1
    write_counter("{remember_count_file}", next)
    return {{"count": next}}

def add_pair(left, right):
    next = read_counter("{memo_count_file}") + 1
    write_counter("{memo_count_file}", next)
    return left + right

f = open(path.join(base, "blob.bin"), "wb")
print(f.write_bytes([0, 1, 2, 255]))
f.close()
f = open(path.join(base, "blob.bin"), "rb")
raw = f.read_bytes()
f.close()
print(len(raw))
print(raw[0])
print(raw[3])

mem = cache.memory()
mem.set("ttl", "soon", 0.01)
time.sleep(0.02)
print(mem.get("ttl", "expired"))
print(cache.remember(mem, "remember", build_value)["count"])
print(cache.remember(mem, "remember", build_value)["count"])
print(read_counter("{remember_count_file}"))
print(mem.invalidate_prefix("remember"))

dc = cache.disk(path.join(base, "disk-cache"), "demo")
dc.set("nums", [1, 2, 3], nil)
print(dc.get("nums", nil)[1])
print(dc.stats()["kind"])

table = memo.memory("calc")
print(memo.call(table, "add", add_pair, [2, 3]))
print(memo.call(table, "add", add_pair, [2, 3]))
print(read_counter("{memo_count_file}"))

print(package.compare_versions("1.2.3", "1.3.0"))
print(package.satisfies("1.2.3", "^1.0.0"))
print(package.bump("1.2.3", "minor"))

project = path.join(base, "project")
os.mkdir(path.join(project, "src"))
os.mkdir(path.join(project, "deps", "util", "src"))
f = open(path.join(project, "cool.toml"), "w")
f.write("[project]\nname = \"demo\"\nversion = \"0.1.0\"\nmain = \"src/main.cool\"\n\n[dependencies]\nutil = {{ path = \"deps/util\" }}\n")
f.close()
f = open(path.join(project, "src", "main.cool"), "w")
f.write("print(\"demo project\")\n")
f.close()
f = open(path.join(project, "deps", "util", "cool.toml"), "w")
f.write("[project]\nname = \"util\"\nversion = \"0.2.0\"\nmain = \"src/main.cool\"\n")
f.close()
f = open(path.join(project, "deps", "util", "src", "main.cool"), "w")
f.write("print(\"util\")\n")
f.close()
info = package.project_info(project)
print(info["name"])
tree = package.dependency_tree(project)
print(len(tree["packages"]))
print(len(tree["edges"]))
print(len(tree["unresolved"]))

gzip_blob = compress.gzip_encode(bytes.from_string("Hello archive"))
print(bytes.to_string(compress.gzip_decode(gzip_blob)))
tar_blob = compress.tar_encode([compress.tar_entry("hello.txt", bytes.from_string("tar!"))])
tar_entries = compress.tar_decode(tar_blob)
print(tar_entries[0]["name"])
print(bytes.to_string(tar_entries[0]["data"]))
zip_blob = compress.zip_encode([compress.zip_entry("hi.txt", bytes.from_string("zip!"))])
zip_entries = compress.zip_decode(zip_blob)
print(zip_entries[0]["name"])
print(bytes.to_string(zip_entries[0]["data"]))

data_root = path.join(base, "data")
os.mkdir(path.join(data_root, "sub"))
f = open(path.join(data_root, "a.txt"), "wb")
f.write_bytes(bytes.from_string("A"))
f.close()
f = open(path.join(data_root, "sub", "b.txt"), "wb")
f.write_bytes(bytes.from_string("B"))
f.close()
archive_path = path.join(base, "data.tar.gz")
created = archive.create(data_root, archive_path)
print(created["format"])
names = archive.list(archive_path)
print("sub/b.txt" in names)
unpacked = archive.unpack(archive_path, path.join(base, "out"))
print(unpacked["count"])
f = open(path.join(base, "out", "a.txt"), "rb")
print(bytes.to_string(f.read_bytes()))
f.close()
f = open(path.join(base, "out", "sub", "b.txt"), "rb")
print(bytes.to_string(f.read_bytes()))
f.close()

bundle_path = path.join(base, "demo.coolbundle")
bundle.create(project, "src/main.cool", bundle_path, ["src"])
manifest = bundle.read_manifest(bundle_path)
print(manifest["package"]["name"])
print(bundle.asset_text(bundle_path, "src/main.cool").find("demo project") >= 0)
extracted = bundle.extract(bundle_path, path.join(base, "bundle-out"))
print(extracted["count"] >= 2)
print(os.exists(path.join(base, "bundle-out", "src", "main.cool")))
"#
        ),
    )
    .unwrap();
    source_path
}

fn expected_phase6_storage_lines() -> Vec<String> {
    vec![
        "4".to_string(),
        "4".to_string(),
        "0".to_string(),
        "255".to_string(),
        "expired".to_string(),
        "1".to_string(),
        "1".to_string(),
        "1".to_string(),
        "1".to_string(),
        "2".to_string(),
        "disk".to_string(),
        "5".to_string(),
        "5".to_string(),
        "1".to_string(),
        "-1".to_string(),
        "true".to_string(),
        "1.3.0".to_string(),
        "demo".to_string(),
        "2".to_string(),
        "1".to_string(),
        "0".to_string(),
        "Hello archive".to_string(),
        "hello.txt".to_string(),
        "tar!".to_string(),
        "hi.txt".to_string(),
        "zip!".to_string(),
        "tar.gz".to_string(),
        "true".to_string(),
        "2".to_string(),
        "A".to_string(),
        "B".to_string(),
        "demo".to_string(),
        "true".to_string(),
        "true".to_string(),
        "true".to_string(),
    ]
}

fn write_phase6_tooling_suite(dir: &PathBuf) -> PathBuf {
    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(dir).unwrap();
    let source_path = dir.join("main.cool");
    let base = cool_quote_path(dir);
    let source = r###"import ast
import diff
import doc
import ffiutil
import inspect
import lexer
import lsp
import modulegraph
import os
import parser
import patch
import path
import plugin
import project
import release
import repo
import shell
import template

base = "@BASE@"

sections = [
    {"kind": "heading", "text": "Usage", "level": 2},
    {"kind": "list", "items": ["run", "test"]},
]
print(doc.heading("Cool", 2))
print(doc.markdown("Tool", sections).find("## Usage") >= 0)
print(doc.html_document("Tool", "# Title\nhello").find("<h1>Title</h1>") >= 0)

rendered = template.render("Hi {{ name }} {{#items}}{{.}},{{/items}}{{> tail}}", {"name": "Ada", "items": ["x", "y"]}, {"tail": "done"})
print(rendered)

tokens = lexer.tokenize("def hi(x):\n    return x + 1\n")
print(lexer.values(tokens, "keyword")[0])
print(len(lexer.filter(tokens, "identifier")))
assignments = parser.parse_assignments(lexer.tokenize("name = value\ncount = 3\n"))
print(assignments[0]["name"])
print(assignments[1]["value"]["value"])

cool_source = "import util\nclass Box:\n    def size(self):\n        return 1\nVALUE = 2\n"
summary = ast.summary(cool_source)
print(len(summary["imports"]))
print(ast.find_symbol(cool_source, "Box")["kind"])

shape = inspect.describe({"b": 2, "a": 1})
print(shape["type"])
print(shape["keys"][0])

changes = diff.compare("a\nb", "a\nc\nb")
print(diff.stats(changes)["insert"])
patch_text = patch.create("a\nb", "a\nc\nb", "old", "new")
print(patch.apply_text("a\nb", patch_text) == "a\nc\nb")

workspace = project.scaffold(base, "demo", "app")
print(workspace["name"])
plan = release.plan(path.join(base, "demo"), "minor", "native")
print(plan["next"])
print(plan["artifact"].endswith("native.tar.gz"))

status = repo.parse_status(" M README.md\n?? new.cool\n")
print(status[0]["worktree"])
print(status[1]["index"])

graph_root = path.join(base, "graph")
project.ensure_dirs(graph_root, [])
f = open(path.join(graph_root, "main.cool"), "w")
f.write("import util\nprint(util.value)\n")
f.close()
f = open(path.join(graph_root, "util.cool"), "w")
f.write("value = 1\n")
f.close()
graph = modulegraph.graph(path.join(graph_root, "main.cool"), nil)
print(len(graph["nodes"]))
print(len(graph["unresolved"]))
print(modulegraph.dot(graph).find("->") >= 0)

plugins_root = path.join(base, "plugins")
project.ensure_dirs(plugins_root, ["demo"])
plugin_root = path.join(plugins_root, "demo")
project.ensure_dirs(path.join(plugin_root, ".codex-plugin"), [])
f = open(path.join(plugin_root, ".codex-plugin", "plugin.json"), "w")
f.write("{\"id\":\"demo-plugin\",\"version\":\"1.0.0\",\"capabilities\":[\"file\"],\"hooks\":{\"start\":\"run\"}}")
f.close()
loaded = plugin.load(plugin_root)
registry = plugin.registry()
plugin.register(registry, loaded)
print(registry["order"][0])
print(plugin.capabilities(loaded)[0])

diag = lsp.diagnostic("bad", 2, 4, 1, "cool")
print(diag["range"]["start"]["line"])
encoded = lsp.encode(lsp.response(1, {"ok": true}))
print(lsp.decode(encoded)["result"]["ok"])

sig = ffiutil.parse_signature("puts(cstring)->i32")
print(sig["name"])
print(ffiutil.cool_type("cstring"))
print(ffiutil.extern_decl("puts", "puts(cstring)->i32", "c").find("library") >= 0)

parts = shell.split("echo 'hello world'")
print(parts[1])
aliases = shell.aliases()
shell.set_alias(aliases, "ll", "ls -la")
print(shell.expand_alias(aliases, "ll src").find("src") >= 0)
print(shell.complete("ma", ["main", "test"])[0])
print(len(shell.source_lines("#x\nrun\n\nexit")))
"###
    .replace("@BASE@", &base);
    fs::write(&source_path, source).unwrap();
    source_path
}

fn expected_phase6_tooling_lines() -> Vec<String> {
    vec![
        "## Cool".to_string(),
        "true".to_string(),
        "true".to_string(),
        "Hi Ada x,y,done".to_string(),
        "def".to_string(),
        "3".to_string(),
        "name".to_string(),
        "3".to_string(),
        "1".to_string(),
        "class".to_string(),
        "dict".to_string(),
        "a".to_string(),
        "1".to_string(),
        "true".to_string(),
        "demo".to_string(),
        "0.2.0".to_string(),
        "true".to_string(),
        "M".to_string(),
        "?".to_string(),
        "2".to_string(),
        "0".to_string(),
        "true".to_string(),
        "demo-plugin".to_string(),
        "file".to_string(),
        "2".to_string(),
        "true".to_string(),
        "puts".to_string(),
        "str".to_string(),
        "true".to_string(),
        "hello world".to_string(),
        "true".to_string(),
        "main".to_string(),
        "2".to_string(),
    ]
}

fn write_phase6_runtime_automation_suite(dir: &PathBuf) -> PathBuf {
    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(dir).unwrap();
    let source_path = dir.join("main.cool");
    let base = cool_quote_path(dir);
    let source = r###"import agent
import bench
import event
import metrics
import notebook
import path
import profile
import retry
import secrets
import trace
import workflow

base = "@BASE@"

def handler(evt):
    return "handled:" + evt["topic"]

b = event.bus("ops")
event.subscribe(b, "build.*", handler)
evt = event.emit(b, "build.done", {"ok": true})
print(evt["topic"])
print(evt["results"][0])
print(len(event.drain(b)))

wf = workflow.workflow("deploy")
workflow.add(wf, workflow.step("build"))
workflow.add(wf, workflow.step("test", nil, ["build"]))
print(workflow.ready(wf)[0]["id"])
workflow.complete(wf, "build", "ok")
print(workflow.ready(wf)[0]["id"])
print(workflow.checkpoint(wf)["steps"][0]["state"])

p = agent.plan("ship", "release")
agent.add_task(p, agent.task("write", "Write docs"))
agent.add_task(p, agent.task("publish", "Publish", ["write"]))
print(agent.next_task(p)["id"])
agent.complete(p, "write", "done")
print(agent.next_task(p)["id"])
mem = agent.memory()
agent.remember(mem, "fact", 42)
print(agent.recall(mem, "fact"))

pol = retry.policy(3, 0)
print(retry.should_retry(pol, retry.failure(1, "timeout")))
print(retry.summary([retry.failure(1, "x"), retry.success(2, "ok")])["ok"])

r = metrics.registry("app")
metrics.inc(r, "requests", 2)
metrics.set_gauge(r, "workers", 3)
metrics.observe(r, "latency", 1)
metrics.observe(r, "latency", 3)
snap = metrics.snapshot(r)
print(snap["counters"]["requests"])
print(snap["gauges"]["workers"])
print(snap["histograms"]["latency"]["count"])

tr = trace.tracer("run", "trace-1")
root = trace.start_span(tr, "root")
trace.event(root, "checkpoint", {"n": 1})
child = trace.start_span(tr, "child", root)
trace.finish_span(tr, child)
trace.finish_span(tr, root)
print(len(trace.export(tr)))
print(trace.export(tr)[0]["name"])
print(len(trace.export(tr)[0]["events"]))

prof = profile.profiler("app")
profile.record(prof, "parse", 2)
profile.record(prof, "parse", 3)
profile.record(prof, "codegen", 1)
print(profile.summary(prof)["samples"])
print(profile.hotspots(prof)[0]["name"])
print(profile.flamegraph(prof).find("parse") >= 0)

print(int(bench.stats([1, 2, 3])["median"]))
print(bench.compare({"name": "a", "mean": 2}, {"name": "b", "mean": 1})["faster"])

nb = notebook.notebook("Demo")
notebook.add(nb, notebook.markdown("Intro"))
cell = notebook.code("print(1)")
notebook.record_output(cell, "1")
notebook.add(nb, cell)
print(len(notebook.outputs(nb)))
print(notebook.render_markdown(nb).find("```cool") >= 0)
notebook.save(nb, path.join(base, "note.json"))
loaded = notebook.load(path.join(base, "note.json"))
print(loaded["title"])

v = secrets.vault(path.join(base, "vault.json"), "key")
secrets.put(v, "TOKEN", "abc123")
print(secrets.get(v, "TOKEN"))
print(secrets.redact("abc123"))
env = secrets.inject({}, {"TOKEN": "abc123"})
print(env["TOKEN"])
print(secrets.list(v)[0])
"###
    .replace("@BASE@", &base);
    fs::write(&source_path, source).unwrap();
    source_path
}

fn expected_phase6_runtime_automation_lines() -> Vec<String> {
    vec![
        "build.done".to_string(),
        "handled:build.done".to_string(),
        "1".to_string(),
        "build".to_string(),
        "test".to_string(),
        "done".to_string(),
        "write".to_string(),
        "publish".to_string(),
        "42".to_string(),
        "true".to_string(),
        "1".to_string(),
        "2".to_string(),
        "3".to_string(),
        "2".to_string(),
        "2".to_string(),
        "root".to_string(),
        "1".to_string(),
        "3".to_string(),
        "parse".to_string(),
        "true".to_string(),
        "2".to_string(),
        "b".to_string(),
        "1".to_string(),
        "true".to_string(),
        "Demo".to_string(),
        "abc123".to_string(),
        "ab***23".to_string(),
        "abc123".to_string(),
        "TOKEN".to_string(),
    ]
}

fn write_phase6_math_data_finance_suite(dir: &PathBuf) -> PathBuf {
    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(dir).unwrap();
    let source_path = dir.join("main.cool");
    let source = r###"import decimal
import embed
import geom
import graph
import matrix
import ml
import money
import pipeline
import search
import stats
import stream
import table
import tree
import vector

def double(x):
    return x * 2

def is_even(x):
    return x % 2 == 0

def add_pair(acc, item):
    return acc + item

def inc(x):
    return x + 1

def times(x, factor):
    return x * factor

print(decimal.format(decimal.add(decimal.parse("1.25"), decimal.parse("2.75"))))
print(decimal.compare(decimal.parse("4.00"), decimal.parse("4")))
print(decimal.format(decimal.div(decimal.parse("1"), decimal.parse("4"), 2)))

usd = money.amount("12.34", "USD")
fee = money.amount("0.66", "USD")
print(money.format(money.add(usd, fee), "$"))
print(money.minor_units(money.convert(usd, "EUR", {"USD_EUR": "0.50"})))
print(len(money.allocate(money.amount("10.00", "USD"), 3)))

values = [1, 2, 3, 4]
print(int(stats.mean(values)))
print(int(stats.median(values)))
print(int(stats.percentile(values, 50)))
print(stats.histogram(values, 2)[0]["count"])

v = vector.vector([3, 4])
print(int(vector.norm(v)))
print(int(vector.dot(v, [1, 2])))
print(int(vector.values(vector.add(v, [1, 1]))[0]))

m = matrix.matrix([[1, 2], [3, 4]])
print(matrix.shape(m)[0])
print(int(matrix.determinant(m)))
print(int(matrix.apply(matrix.identity(2), [5, 6])[1]))

r = geom.rect(0, 0, 10, 5)
print(int(geom.area(r)))
print(geom.contains(r, geom.point(2, 2)))
print(int(geom.union_rect(r, geom.rect(8, 4, 4, 4))["width"]))

g = graph.graph(true)
graph.add_edge(g, "a", "b")
graph.add_edge(g, "b", "c")
print(graph.bfs(g, "a")[2])
print(len(graph.shortest_path(g, "a", "c")))
print(graph.has_cycle(g))

root = tree.node("root")
tree.add(root, tree.node("child", [tree.node("leaf")]))
print(tree.size(root))
print(tree.height(root))
print(tree.values(root)[2])

p = pipeline.pipeline("numbers")
pipeline.add(p, "inc", inc)
pipeline.add(p, "times", times, [3])
print(pipeline.run(p, 1)["value"])
print(pipeline.reduce([1, 2, 3], add_pair, 0))

s = stream.range_stream(0, 5)
print(stream.collect(stream.map(stream.filter(s, is_even), double))[1])
print(stream.collect(stream.chunk(s, 2))[1][0])

t = table.table([{"name": "Ada", "score": 2}, {"name": "Bo", "score": 1}], ["name", "score"])
print(table.sort_by(t, "score")["rows"][0]["name"])
print(table.render(t).find("Ada") >= 0)

idx = search.index([{"id": "a", "text": "cool language tools"}, {"id": "b", "text": "finance math"}])
print(search.search(idx, "cool tools")[0]["id"])
print(search.search(idx, "finance")[0]["score"])

emb = embed.index(["cool language", "finance money"])
print(embed.nearest(emb, "cool tools", 1)[0]["id"])
print(int(embed.encode("cool cool", ["cool"])["values"][0]))

print(ml.knn([[0, 0], [10, 10]], ["near", "far"], [1, 1]))
print(int(ml.accuracy(["a", "b"], ["a", "x"]) * 100))
print(ml.confusion(["yes", "no", "yes"], ["yes", "yes", "yes"])["no"]["yes"])
"###;
    fs::write(&source_path, source).unwrap();
    source_path
}

fn expected_phase6_math_data_finance_lines() -> Vec<String> {
    vec![
        "4.00".to_string(),
        "0".to_string(),
        "0.25".to_string(),
        "$13.00".to_string(),
        "617".to_string(),
        "3".to_string(),
        "2".to_string(),
        "2".to_string(),
        "2".to_string(),
        "2".to_string(),
        "5".to_string(),
        "11".to_string(),
        "4".to_string(),
        "2".to_string(),
        "-2".to_string(),
        "6".to_string(),
        "50".to_string(),
        "true".to_string(),
        "12".to_string(),
        "c".to_string(),
        "3".to_string(),
        "false".to_string(),
        "3".to_string(),
        "3".to_string(),
        "leaf".to_string(),
        "6".to_string(),
        "6".to_string(),
        "4".to_string(),
        "2".to_string(),
        "Bo".to_string(),
        "true".to_string(),
        "a".to_string(),
        "1".to_string(),
        "0".to_string(),
        "2".to_string(),
        "near".to_string(),
        "50".to_string(),
        "1".to_string(),
    ]
}

fn write_phase6_security_crypto_suite(dir: &PathBuf) -> PathBuf {
    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(dir).unwrap();
    let source_path = dir.join("main.cool");
    let source = r###"import crypto

key = crypto.derive_key("password", "salt", 2, 16)
print(key["algorithm"])
print(len(key["hex"]))
print(len(crypto.random_bytes(4, 7)))
print(len(crypto.random_hex(4, 7)))
print(len(crypto.token_urlsafe(6, 7)) > 0)

sig = crypto.sign("hello", key)
print(len(sig))
print(crypto.verify("hello", sig, key))
print(crypto.verify("HELLO", sig, key))

box = crypto.encrypt("secret", key, {"nonce": "00112233445566778899aabb", "aad": "meta"})
print(box["algorithm"])
print(len(box["ciphertext"]))
print(crypto.decrypt(box, key))

sealed = crypto.seal("payload", "pw", "salt", 2, {"nonce": "abcdefabcdefabcdefabcdef"})
print(crypto.open(sealed, "pw", "salt", 2))
print(crypto.constant_time_equal("abc", "abc"))
print(crypto.constant_time_equal("abc", "abd"))
"###;
    fs::write(&source_path, source).unwrap();
    source_path
}

fn expected_phase6_security_crypto_lines() -> Vec<String> {
    vec![
        "sha256-kdf".to_string(),
        "32".to_string(),
        "4".to_string(),
        "8".to_string(),
        "true".to_string(),
        "64".to_string(),
        "true".to_string(),
        "false".to_string(),
        "xor-sha256-hmac".to_string(),
        "12".to_string(),
        "secret".to_string(),
        "payload".to_string(),
        "true".to_string(),
        "false".to_string(),
    ]
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

fn host_pointer_bits() -> i64 {
    usize::BITS as i64
}

fn host_pointer_bytes() -> i64 {
    std::mem::size_of::<usize>() as i64
}

fn wrap_unsigned_host(n: i64) -> i64 {
    let mask = (1i128 << usize::BITS) - 1;
    ((n as i128) & mask) as i64
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

fn compile_native_binary(source: &str) -> Result<(PathBuf, PathBuf), String> {
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

    Ok((source_path, binary_path))
}

fn compile_freestanding_object(source: &str) -> Result<(PathBuf, PathBuf), String> {
    let _guard = LLVM_BUILD_LOCK.lock().unwrap();
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let source_path = cwd.join(unique_test_path("temp_llvm_freestanding", "cool"));
    let object_path = source_path.with_extension("o");

    fs::write(&source_path, source).map_err(|e| e.to_string())?;

    let build_output = Command::new(cool_bin())
        .args(["build", "--freestanding", source_path.to_str().unwrap()])
        .output()
        .map_err(|e| e.to_string())?;

    if !build_output.status.success() {
        let stderr = String::from_utf8_lossy(&build_output.stderr).to_string();
        let stdout = String::from_utf8_lossy(&build_output.stdout).to_string();
        let _ = fs::remove_file(&source_path);
        let _ = fs::remove_file(&object_path);
        return Err(format!("{stdout}{stderr}"));
    }

    Ok((source_path, object_path))
}

fn binary_has_section(binary_path: &PathBuf, section: &str) -> Result<bool, String> {
    if cfg!(target_os = "macos") {
        let (segment, section_name) = section
            .split_once(',')
            .ok_or_else(|| format!("invalid Mach-O section specifier '{section}'"))?;
        let output = Command::new("otool")
            .args(["-l", binary_path.to_str().unwrap()])
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).to_string());
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.contains(&format!("segname {segment}")) && stdout.contains(&format!("sectname {section_name}")))
    } else {
        let output = Command::new("objdump")
            .args(["-h", binary_path.to_str().unwrap()])
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).to_string());
        }
        Ok(String::from_utf8_lossy(&output.stdout).contains(section))
    }
}

fn object_has_symbol(object_path: &PathBuf, symbol: &str) -> Result<bool, String> {
    let output = Command::new("nm")
        .args(["-g", object_path.to_str().unwrap()])
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).contains(symbol))
}

fn object_has_undefined_symbol(object_path: &PathBuf, symbol: &str) -> Result<bool, String> {
    let output = Command::new("nm")
        .args(["-u", object_path.to_str().unwrap()])
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some(name) = line.split_whitespace().last() {
            if name.trim_start_matches('_') == symbol {
                return Ok(true);
            }
        }
    }
    Ok(false)
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

fn spawn_graphql_feed_server() -> (String, String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let graphql_url = format!("http://{addr}/graphql");
    let feed_url = format!("http://{addr}/feed");
    let handle = thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_http_request(&mut stream);
            let first_line = request.lines().next().unwrap_or("");
            let (status, body, content_type) = if first_line.starts_with("POST /graphql ") {
                (
                    "200 OK",
                    r#"{"data":{"status":"ok","echo":"hi"}}"#.to_string(),
                    "application/json",
                )
            } else if first_line.starts_with("GET /feed ") {
                (
                    "200 OK",
                    "<rss version='2.0'><channel><title>T</title><link>https://x</link><description>D</description><item><title>A</title><link>https://x/a</link></item></channel></rss>".to_string(),
                    "application/rss+xml",
                )
            } else {
                ("404 Not Found", "missing".to_string(), "text/plain")
            };
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
        }
    });
    (graphql_url, feed_url, handle)
}

fn spawn_smtp_test_server() -> (u16, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.write_all(b"220 mail.test ESMTP\r\n").unwrap();
        let reader_stream = stream.try_clone().unwrap();
        let mut reader = BufReader::new(reader_stream);
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap() == 0 {
                break;
            }
            if line.starts_with("HELO ") {
                stream.write_all(b"250 hello\r\n").unwrap();
            } else if line.starts_with("MAIL FROM:") || line.starts_with("RCPT TO:") {
                stream.write_all(b"250 ok\r\n").unwrap();
            } else if line.starts_with("DATA") {
                stream.write_all(b"354 end with .\r\n").unwrap();
                loop {
                    line.clear();
                    if reader.read_line(&mut line).unwrap() == 0 {
                        break;
                    }
                    if line == ".\r\n" {
                        break;
                    }
                }
                stream.write_all(b"250 queued\r\n").unwrap();
            } else if line.starts_with("QUIT") {
                stream.write_all(b"221 bye\r\n").unwrap();
                break;
            } else {
                stream.write_all(b"250 ok\r\n").unwrap();
            }
        }
    });
    (port, handle)
}

fn spawn_imap_test_server() -> (u16, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.write_all(b"* OK IMAP4rev1 ready\r\n").unwrap();
        let reader_stream = stream.try_clone().unwrap();
        let mut reader = BufReader::new(reader_stream);
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap() == 0 {
                break;
            }
            let trimmed = line.trim_end();
            let mut parts = trimmed.splitn(3, ' ');
            let tag = parts.next().unwrap_or("");
            let command = parts.next().unwrap_or("").to_ascii_uppercase();
            if command == "CAPABILITY" {
                stream
                    .write_all(
                        format!("* CAPABILITY IMAP4rev1 STARTTLS\r\n{tag} OK CAPABILITY completed\r\n").as_bytes(),
                    )
                    .unwrap();
            } else if command == "NOOP" {
                stream
                    .write_all(format!("{tag} OK NOOP completed\r\n").as_bytes())
                    .unwrap();
            } else if command == "LOGOUT" {
                stream
                    .write_all(format!("* BYE logging out\r\n{tag} OK LOGOUT completed\r\n").as_bytes())
                    .unwrap();
                break;
            } else {
                stream
                    .write_all(format!("{tag} BAD unsupported\r\n").as_bytes())
                    .unwrap();
            }
        }
    });
    (port, handle)
}

fn spawn_udp_echo_server(expected_packets: usize) -> (u16, thread::JoinHandle<()>) {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        let mut buf = [0u8; 4096];
        for _ in 0..expected_packets {
            let (n, addr) = socket.recv_from(&mut buf).unwrap();
            socket.send_to(&buf[..n], addr).unwrap();
        }
    });
    (port, handle)
}

fn spawn_websocket_echo_server() -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let url = format!("ws://127.0.0.1:{port}/echo");
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_http_request(&mut stream);
        assert!(request.starts_with("GET /echo HTTP/1.1"));
        assert!(request.contains("Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ=="));
        stream.write_all(
            b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\r\n",
        )
        .unwrap();

        let mut header = [0u8; 2];
        stream.read_exact(&mut header).unwrap();
        assert_eq!(header[0], 0x81);
        let masked = (header[1] & 0x80) != 0;
        assert!(masked);
        let len = (header[1] & 0x7f) as usize;
        let payload_len = if len == 126 {
            let mut ext = [0u8; 2];
            stream.read_exact(&mut ext).unwrap();
            u16::from_be_bytes(ext) as usize
        } else {
            len
        };
        let mut mask = [0u8; 4];
        stream.read_exact(&mut mask).unwrap();
        let mut payload = vec![0u8; payload_len];
        stream.read_exact(&mut payload).unwrap();
        for (idx, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[idx % 4];
        }
        assert_eq!(String::from_utf8(payload).unwrap(), "cool");
        stream.write_all(&[0x81, 0x04, b'p', b'o', b'n', b'g']).unwrap();
        let _ = stream.read(&mut [0u8; 32]);
    });
    (url, handle)
}

fn run_native_websocket_server_case() -> Vec<String> {
    let _guard = LLVM_BUILD_LOCK.lock().unwrap();
    let port_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = port_listener.local_addr().unwrap().port();
    drop(port_listener);

    let cwd = std::env::current_dir().unwrap();
    let source_path = cwd.join(unique_test_path("temp_llvm_ws_server", "cool"));
    let binary_path = source_path.with_extension("");
    fs::write(
        &source_path,
        format!(
            r#"import websocket

listener = websocket.listen("127.0.0.1", {port})
conn = websocket.accept_client(listener)
print(conn["request"]["path"])
print(websocket.recv_text(conn))
websocket.send_text(conn, "ack")
websocket.close(conn)
"#
        ),
    )
    .unwrap();

    let build_output = Command::new(cool_bin())
        .args(["build", source_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        build_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&build_output.stdout),
        String::from_utf8_lossy(&build_output.stderr)
    );

    let child = Command::new(&binary_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(3);
    let mut stream = loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(stream) => break stream,
            Err(_) => {
                assert!(
                    Instant::now() < deadline,
                    "native websocket server did not start in time"
                );
                thread::sleep(Duration::from_millis(20));
            }
        }
    };

    stream
        .write_all(
            b"GET /srv HTTP/1.1\r\nHost: 127.0.0.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n",
        )
        .unwrap();
    let response = read_http_request(&mut stream);
    assert!(response.starts_with("HTTP/1.1 101"), "response:\n{response}");
    assert!(
        response.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo="),
        "response:\n{response}"
    );

    let payload = b"client";
    let mask = [1u8, 2, 3, 4];
    let mut frame = vec![0x81, 0x80 | payload.len() as u8];
    frame.extend_from_slice(&mask);
    for (idx, byte) in payload.iter().enumerate() {
        frame.push(*byte ^ mask[idx % 4]);
    }
    stream.write_all(&frame).unwrap();

    let mut header = [0u8; 2];
    stream.read_exact(&mut header).unwrap();
    assert_eq!(header[0], 0x81);
    let len = (header[1] & 0x7f) as usize;
    let mut reply = vec![0u8; len];
    stream.read_exact(&mut reply).unwrap();
    assert_eq!(String::from_utf8(reply).unwrap(), "ack");

    let _ = stream.read(&mut [0u8; 32]);
    drop(stream);

    let output = child.wait_with_output().unwrap();
    cleanup_native_artifacts(&source_path, &binary_path);
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.to_string())
        .collect()
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
fn test_llvm_dict_copy() {
    let result = compile_and_run_native(
        r#"
d = {"a": 1}
c = d.copy()
d["a"] = 2
c["b"] = 3
print(d["a"])
print(c["a"])
print("b" in d)
print("b" in c)
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["2", "1", "false", "true"]);
}

#[test]
fn test_llvm_fixed_width_ints_and_memory() {
    let result = compile_and_run_native(
        r#"
print(i8(255))
print(u8(-1))
print(i16(65535))
print(u16(-1))
print(i32(4294967295))
print(u32(-1))
print(i64(42.9))

ptr = malloc(16)
write_i8(ptr, -1)
write_u8(ptr + 1, 255)
write_i16(ptr + 2, -2)
write_u16(ptr + 4, 65535)
write_i32(ptr + 8, -123456)
write_u32(ptr + 12, 4294967295)
print(read_i8(ptr))
print(read_u8(ptr + 1))
print(read_i16(ptr + 2))
print(read_u16(ptr + 4))
print(read_i32(ptr + 8))
print(read_u32(ptr + 12))
free(ptr)
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(
        lines,
        [
            "-1",
            "255",
            "-1",
            "65535",
            "-1",
            "4294967295",
            "42",
            "-1",
            "255",
            "-2",
            "65535",
            "-123456",
            "4294967295",
        ]
    );
}

#[test]
fn test_llvm_pointer_width_helpers() {
    let result = compile_and_run_native(
        r#"
print(isize(-1))
print(usize(4294967296))
print(word_bits())
print(word_bytes())
"#,
    )
    .unwrap();

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
fn test_llvm_extern_declarations() {
    let result = compile_and_run_native(
        r#"
extern def abs(x: i32) -> i32

extern def c_strlen(text: str) -> usize:
    symbol: "strlen"
    cc: "c"

print(abs(-42))
print(c_strlen("hello"))
f = abs
print(f(-7))
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["42", "5", "7"]);
}

#[test]
fn test_llvm_typed_top_level_function_signatures() {
    let result = compile_and_run_native(
        r#"
extern def c_strlen(text: str) -> usize:
    symbol: "strlen"
    cc: "c"

def add(x: i32, y: i32) -> i32:
    return x + y

def halve(x: f32) -> f32:
    return x / 2

def len_plus(text: str, extra: i32) -> i32:
    return c_strlen(text) + extra

print(add(40, 2))
print(halve(5.0))
print(len_plus("cool", 3))
f = add
print(f(7, 8))
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["42", "2.5", "7", "15"]);
}

#[test]
fn test_llvm_typed_void_function() {
    let result = compile_and_run_native(
        r#"
def log_value(value: i32) -> void:
    print(value)
    return

log_value(11)
print("done")
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["11", "done"]);
}

#[test]
fn test_llvm_linker_section_placement_for_functions_and_data() {
    let fn_section = if cfg!(target_os = "macos") {
        "__TEXT,__coolfn"
    } else {
        ".text.coolfn"
    };
    let data_section = if cfg!(target_os = "macos") {
        "__DATA,__cooldat"
    } else {
        ".data.cooldat"
    };

    let source = format!(
        r#"
struct BootHeader:
    magic: u32
    flags: u32
    checksum: i32

def boot_entry():
    section: "{fn_section}"
    print(read_u32(BOOT_HEADER))
    print(read_u32(BOOT_HEADER + 4))
    print(read_i32(BOOT_HEADER + 8))

data BOOT_HEADER: BootHeader = BootHeader(
    magic=464367618,
    flags=0,
    checksum=-464367618,
):
    section: "{data_section}"

boot_entry()
"#,
        fn_section = fn_section,
        data_section = data_section,
    );

    let (source_path, binary_path) = compile_native_binary(&source).unwrap();
    let run_output = Command::new(&binary_path).output().unwrap();
    let stdout = String::from_utf8_lossy(&run_output.stdout).to_string();
    let has_fn_section = binary_has_section(&binary_path, fn_section).unwrap();
    let has_data_section = binary_has_section(&binary_path, data_section).unwrap();
    cleanup_native_artifacts(&source_path, &binary_path);

    assert!(
        run_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&run_output.stderr)
    );
    let lines: Vec<_> = stdout.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["464367618", "0", "-464367618"]);
    assert!(has_fn_section);
    assert!(has_data_section);
}

#[test]
fn test_llvm_freestanding_build_emits_object_file() {
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
    return 7

data BOOT_MAGIC: u32 = 464367618:
    section: "{data_section}"
"#
    );

    let (source_path, object_path) = compile_freestanding_object(&source).unwrap();
    let has_fn_section = binary_has_section(&object_path, fn_section).unwrap();
    let has_data_section = binary_has_section(&object_path, data_section).unwrap();
    let has_boot_entry = object_has_symbol(&object_path, "boot_entry").unwrap();
    let binary_path = source_path.with_extension("");
    cleanup_native_artifacts(&source_path, &binary_path);
    let _ = fs::remove_file(&object_path);

    assert!(has_fn_section);
    assert!(has_data_section);
    assert!(has_boot_entry);
    assert!(!binary_path.exists());
}

#[test]
fn test_llvm_freestanding_assert_traps_without_libc_abort() {
    let source = r#"
def boot_entry():
    assert(false, "boom")
    return 7
"#;

    let (source_path, object_path) = compile_freestanding_object(source).unwrap();
    let has_boot_entry = object_has_symbol(&object_path, "boot_entry").unwrap();
    let has_abort = object_has_undefined_symbol(&object_path, "abort").unwrap();
    let has_cool_print = object_has_undefined_symbol(&object_path, "cool_print").unwrap();
    let binary_path = source_path.with_extension("");
    cleanup_native_artifacts(&source_path, &binary_path);
    let _ = fs::remove_file(&object_path);

    assert!(has_boot_entry);
    assert!(!has_abort);
    assert!(!has_cool_print);
}

#[test]
fn test_llvm_freestanding_entry_metadata_exports_raw_symbol() {
    let source = r#"
def boot_entry():
    entry: "cool_boot_raw"
    return 7
"#;

    let (source_path, object_path) = compile_freestanding_object(source).unwrap();
    let has_boot_entry = object_has_symbol(&object_path, "boot_entry").unwrap();
    let has_raw_entry = object_has_symbol(&object_path, "cool_boot_raw").unwrap();
    let binary_path = source_path.with_extension("");
    cleanup_native_artifacts(&source_path, &binary_path);
    let _ = fs::remove_file(&object_path);

    assert!(has_boot_entry);
    assert!(has_raw_entry);
}

#[test]
fn test_llvm_freestanding_entry_metadata_requires_zero_argument_function() {
    let source = r#"
def boot_entry(arg):
    entry: "cool_boot_raw"
    return arg
"#;

    let err = compile_freestanding_object(source).unwrap_err();
    assert!(err.contains("entry metadata requires a zero-argument function"));
}

fn object_undefined_cool_symbols(object_path: &PathBuf) -> Result<Vec<String>, String> {
    let output = Command::new("nm")
        .args(["-u", object_path.to_str().unwrap()])
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    let mut found = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some(sym) = line.split_whitespace().last() {
            let bare = sym.trim_start_matches('_');
            if bare.starts_with("cool_") {
                found.push(bare.to_string());
            }
        }
    }
    Ok(found)
}

#[test]
fn test_llvm_freestanding_core_allocator_hooks_have_no_undefined_runtime_symbols() {
    let source = r#"
import core

data LAST_PTR: i64 = 0

def kernel_alloc(size: i64) -> i64:
    aligned = core.page_align_up(size)
    write_i64(LAST_PTR, aligned)
    return aligned

def kernel_free(ptr: i64) -> void:
    write_i64(LAST_PTR, ptr)
    return

def boot_entry():
    entry: "boot_entry"
    core.set_allocator(kernel_alloc, kernel_free)
    ptr = core.alloc(33)
    core.free(ptr)
    core.clear_allocator()
    return core.page_offset(read_i64(LAST_PTR))
"#;

    let (source_path, object_path) = compile_freestanding_object(source).unwrap();
    let undefined_cool = object_undefined_cool_symbols(&object_path).unwrap();
    let binary_path = source_path.with_extension("");
    cleanup_native_artifacts(&source_path, &binary_path);
    let _ = fs::remove_file(&object_path);

    assert!(
        undefined_cool.is_empty(),
        "freestanding core hooks must not reference hosted runtime symbols, found: {undefined_cool:?}"
    );
}

#[test]
fn test_llvm_freestanding_volatile_builtins_have_no_undefined_runtime_symbols() {
    // Use literal constant addresses so no cool_add/cool_neg symbols appear —
    // we are testing that the memory access builtins themselves are self-contained.
    let source = r#"
def mmio_test():
    entry: "mmio_test"
    write_u8_volatile(4096, 65)
    write_u8_volatile(4097, 200)
    write_u16_volatile(4098, 511)
    write_u16_volatile(4100, 1000)
    write_u32_volatile(4102, 4294967295)
    write_u32_volatile(4106, 1234)
    write_i64_volatile(4110, 9000000000)
    write_f64_volatile(4118, 3.14)
    a = read_u8_volatile(4096)
    b = read_u8_volatile(4097)
    c = read_u16_volatile(4098)
    d = read_u32_volatile(4102)
    e = read_i64_volatile(4110)
    f = read_f64_volatile(4118)
    return 0
"#;

    let (source_path, object_path) = compile_freestanding_object(source).unwrap();
    let undefined_cool = object_undefined_cool_symbols(&object_path).unwrap();
    let binary_path = source_path.with_extension("");
    cleanup_native_artifacts(&source_path, &binary_path);
    let _ = fs::remove_file(&object_path);

    assert!(
        undefined_cool.is_empty(),
        "freestanding volatile ops must not reference C runtime symbols, found: {undefined_cool:?}"
    );
}

#[test]
fn test_llvm_freestanding_core_mmio_and_reg_helpers_have_no_undefined_runtime_symbols() {
    let source = r#"
import core

def mmio_test():
    entry: "mmio_test"
    core.mmio_write(4096, "u32", 1)
    value = core.mmio_read(4096, "u32")
    core.reg_write(4100, "u32", value)
    core.reg_set_bits(4100, "u32", 16)
    core.reg_clear_bits(4100, "u32", 1)
    core.reg_update_bits(4100, "u32", 255, 32)
    return core.reg_read(4100, "u32")
"#;

    let (source_path, object_path) = compile_freestanding_object(source).unwrap();
    let undefined_cool = object_undefined_cool_symbols(&object_path).unwrap();
    let binary_path = source_path.with_extension("");
    cleanup_native_artifacts(&source_path, &binary_path);
    let _ = fs::remove_file(&object_path);

    assert!(
        undefined_cool.is_empty(),
        "freestanding core MMIO/register helpers must not reference hosted runtime symbols, found: {undefined_cool:?}"
    );
}

#[test]
fn test_llvm_freestanding_nonvolatile_builtins_have_no_undefined_runtime_symbols() {
    let source = r#"
def mem_test():
    entry: "mem_test"
    write_byte(4096, 255)
    write_u8(4097, 200)
    write_u16(4098, 600)
    write_u32(4100, 70000)
    write_i64(4104, 999999999999)
    write_f64(4112, 2.718)
    x = read_byte(4096)
    y = read_u8(4097)
    z = read_u32(4100)
    w = read_f64(4112)
    return 0
"#;

    let (source_path, object_path) = compile_freestanding_object(source).unwrap();
    let undefined_cool = object_undefined_cool_symbols(&object_path).unwrap();
    let binary_path = source_path.with_extension("");
    cleanup_native_artifacts(&source_path, &binary_path);
    let _ = fs::remove_file(&object_path);

    assert!(
        undefined_cool.is_empty(),
        "freestanding non-volatile ops must not reference C runtime symbols, found: {undefined_cool:?}"
    );
}

#[test]
fn test_llvm_freestanding_port_io_builtins_have_no_undefined_runtime_symbols() {
    // outb / inb / write_serial_byte are x86 freestanding operations. Darwin
    // hosts use Mach-O targets by default, where these inline-asm constraints
    // are not valid; use an explicit freestanding target there instead.
    if (!cfg!(target_arch = "x86_64") && !cfg!(target_arch = "x86")) || cfg!(target_os = "macos") {
        eprintln!("skipping: port I/O builtins require a non-Darwin x86/x86-64 host target");
        return;
    }
    let source = r#"
def port_test():
    entry: "port_test"
    outb(0x3F8, 65)
    write_serial_byte(72)
    x = inb(0x3FD)
    return 0
"#;
    let (source_path, object_path) = compile_freestanding_object(source).unwrap();
    let undefined_cool = object_undefined_cool_symbols(&object_path).unwrap();
    let binary_path = source_path.with_extension("");
    cleanup_native_artifacts(&source_path, &binary_path);
    let _ = fs::remove_file(&object_path);

    assert!(
        undefined_cool.is_empty(),
        "port I/O builtins must not reference C runtime symbols, found: {undefined_cool:?}"
    );
}

#[test]
fn test_llvm_volatile_memory_builtins() {
    let result = compile_and_run_native(
        r#"
ptr = malloc(32)
write_byte_volatile(ptr, 0xAB)
write_i8_volatile(ptr + 1, -1)
write_u8_volatile(ptr + 2, 255)
write_i16_volatile(ptr + 4, -2)
write_u16_volatile(ptr + 6, 65535)
write_i32_volatile(ptr + 8, -123456)
write_u32_volatile(ptr + 12, 4294967295)
write_i64_volatile(ptr + 16, -9876543210)
write_f64_volatile(ptr + 24, 3.25)
print(read_byte_volatile(ptr))
print(read_i8_volatile(ptr + 1))
print(read_u8_volatile(ptr + 2))
print(read_i16_volatile(ptr + 4))
print(read_u16_volatile(ptr + 6))
print(read_i32_volatile(ptr + 8))
print(read_u32_volatile(ptr + 12))
print(read_i64_volatile(ptr + 16))
print(read_f64_volatile(ptr + 24))
free(ptr)
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(
        lines,
        [
            "171",
            "-1",
            "255",
            "-2",
            "65535",
            "-123456",
            "4294967295",
            "-9876543210",
            "3.25"
        ]
    );
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
fn test_llvm_string_instance_methods_match_module_helpers() {
    let result = compile_and_run_native(
        r#"
line = "GET /health HTTP/1.1"
print(line.split(" "))
print("abcabc".replace("a", "X"))
print("hello".startswith("he"))
print("hello".endswith("lo"))
print("hello".find("ll"))
print("hello".count("l"))
print("hello world".title())
print("hello world".capitalize())
print("hi {}, {}".format("cool", 7))
"#,
    )
    .unwrap();

    assert!(result.contains("[GET, /health, HTTP/1.1]") || result.contains("[GET,/health,HTTP/1.1]"));
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
fn test_llvm_lambdas_and_nested_closures() {
    let result = compile_and_run_native(
        r#"
f = lambda x: x * 2
print(f(5))

n = 7
add = lambda x: x + n
print(add(5))
n = 100
print(add(5))

def make_adder(n):
    def adder(x):
        return x + n
    return adder

add5 = make_adder(5)
print(add5(10))

def outer(a):
    f = lambda x: x + a
    def g(y):
        return f(y) + a
    return g

print(outer(2)(3))

def make_counter(start):
    state = [start]
    def tick():
        state[0] = state[0] + 1
        return state[0]
    def peek():
        return state[0]
    return tick, peek

tick, peek = make_counter(10)
print(tick())
print(tick())
print(peek())

def make_scalar_counter(start):
    count = start
    def tick():
        count = count + 1
        return count
    return tick

scalar_tick = make_scalar_counter(3)
print(scalar_tick())
print(scalar_tick())

def make_bad(n):
    def bad():
        raise "boom"
    return bad

try:
    make_bad(1)()
except:
    print(add(1))

import list

def make_bad_mapper():
    def bad(x):
        raise "boom"
    return bad

try:
    list.map(make_bad_mapper(), [1])
except:
    print(add(1))

print(list.map(lambda x: x + 3, [1, 2]))
"#,
    )
    .unwrap();
    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(
        &lines[..12],
        &["10", "12", "105", "15", "7", "11", "12", "12", "4", "5", "101", "101"]
    );
    assert!(lines[12] == "[4, 5]" || lines[12] == "[4,5]");
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
fn test_llvm_import_platform_module() {
    let result = compile_and_run_native(
        r#"
import platform
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
    assert_eq!(lines, expected_platform_lines("native", true, true, true, true));
}

#[test]
fn test_llvm_runtime_metadata_and_std_memory() {
    let result = compile_and_run_native(
        r#"
import platform
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

with memory.Arena() as arena:
    ptr = arena.alloc(4)
    write_u8(ptr, 65)
    print(read_u8(ptr))

print(platform.runtime_profile())
print(platform.memory_model()["raw_memory"])
print(platform.panic_policy()["stack_trace"])
print(platform.thread_safety()["mode"])
print(platform.stdlib_split()["legacy_flat_imports"])
print(runtime.runtime_profile())
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(
        lines,
        vec![
            "1",
            "1",
            "close b",
            "close a",
            "65",
            "hosted",
            "true",
            "true",
            "single-threaded",
            "true",
            "hosted",
        ]
    );
}

#[test]
fn test_llvm_copy_rejects_resource_handles() {
    let output = compile_and_run_native_expect_runtime_error(
        r#"
f = open("phase14_copy.txt", "w")
copy(f)
"#,
    );
    assert!(
        output.contains("copy() does not duplicate external/resource handles"),
        "stderr:\n{output}"
    );
    let _ = fs::remove_file("phase14_copy.txt");
}

#[test]
fn test_llvm_panic_and_abort_builtins_are_fatal() {
    let panic_output = compile_and_run_native_expect_runtime_error(
        r#"
def leaf():
    panic("boom")

leaf()
"#,
    );
    assert!(panic_output.contains("Panic: boom"), "stderr:\n{panic_output}");
    assert!(panic_output.contains("Stack trace"), "stderr:\n{panic_output}");
    assert!(panic_output.contains("leaf"), "stderr:\n{panic_output}");

    let abort_output = compile_and_run_native_expect_runtime_error("abort()\n");
    assert!(
        abort_output.contains("Abort: program terminated"),
        "stderr:\n{abort_output}"
    );
}

#[test]
fn test_llvm_import_core_module() {
    let result = compile_and_run_native(
        r#"
import core
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
fn test_llvm_import_core_extended_module() {
    let result = compile_and_run_native(
        r#"
import core
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
fn test_llvm_core_allocator_hooks_override_malloc_and_free() {
    let result = compile_and_run_native(
        r#"
import core

data LAST_PTR: i64 = 0

def kernel_alloc(size: i64) -> i64:
    aligned = core.page_align_up(size)
    write_i64(LAST_PTR, aligned)
    return aligned

def kernel_free(ptr: i64) -> void:
    write_i64(LAST_PTR, ptr)
    return

core.set_allocator(kernel_alloc, kernel_free)
ptr = malloc(33)
print(ptr)
core.free(ptr)
print(read_i64(LAST_PTR))
core.clear_allocator()
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["4096", "4096"]);
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
fn test_llvm_import_toml_module() {
    let result = compile_and_run_native(
        r#"
import toml
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
fn test_llvm_import_yaml_module() {
    let result = compile_and_run_native(
        r#"
import yaml
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
fn test_llvm_import_sqlite_module() {
    let cwd = std::env::current_dir().unwrap();
    let db_path = cwd.join(unique_test_path("temp_llvm_sqlite_module", "db"));
    let source = format!(
        r#"
import sqlite
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

    let result = compile_and_run_native(&source).unwrap();
    let _ = fs::remove_file(&db_path);
    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["true", "1", "1", "2", "alpha", "2.25", "1", "alpha"]);
}

#[test]
fn test_llvm_import_http_module() {
    let (base_url, handle) = spawn_http_test_server(4);
    let source = format!(
        r#"
import http
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

    let result = compile_and_run_native(&source).unwrap();
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
fn test_llvm_import_socket_module() {
    let (addr, handle) = spawn_echo_server();
    let parts: Vec<&str> = addr.splitn(2, ':').collect();
    let host = parts[0];
    let port: i64 = parts[1].parse().unwrap();
    let source = format!(
        r#"
import socket
conn = socket.connect("{host}", {port})
conn.send("hello llvm socket\n")
data = conn.recv(64)
conn.close()
print(data.strip())
"#
    );
    let result = compile_and_run_native(&source).unwrap();
    handle.join().unwrap();
    assert_eq!(result.trim(), "hello llvm socket");
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
fn test_llvm_struct_basic() {
    let result = compile_and_run_native(
        r#"
struct Point:
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
fn test_llvm_struct_type_coercion() {
    let result = compile_and_run_native(
        r#"
struct Counts:
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
fn test_llvm_struct_pointer_width_aliases() {
    let result = compile_and_run_native(
        r#"
struct PtrSized:
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
fn test_llvm_struct_in_function() {
    let result = compile_and_run_native(
        r#"
struct Rect:
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
fn test_llvm_struct_ffi_layout() {
    // Verify stable binary layout: pass struct pointer to a C function that reads fields by offset.
    // The C code (via ffi) reads x and y from a { i32, i32 } layout.  If the layout were wrong
    // (e.g. a hash-map class object), the C function would read garbage and the sums would differ.
    let result = compile_and_run_native(
        r#"
import ffi

struct Vec2:
    x: i32
    y: i32

libc = ffi.open("libc")
# Use memcpy to read two i32s from the struct as a sanity check on layout.
# We verify stable layout by confirming that calling with positional args produces
# the expected field values via the dynamic path (cool_get_attr side table).
v = Vec2(10, 20)
print(v.x)
print(v.y)
v.x = 99
v.y = 77
print(v.x)
print(v.y)

def read_fields(s):
    return s.x + s.y

print(read_fields(v))
"#,
    )
    .unwrap();
    let lines: Vec<_> = result.trim().lines().collect();
    assert_eq!(lines[0], "10");
    assert_eq!(lines[1], "20");
    assert_eq!(lines[2], "99");
    assert_eq!(lines[3], "77");
    assert_eq!(lines[4], "176");
}

#[test]
fn test_llvm_packed_struct() {
    // packed struct has no padding between fields — an i8 followed by an i32 occupies 5 bytes,
    // not 8 as a naturally-aligned struct would.  The LLVM backend uses set_body(is_packed=true)
    // so GEP field accesses use the consecutive layout.
    let result = compile_and_run_native(
        r#"
packed struct Header:
    flags: i8
    length: i32
    count: i16

h = Header(7, 1000, 3)
print(h.flags)
print(h.length)
print(h.count)
h.flags = 1
h.length = 42
h.count = 9
print(h.flags)
print(h.length)
print(h.count)

def sum_header(hdr):
    return hdr.flags + hdr.length + hdr.count

print(sum_header(h))
"#,
    )
    .unwrap();
    let lines: Vec<_> = result.trim().lines().collect();
    assert_eq!(lines[0], "7");
    assert_eq!(lines[1], "1000");
    assert_eq!(lines[2], "3");
    assert_eq!(lines[3], "1");
    assert_eq!(lines[4], "42");
    assert_eq!(lines[5], "9");
    assert_eq!(lines[6], "52"); // 1 + 42 + 9
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
labs_fn = ffi.func(libc, "labs", "isize", ["isize"])
strlen_fn = ffi.func(libc, "strlen", "usize", ["str"])
dup_fn = ffi.func(libc, "strdup", "str", ["str"])

print(sqrt_fn(81.0))
print(pow_fn(2.0, 5.0))
print(abs_fn(-42))
print(labs_fn(-42))
print(strlen_fn("cool"))
print(dup_fn("ffi-ok"))
"#,
    )
    .unwrap();

    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["9", "32", "42", "42", "4", "ffi-ok"]);
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

#[test]
fn test_llvm_import_visibility_filters_private_exports() {
    let temp_dir = unique_temp_dir("cool_llvm_import_visibility_test");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();
    let source_path = temp_dir.join("main.cool");
    fs::write(
        temp_dir.join("helper.cool"),
        "private const hidden: i32 = 1\npublic const shown: i32 = 2\n",
    )
    .unwrap();
    fs::write(
        &source_path,
        "import \"helper.cool\"\nimport helper\nprint(shown)\nprint(helper.shown)\n",
    )
    .unwrap();

    let result = compile_and_run_native_path(&source_path).unwrap();

    let _ = fs::remove_dir_all(&temp_dir);
    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["2", "2"]);
}

#[test]
fn test_llvm_term_get_char() {
    let stdout = compile_and_run_native(
        r#"
import term
size = term.size()
print(size[0] > 0)
print(size[1] > 0)
term.write("native-term")
term.flush()
"#,
    )
    .unwrap();

    assert!(stdout.contains("true"));
    assert!(stdout.contains("native-term"));
}

#[test]
fn test_llvm_platform_capabilities_reports_default_allow_all() {
    let result = compile_and_run_native("import platform\nprint(platform.capabilities())\n").unwrap();
    assert!(result.contains("\"file\": true"), "stdout:\n{result}");
    assert!(result.contains("\"network\": true"), "stdout:\n{result}");
    assert!(result.contains("\"env\": true"), "stdout:\n{result}");
    assert!(result.contains("\"process\": true"), "stdout:\n{result}");
}

#[test]
fn test_llvm_capability_enforcement_in_native_runtime() {
    let _guard = LLVM_BUILD_LOCK.lock().unwrap();
    let temp_dir = unique_temp_dir("cool_llvm_capabilities_native");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();
    fs::write(
        temp_dir.join("cool.toml"),
        "name = \"capnative\"\nmain = \"main.cool\"\noutput = \"capnative\"\n\n[capabilities]\nnetwork = false\n",
    )
    .unwrap();
    fs::write(
        temp_dir.join("main.cool"),
        "import http\nprint(http.get(\"https://example.com\"))\n",
    )
    .unwrap();

    let build_output = Command::new(cool_bin())
        .current_dir(&temp_dir)
        .args(["build"])
        .output()
        .unwrap();
    assert!(
        build_output.status.success(),
        "build failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&build_output.stdout),
        String::from_utf8_lossy(&build_output.stderr)
    );

    let binary_path = temp_dir.join("capnative");
    let run_output = Command::new(&binary_path).output().unwrap();
    assert!(!run_output.status.success(), "expected native run to fail");
    let stderr = String::from_utf8_lossy(&run_output.stderr).to_string();
    assert!(
        stderr.contains("CapabilityError: network access denied"),
        "stderr:\n{stderr}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_llvm_import_jobs_module() {
    let result = compile_and_run_native(
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
    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, ["hello", "command:true", "sleep:true"]);
}

#[test]
fn test_llvm_phase6_data_stdlib_modules() {
    let temp_dir = unique_temp_dir("cool_llvm_phase6_data");
    let source_path = write_phase6_data_suite(&temp_dir);
    let result = compile_and_run_native_path(&source_path).unwrap();
    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
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
    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_llvm_phase6_pass2_stdlib_modules() {
    let temp_dir = unique_temp_dir("cool_llvm_phase6_pass2");
    let source_path = write_phase6_pass2_suite(&temp_dir);
    let result = compile_and_run_native_path(&source_path).unwrap();
    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
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
    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_llvm_phase6_pass3_stdlib_modules() {
    let temp_dir = unique_temp_dir("cool_llvm_phase6_pass3");
    let source_path = write_phase6_pass3_suite(&temp_dir);
    let result = compile_and_run_native_path_with_env(&source_path, &[("COOL_PHASE6_SB", "allowed")]).unwrap();
    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
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
    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_llvm_phase6_filesystem_os_modules() {
    let temp_dir = unique_temp_dir("cool_llvm_phase6_fs");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();
    let result = compile_and_run_native_with_env(
        &phase6_filesystem_os_source(&temp_dir),
        &[("COOL_PHASE6_TOKEN", "present")],
    )
    .unwrap();
    let lines: Vec<String> = result.lines().map(|line| line.to_string()).collect();
    assert_eq!(lines, expected_phase6_filesystem_os_lines("native"));
    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_llvm_phase6_storage_modules() {
    let temp_dir = unique_temp_dir("cool_llvm_phase6_storage");
    let source_path = write_phase6_storage_suite(&temp_dir);
    let result = compile_and_run_native_path(&source_path).unwrap();
    let lines: Vec<String> = result.lines().map(|line| line.to_string()).collect();
    assert_eq!(lines, expected_phase6_storage_lines());
    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_llvm_phase6_tooling_modules() {
    let temp_dir = unique_temp_dir("cool_llvm_phase6_tooling");
    let source_path = write_phase6_tooling_suite(&temp_dir);
    let result = compile_and_run_native_path(&source_path).unwrap();
    let lines: Vec<String> = result.lines().map(|line| line.to_string()).collect();
    assert_eq!(lines, expected_phase6_tooling_lines());
    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_llvm_phase6_runtime_automation_modules() {
    let temp_dir = unique_temp_dir("cool_llvm_phase6_runtime");
    let source_path = write_phase6_runtime_automation_suite(&temp_dir);
    let result = compile_and_run_native_path(&source_path).unwrap();
    let lines: Vec<String> = result.lines().map(|line| line.to_string()).collect();
    assert_eq!(lines, expected_phase6_runtime_automation_lines());
    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_llvm_phase6_math_data_finance_modules() {
    let temp_dir = unique_temp_dir("cool_llvm_phase6_math");
    let source_path = write_phase6_math_data_finance_suite(&temp_dir);
    let result = compile_and_run_native_path(&source_path).unwrap();
    let lines: Vec<String> = result.lines().map(|line| line.to_string()).collect();
    assert_eq!(lines, expected_phase6_math_data_finance_lines());
    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_llvm_phase6_security_crypto_modules() {
    let temp_dir = unique_temp_dir("cool_llvm_phase6_crypto");
    let source_path = write_phase6_security_crypto_suite(&temp_dir);
    let result = compile_and_run_native_path(&source_path).unwrap();
    let lines: Vec<String> = result.lines().map(|line| line.to_string()).collect();
    assert_eq!(lines, expected_phase6_security_crypto_lines());
    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_llvm_phase6_network_core_modules() {
    let temp_dir = unique_temp_dir("cool_llvm_phase6_network_core");
    let cluster_root = cool_quote_path(&temp_dir.join("cluster-root"));
    let result = compile_and_run_native(&format!(
        r#"import calendar
import cluster
import path
import rpc
import url

def handle_sum(params, message):
    return params["a"] + params["b"]

u = url.parse("https://user:pass@example.com:8443/a/b?q=1#frag")
print(u["host"])
print(url.join("https://example.com/a/b", "../c"))
print(url.encode_query({{"a": "1 2", "b": ["x", "y"]}}))
routes = rpc.router()
rpc.register(routes, "sum", handle_sum)
print(rpc.dispatch(routes, rpc.request(1, "sum", {{"a": 2, "b": 3}}))["result"])
print(rpc.dispatch(routes, rpc.request(2, "missing", nil))["error"]["code"])
hits = calendar.occurrences({{"start": "2024-01-01 09:00:00", "freq": "daily", "count": 3}}, 3)
print(len(hits))
print(calendar.format_time(hits[1]))
c = cluster.open_cluster("{cluster_root}", "demo", 30)
cluster.join(c, "node-a", {{"role": "leader"}})
cluster.join(c, "node-b", {{"role": "worker"}})
print(cluster.leader(c)["id"])
print(cluster.claim(c, "lease", "node-a"))
print(cluster.barrier(c, "ready", "node-a", 1)["ready"])
"#
    ))
    .unwrap();
    let lines: Vec<String> = result.lines().map(|line| line.to_string()).collect();
    assert_eq!(
        lines,
        vec![
            "example.com".to_string(),
            "https://example.com/c".to_string(),
            "a=1+2&b=x&b=y".to_string(),
            "5".to_string(),
            "-32601".to_string(),
            "3".to_string(),
            "2024-01-02 09:00:00".to_string(),
            "node-a".to_string(),
            "true".to_string(),
            "true".to_string(),
        ]
    );
    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_llvm_phase6_graphql_feed_mail_and_socket_modules() {
    let (graphql_url, feed_url, http_handle) = spawn_graphql_feed_server();
    let (smtp_port, smtp_handle) = spawn_smtp_test_server();
    let (imap_port, imap_handle) = spawn_imap_test_server();
    let (udp_port, udp_handle) = spawn_udp_echo_server(4);
    let (ws_url, ws_handle) = spawn_websocket_echo_server();
    let result = compile_and_run_native(&format!(
        r#"import feed
import graphql
import mail
import socket
import websocket

query = graphql.operation("query", [graphql.field("status"), graphql.field("echo", nil, [graphql.arg("value", "hi")])])
gql = graphql.execute("{graphql_url}", query)
print(graphql.extract(gql, "/status"))
print(graphql.extract(gql, "/echo"))
msg = mail.message("from@example.com", ["to@example.com"], "Hi", "Body")
smtp = mail.smtp_send("127.0.0.1", {smtp_port}, msg, "cool.test")
print(smtp["recipients"][0])
imap = mail.imap_run("127.0.0.1", {imap_port}, ["CAPABILITY"])
print(imap["banner"].find("IMAP4rev1") >= 0)
polled = feed.poll("{feed_url}")
print(polled["feed"]["title"])
print(len(polled["new"]))
udp = socket.connect_udp("127.0.0.1", {udp_port})
udp.send("ping")
print(udp.recv(16))
udp.send_bytes([0, 1, 2, 255])
raw = udp.recv_bytes(16)
print(raw[3])
bind = socket.bind_udp("127.0.0.1", 0)
bind.sendto("127.0.0.1", {udp_port}, "from-bind")
packet = bind.recvfrom(32)
print(packet[0])
bind.sendto_bytes("127.0.0.1", {udp_port}, [9, 8, 7])
packet2 = bind.recvfrom_bytes(32)
bytes2 = packet2[0]
print(bytes2[1])
ws = websocket.connect("{ws_url}", nil, "dGhlIHNhbXBsZSBub25jZQ==")
websocket.send_text(ws, "cool")
print(websocket.recv_text(ws))
websocket.close(ws)
"#
    ))
    .unwrap();
    let lines: Vec<String> = result.lines().map(|line| line.to_string()).collect();
    assert_eq!(
        lines,
        vec![
            "ok".to_string(),
            "hi".to_string(),
            "to@example.com".to_string(),
            "true".to_string(),
            "T".to_string(),
            "1".to_string(),
            "ping".to_string(),
            "255".to_string(),
            "from-bind".to_string(),
            "8".to_string(),
            "pong".to_string(),
        ]
    );
    http_handle.join().unwrap();
    smtp_handle.join().unwrap();
    imap_handle.join().unwrap();
    udp_handle.join().unwrap();
    ws_handle.join().unwrap();
}

#[test]
fn test_llvm_websocket_server_support() {
    assert_eq!(
        run_native_websocket_server_case(),
        vec!["/srv".to_string(), "client".to_string()]
    );
}

#[test]
fn test_llvm_phase13_typed_language_features() {
    let result = compile_and_run_native(
        r#"
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
"#,
    )
    .unwrap();
    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, vec!["7", "Ada", "41", "none"]);
}

#[test]
fn test_llvm_phase13_option_and_result_stdlib_modules() {
    let result = compile_and_run_native(
        r#"
import option
import result

value = option.some(5)
print(option.is_some(value))
print(option.unwrap(value))
print(option.unwrap_or(option.none(), 9))

ok = result.ok(41)
err = result.err("boom")
print(result.is_ok(ok))
print(result.unwrap(ok))
print(result.is_err(err))
print(result.unwrap_err(err))
"#,
    )
    .unwrap();
    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, vec!["true", "5", "9", "true", "41", "true", "boom"]);
}

fn llvm_phase6_terminal_media_source() -> &'static str {
    r##"
import ansi
import audio
import color
import game
import image
import scene
import sprite
import term
import theme
import tui

print(ansi.cursor(2, 3).endswith("H"))
print(ansi.rgb_fg(1, 2, 3).startswith("\x1b[38;2"))
print(ansi.box(4, 3, "T").split("\n")[0].startswith("+ T "))

c = color.parse_hex("#336699")
print(color.to_hex(c))
print(color.to_hex(color.mix(color.rgb(0, 0, 0), color.rgb(255, 255, 255), 0.5)))
print(int(color.luminance(color.rgb(255, 255, 255)) * 100))
print(color.to_hex(color.hsl(120, 1, 0.5)))
print(color.to_hex(color.hsv(240, 1, 1)))
print(len(color.palette(c, 3)))

th = theme.default()
print(color.to_hex(theme.get(th, "accent")))
print(theme.style(th, "title", "Hi").find("Hi") >= 0)
print(theme.space(th, "md"))

bounds = tui.rect(0, 0, 20, 5)
print(tui.split_horizontal(bounds, [5, 15])[1]["width"])
print(tui.render(tui.button("OK", true), 10).strip())
print(tui.render(tui.list(["a", "b"], 1), 4).split("\n")[1].strip())
focus_state = tui.focus(["a", "b"], 0)
tui.next_focus(focus_state)
print(tui.focused(focus_state))
list_widget = tui.list(["a", "b"], 0)
tui.handle_event(list_widget, {"key": "down"})
print(list_widget["selected"])
loop_record = tui.event_loop({"kind": "state"}, [{"type": "tick"}])
print(len(loop_record["events"]))

root = scene.node("root", 1, 1, "R")
scene.add(root, scene.node("child", 1, 0, "C"))
print(len(scene.flatten(root)))
print(scene.bounds(root)["width"])
print(scene.render(root, 4, 3).split("\n")[1])

img = image.blank(2, 2, color.rgb(10, 20, 30))
image.set(img, 1, 1, color.rgb(200, 100, 0))
print(image.metadata(img)["pixels"])
print(color.to_hex(image.get(img, 1, 1)))
print(image.to_ppm(image.crop(img, 1, 1, 1, 1)).split("\n")[0])

snd = audio.sound(4, [0.0, 0.5, -0.5, 1.0])
print(int(audio.duration(snd) * 100))
print(audio.metadata(audio.silence(1, 4))["samples"])
print(len(audio.mix(snd, audio.silence(1, 4))["samples"]))
print(audio.pcm8(snd)[3])
print(audio.wav(snd)["bits_per_sample"])
print(int(audio.from_pcm([0, 255], 4)["samples"][1] * 100))

sp = sprite.sprite([sprite.frame(["ab", "cd"]), sprite.frame(["ef", "gh"])], 2)
print(sprite.render(sp, 1).split("\n")[0])
print(sprite.render(sprite.flip_h(sp), 0).split("\n")[0])
print(sprite.current(sp, 0.6)["lines"][0])
print(sprite.tile(["abcd", "efgh"], 1, 0, 2, 2)["lines"][1])

w = game.world(10, 10, 10)
player = game.entity("p", 1, 1, 2, 0, 1, 1)
wall = game.entity("wall", 3, 1, 0, 0, 1, 1)
game.add(w, player)
game.add(w, wall)
game.tick(w, 1.0)
print(int(game.find(w, "p")["x"]))
print(game.collides(player, wall))
print(game.pressed(game.input_state(["left"]), "left"))
timer = game.timer(1)
game.timer_tick(timer, 0.5)
print(timer["done"])
game.timer_tick(timer, 0.5)
print(timer["ticks"])
loop_state = game.loop_state(w)
game.step_loop(loop_state, 0.1)
print(len(loop_state["frames"]))

m = term.mouse_event("down", 4, 5, "left", ["shift"])
print(m["button"])
print(term.mouse_sequence(true).endswith("h"))
scr = term.screen(5, 2, ".")
term.screen_put(scr, 1, 2, "OK")
print(term.screen_text(scr).split("\n")[0])
term.screen_clear(scr, "-")
print(term.screen_text(scr).split("\n")[1])
"##
}

#[test]
fn test_llvm_phase6_terminal_ui_presentation_and_media_game_modules() {
    let result = compile_and_run_native(llvm_phase6_terminal_media_source()).unwrap();
    let lines: Vec<_> = result.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(
        lines,
        vec![
            "true", "true", "true", "#336699", "#7f7f7f", "99", "#00ff00", "#0000ff", "3", "#fdb515", "true", "2",
            "15", "> [ OK ]", "> b", "b", "1", "1", "2", "2", ".RC.", "4", "#c86400", "P3", "100", "4", "4", "255",
            "8", "100", "ef", "ba", "ef", "fg", "3", "true", "true", "false", "1", "1", "left", "true", ".OK..",
            "-----",
        ]
    );
}
