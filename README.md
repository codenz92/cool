# Cool

A native-first, high-level systems language with Python-like syntax, an LLVM compiler, FFI, and freestanding build support — all implemented in Rust.

Cool is a high-level systems language with Python-like syntax and a native-first toolchain. Its primary compilation path targets native binaries, freestanding objects, static libraries, and shared libraries through LLVM, while the tree-walk interpreter and bytecode VM provide fast iteration, tooling support, and runtime parity. It also ships with a **foreign function interface**, explicit ABI/data-layout features, and bundled terminal apps written in Cool itself.

---

## Features

### Language

- Native-first toolchain: tree-walk interpreter, bytecode VM, LLVM native compiler, freestanding object output, static/shared library output, and kernel image linking via LLD
- Systems reach: `extern def`, `data`, structs/unions, FFI, inline assembly, raw memory operations, MMIO/register helpers, and no-libc syscall paths
- Indentation-based block syntax (Python-style)
- Variables, arithmetic, comparisons, logical and bitwise operators
- `if` / `elif` / `else`, `while`, `for`, `break`, `continue`
- Functions with default args, `*args`, `**kwargs`, keyword args, closures
- Typed local bindings (`name: Type = expr`) and immutable constants (`const NAME: Type = expr`)
- Enums/tagged unions with `match`, traits via `trait` / `implements`, generic `def` / `struct` / `union`, trait bounds, and typed collection surfaces like `list[T]`, `dict[K, V]`, and `tuple[T, U]`
- Classes with inheritance, `super()`, operator overloading (`__add__`, `__str__`, `__eq__`, `__len__`, etc.)
- Lists, dicts, tuples with full method support
- `set()` built-in (returns deduplicated list)
- Slicing (`lst[1:3]`, negative indices)
- `try` / `except` / `else` / `finally`, `raise`
- f-strings, multi-line strings, `string.format()`
- List comprehensions, lambda expressions, ternary expressions
- `nonlocal` / `global`, `assert`, `with` / context managers
- `import math`, `import os`, `import sys`, `import path`, `import platform`, `import core`, `import csv`, `import datetime`, `import hashlib`, `import toml`, `import yaml`, `import sqlite`, `import http`, `import argparse`, `import logging`, `import test`
- `import string`, `import list`, `import json`, `import re`, `import time`, `import random`, `import collections`, `import subprocess`, `import socket`, `import url`, `import websocket`, `import rpc`, `import graphql`, `import mail`, `import feed`, `import calendar`, `import cluster`, `import glob`, `import tempfile`, `import process`, `import fswatch`, `import daemon`, `import sandbox`, `import sync`, and `import store` (`http` requires host `curl`)
- `import ffi` — call C functions from shared libraries at runtime
- Project-level capability policy via `[capabilities]` in `cool.toml` for file, network, env, and process access
- LLVM-native `extern def` declarations with symbol/link/ownership metadata
- LLVM-native ordinary `def` signatures with typed parameters and return types
- Explicit module export control with `public` / `private` on top-level bindings and declarations
- Build profiles for native workflows: `cool build --profile dev|release|freestanding|strict`
- Explicit target triples and CPU tuning for native builds: `cool build --target <triple> --cpu <name> --cpu-features <spec>`
- Incremental native build caching plus reproducible/toolchain-pinned builds via `[build]` and `[toolchain]`
- Native stack traces on unhandled exceptions, `cool bench --profile`, and first-class `cool fmt`
- `cool build --freestanding` / `--no-libc` for host-free outputs, `--entry <symbol>` for explicit entry control, and `--linker-script=<path>` to link a kernel image (`.elf`) via LLD
- `cool bindgen` for C headers and `cool layout` for archive/object/kernel/shared-library inspection
- `cool new --template app|lib|service|freestanding` for app, library, network-service, and freestanding scaffolds
- `cool publish`, `cool install --locked`, and lockfile checksums for source-distribution packaging and reproducible dependency installs
- `cool pkg` for Cool-native project/package inspection and dependency diagnostics
- `import jobs` for structured concurrency: tasks, channels, deadlines, cancellation, and process/network orchestration
- Bundled typed error helpers: `import option` and `import result`
- Bundled data/text modules: `import bytes`, `import base64`, `import codec`, `import html`, `import xml`, `import unicode`, `import locale`, `import config`, and `import schema`
- Bundled filesystem/OS modules: `import glob`, `import tempfile`, `import process`, `import fswatch`, `import daemon`, `import sandbox`, `import sync`, and `import store`
- Bundled networking/service modules: `import url`, `import websocket`, `import rpc`, `import graphql`, `import mail`, `import feed`, `import calendar`, and `import cluster`
- Bundled storage/package modules: `import cache`, `import memo`, `import package`, `import compress`, `import archive`, and `import bundle`
- Bundled parsing/tooling modules: `import doc`, `import template`, `import lexer`, `import parser`, `import ast`, `import inspect`, `import diff`, `import patch`, `import project`, `import release`, `import repo`, `import modulegraph`, `import plugin`, `import lsp`, `import ffiutil`, and `import shell`
- Bundled runtime/automation/observability modules: `import event`, `import workflow`, `import agent`, `import retry`, `import metrics`, `import trace`, `import profile`, `import bench`, `import notebook`, and `import secrets`
- Bundled math/data-science/finance modules: `import decimal`, `import money`, `import stats`, `import vector`, `import matrix`, `import geom`, `import graph`, `import tree`, `import pipeline`, `import stream`, `import table`, `import search`, `import embed`, and `import ml`
- Bundled security/crypto modules: `import crypto` plus built-in `import hashlib`
- Bundled terminal/UI modules: `import ansi`, `import color`, `import theme`, `import tui`, and `import scene` plus built-in `import term`
- Bundled media/game modules: `import image`, `import audio`, `import sprite`, and `import game`
- x86 port I/O primitives: `outb(port, byte)`, `inb(port)`, `write_serial_byte(byte)` — bare-metal serial output with no C runtime dependency
- Package system: `import foo.bar` loads `foo/bar.cool`
- File I/O via `open()`, `read()`, `read_bytes()`, `write()`, `write_bytes()`, and `readlines()`
- Explicit runtime helpers: `copy()`, `clone()`, `close()`, `panic()`, and `abort()`
- `runfile()` to execute another `.cool` file at runtime
- `eval(str)` to evaluate a Cool expression or statement at runtime
- `import term` for raw terminal mode, cursor control, terminal sizing, key input, mouse event records, mouse tracking escape toggles, and deterministic screen buffers across interpreter, VM, and native builds (real TTY required for interactive input)
- `os.popen(cmd)` to run shell commands and capture output
- Integer width helpers: `i8`, `u8`, `i16`, `u16`, `i32`, `u32`, `i64`, plus pointer-width `isize`, `usize`, `word_bits()`, and `word_bytes()`
- Runtime-model helpers via `import std.memory` and `import std.runtime`
- Bundled data modules: `import bytes`, `import base64`, `import codec`, `import html`, `import config`, and `import schema`
- `struct` definitions with typed fields and positional/keyword construction across all runtimes
- `packed struct` — no inter-field padding, stable binary layout in LLVM
- Hex / binary / octal literals, `\x` escape sequences
- REPL mode

### Built-in Functions

`print()`, `input()`, `str()`, `int()`, `float()`, `bool()`, `len()`, `range()`, `type()`, `repr()`, `ord()`, `chr()`, `abs()`, `min()`, `max()`, `sum()`, `any()`, `all()`, `round()`, `sorted()`, `reversed()`, `enumerate()`, `zip()`, `map()`, `filter()`, `list()`, `tuple()`, `dict()`, `set()`, `isinstance()`, `hasattr()`, `getattr()`, `copy()`, `clone()`, `close()`, `panic()`, `abort()`, `isize()`, `usize()`, `word_bits()`, `word_bytes()`, `assert`, `exit()`

### String Methods

`.upper()`, `.lower()`, `.strip()`, `.lstrip()`, `.rstrip()`, `.split()`, `.join()`, `.replace()`, `.find()`, `.count()`, `.startswith()`, `.endswith()`, `.title()`, `.capitalize()`, `.format()`

### Platform Module

Inspect host OS/architecture details and runtime capabilities from Cool:

```python
import platform

print(platform.os())              # "macos", "linux", "windows", ...
print(platform.arch())            # "x86_64", "aarch64", ...
print(platform.family())          # "unix" or "windows"
print(platform.runtime())         # "interpreter", "vm", or "native"
print(platform.runtime_profile()) # "hosted" or "freestanding"
print(platform.shared_lib_ext())  # "dylib", "so", or "dll"
print(platform.has_ffi())         # runtime capability flags
print(platform.has_raw_memory())
print(platform.capabilities())    # {"file": true, "network": true, ...}
print(platform.memory_model()["raw_memory"])
print(platform.panic_policy()["stack_trace"])
```

### Capability Model

Projects can declare runtime permissions in `cool.toml`:

```toml
[capabilities]
file = true
network = false
env = true
process = false
```

The interpreter, bytecode VM, and native runtime enforce the same policy for `open()`, `import os`, `import http`, `import socket`, and `import subprocess`. Denied operations raise an explicit capability error, and `platform.capabilities()` reports the active manifest policy back to Cool code.

### Runtime Model

Phase 14 adds a user-visible runtime/memory surface:

```python
import std.memory
import std.runtime

scope = std.memory.Scope()
file = scope.track(open("demo.txt", "w"))
file.write("hello")
close(scope)  # deterministic cleanup beyond `with`

print(std.runtime.runtime_profile())      # hosted / freestanding
print(std.runtime.memory_model()["copy"]) # shallow-copy policy
print(std.runtime.thread_safety()["mode"])
```

`copy()` / `clone()` are shallow across containers and instances; `std.memory.deep_clone()` recursively clones list/dict trees. `std.memory.Arena` layers explicit `core.alloc()` / `core.free()` ownership onto hosted native builds when raw memory is available. `panic()` is a fatal diagnostic path, while `abort()` is an immediate fatal termination path.

### Data Modules

Bundled data/serialization helpers now live under `stdlib/` and import cleanly in all three runtimes:

```python
import bytes
import base64
import config
import html
import schema

blob = bytes.from_string("A🙂")
print(bytes.hex(blob))                       # 41f09f9982
print(base64.encode(blob))                   # QfCfmYI=
print(config.load("settings.env")["HELLO"])  # format inference

rule = schema.shape({"name": schema.string({"min": 1})})
print(schema.check(rule, {"name": "Ada"}))   # true
print(html.extract_title("<title>Hi</title>"))
```

Package/bundle metadata and `cool pkg capabilities` expose the same permission set so projects can audit what an app or dependency expects before running it.

### Filesystem Modules

Bundled filesystem/process helpers cover discovery, temp paths, runtime metadata, file/state reconciliation, persistent key-value state, constrained automation, and polling-based file watching:

```python
import daemon
import fswatch
import glob
import process
import sandbox
import store
import sync
import tempfile

matches = glob.glob("**/*.cool", ".")
tmp = tempfile.named_dir("demo-")
print(tmp.path)
print(process.pid(), process.runtime())

db = store.open_store(".cool/state")
prefs = db.namespace("prefs")
prefs.set("theme", "amber")
print(prefs.get("theme"))

sb = sandbox.open_sandbox({
    "root": ".",
    "process": true,
    "commands": ["printf"],
})
print(sb.check_output("printf ok"))

before = fswatch.snapshot("src")
# ... edit files ...
after = fswatch.snapshot("src")
print(fswatch.changed_paths(fswatch.diff(before, after)))

plan = sync.reconcile("src", "backup", ".cool/sync.json")
print(len(plan["conflicts"]))

svc = daemon.service("worker", {"command": "printf ready"})
svc.start()
svc.wait(1.0)
print(svc.status()["exit_code"])
close(tmp)
```

### Networking Modules

Bundled networking/service helpers now cover URL parsing, TCP/UDP sockets, websockets, RPC-style routing, GraphQL, mail workflows, feeds, recurring calendars, and simple cluster coordination:

```python
import calendar
import graphql
import rpc
import socket
import url
import websocket

def handle_sum(params, message):
    return params["a"] + params["b"]

parsed = url.parse("ws://127.0.0.1:9000/chat?room=ops")
print(parsed["scheme"], parsed["host"], parsed["path"])

udp = socket.connect_udp("127.0.0.1", 9001)
udp.send_bytes([1, 2, 3])
print(udp.recv_bytes(16))

routes = rpc.router()
rpc.register(routes, "sum", handle_sum)
reply = rpc.dispatch(routes, rpc.request(1, "sum", {"a": 2, "b": 3}))
print(reply["result"])

query = graphql.operation("query", [graphql.field("status")])
print(graphql.render(query))

req = websocket.request("ws://127.0.0.1:9000/chat")
print(req["headers"]["Sec-WebSocket-Key"])

rule = {"start": "2024-01-01 09:00:00", "freq": "daily", "count": 3}
print(calendar.format_time(calendar.occurrences(rule, 3)[1]))
```

### Storage Modules

Bundled storage/package helpers now cover TTL caches, memoized work, semver/project metadata, gzip/tar/zip payloads, higher-level archives, and single-file app bundles:

```python
import archive
import bundle
import cache
import memo
import package

def build_answer():
    return {"value": 42}

def add_pair(left, right):
    return left + right

mem = cache.memory()
print(cache.remember(mem, "answer", build_answer)["value"])

table = memo.memory("math")
print(memo.call(table, "add_pair", add_pair, [2, 3]))

info = package.project_info(".")
print(info["name"], info["version"])

archive.create("assets", "dist/assets.tar.gz")
print(archive.list("dist/assets.tar.gz")[0])

print(bundle.read_manifest("dist/app.coolbundle")["package"]["name"])
```

### Tooling Modules

Bundled parsing/language/tooling helpers now cover documentation rendering, small templates, token streams, source summaries, diffs and patches, project/release metadata, repository status parsing, import graphs, plugin descriptors, LSP messages, FFI wrapper text, and shell syntax:

```python
import ast
import diff
import doc
import lsp
import modulegraph
import shell
import template

print(doc.heading("Cool", 2))
print(template.render("Hello {{ name }}", {"name": "Ada"}))
print(ast.summary("import util\nVALUE = 1\n")["counts"]["binding"])
print(diff.stats(diff.compare("a", "b"))["insert"])
print(modulegraph.dot(modulegraph.graph({"main": "import util\n"})))
print(lsp.encode(lsp.response(1, {"ok": true})).startswith("Content-Length: "))
print(shell.split("cool run app.cool")[0])
```

### Jobs Module

`import jobs` provides a small structured-concurrency layer that works across interpreter, VM, and native builds:

```python
import jobs

g = jobs.group("checks")
ch = jobs.channel(g)
jobs.send(ch, "ready")
print(jobs.recv(ch))

jobs.command(g, "printf ok", 2.0, "smoke")
jobs.sleep(g, 0.05, "tick")

for result in jobs.await_all(g):
    print(result["name"], result["ok"], result["duration"])
```

The module includes groups, deadlines, cancellation, channels, background command tasks, HTTP tasks, and polling helpers. The bundled `cool pkg`, `pulse`, and `control` tools all use this layer directly.

### Runtime Automation Modules

Bundled automation and observability helpers now cover pub/sub events, resumable workflows, agent-style plans, retries, counters/gauges/histograms, traces, profiler summaries, benchmark records, executable notebooks, and redacted/encrypted secret storage:

```python
import event
import metrics
import retry
import trace
import workflow

bus = event.bus("build")
event.emit(bus, "build.done", {"ok": true})
print(event.drain(bus)[0]["topic"])

wf = workflow.workflow("release")
workflow.add(wf, workflow.step("test"))
workflow.complete(wf, "test", "ok")
print(workflow.status(wf)["done"])

r = metrics.registry("service")
metrics.inc(r, "requests", 2)
metrics.observe(r, "latency", 0.25)
print(metrics.snapshot(r)["histograms"]["latency"]["count"])

tr = trace.tracer("request")
span = trace.start_span(tr, "handler")
trace.finish_span(tr, span)
print(len(trace.export(tr)))

print(retry.should_retry(retry.policy(3), retry.failure(1, "timeout")))
```

### Math And Data Modules

Bundled math, data-science, and finance helpers cover exact decimal and money values, descriptive statistics, vectors/matrices/geometry, graph/tree traversal, pipelines/streams, tables, local search, bag-of-words embeddings, and lightweight ML helpers:

```python
import decimal
import graph
import money
import stats
import table
import vector

total = money.add(money.amount("12.34", "USD"), money.amount("0.66", "USD"))
print(money.format(total, "$"))

print(decimal.format(decimal.div(decimal.parse("1"), decimal.parse("4"), 2)))
print(stats.describe([1, 2, 3, 4])["median"])
print(vector.dot(vector.vector([3, 4]), [1, 2]))

g = graph.graph(true)
graph.add_edge(g, "a", "b")
graph.add_edge(g, "b", "c")
print(graph.shortest_path(g, "a", "c"))

t = table.table([{"name": "Ada", "score": 2}], ["name", "score"])
print(table.render(t))
```

### Security Modules

`import crypto` builds on `hashlib`, `bytes`, and `base64` to provide key derivation, random byte/token helpers, HMAC-style signatures, constant-time comparison, and authenticated symmetric envelopes:

```python
import crypto

key = crypto.derive_key("password", "salt", 1000, 32)
sig = crypto.sign("payload", key)
print(crypto.verify("payload", sig, key))

box = crypto.encrypt("secret", key)
print(crypto.decrypt(box, key))
print(crypto.token_hex(16))
```

### Terminal And UI Modules

Terminal helpers now cover ANSI escape generation, RGB/HSL/HSV color utilities, theme palettes/spacing, deterministic TUI rendering and event state, ASCII scene graphs, and the built-in `term` runtime surface for raw mode, cursor/key/mouse helpers, and mutable screen buffers:

```python
import ansi
import color
import term
import theme
import tui

print(ansi.style("Ready", [1, 38, 5, 46]))
print(color.to_hex(color.mix(color.rgb(0, 0, 0), color.rgb(255, 255, 255))))

screen = term.screen(20, 3, ".")
term.screen_put(screen, 1, 2, "Cool")
print(term.screen_text(screen))

print(tui.render(tui.button("Run", true), 12))
focus = tui.focus(["menu", "body"], 0)
tui.next_focus(focus)
print(tui.focused(focus))
print(theme.style(theme.default(), "title", "Dashboard"))
```

### Media And Game Modules

Small in-memory media/game helpers provide image buffers, PCM/WAV-style audio records, ASCII sprite tiles/animation, timers, and entity/world/collision primitives without external device or codec dependencies:

```python
import audio
import game
import image
import sprite

img = image.blank(2, 2, {"r": 10, "g": 20, "b": 30, "a": 255})
image.set(img, 1, 1, {"r": 200, "g": 100, "b": 0, "a": 255})
print(image.metadata(img)["pixels"])

snd = audio.square(440, 0.1, 8000)
print(audio.wav(audio.normalize(snd))["bits_per_sample"])

hero = sprite.sprite([sprite.frame(["@."])], 1)
print(sprite.render(hero))
print(sprite.tile(["abcd", "efgh"], 1, 0, 2, 2)["lines"][1])

world = game.world(20, 10, 60)
game.add(world, game.entity("player", 1, 1, 1, 0))
game.tick(world, 0.5)
print(game.snapshot(world)["frame"])
```

### Core Module

Use host-free word-size, page, and paging-index helpers from any runtime:

```python
import core

addr = 74565
print(core.word_bits())
print(core.word_bytes())
print(core.page_size())        # 4096
print(core.page_align_down(addr))
print(core.page_align_up(addr))
print(core.page_offset(addr))
print(core.page_count(8193))   # 3
print(core.pt_index(addr))
print(core.pd_index(addr))
print(core.pdpt_index(addr))
print(core.pml4_index(addr))
```

`core.alloc()`, `core.free()`, `core.set_allocator()`, and `core.clear_allocator()` are LLVM-native allocator hooks for hosted/freestanding builds. The interpreter and bytecode VM recognize those names but intentionally report `compile with cool build`.

Phase 16 extends `import core` with pointer/address helpers, string/formatting helpers, lightweight collection constructors, and low-level register/syscall primitives:

```python
import core

ptr = core.addr(4099)
print(core.addr_align_up(ptr, 16))
print(core.format_hex(255))        # 0xff
print(core.format_bin(10))         # 0b1010
print(core.format_ptr(ptr))

items = core.list_new(2)
core.list_push(items, "cool")
print(core.list_pop(items))

flags_addr = 0x1000
core.reg_set_bits(flags_addr, "u32", 0x10)   # LLVM-only
```

`core.mmio_*`, `core.reg_*`, and `core.syscall*` are LLVM-only. `core.syscall*` currently targets Linux x86_64 / aarch64 no-libc or freestanding builds.

### FFI — Call C from Cool

Load any shared library and call its functions directly:

```python
import ffi

libm = ffi.open("libm")
sin_fn  = ffi.func(libm, "sin",  "f64", ["f64"])
pow_fn  = ffi.func(libm, "pow",  "f64", ["f64", "f64"])

print(sin_fn(3.14159 / 2.0))   # 1.0
print(pow_fn(2.0, 10.0))       # 1024.0
```

Supported types: `"void"`, `"i8"`–`"i64"`, `"u8"`–`"u64"`, `"isize"`, `"usize"`, `"f32"`, `"f64"`, `"str"`, `"ptr"`.

### Typed Function Signatures (LLVM backend)

Top-level `def` can use the same ABI type names as `extern def` when you compile with `cool build`:

```python
extern def c_strlen(text: str) -> usize:
    symbol: "strlen"
    cc: "c"

def add(x: i32, y: i32) -> i32:
    return x + y

def len_plus(text: str, extra: i32) -> i32:
    return c_strlen(text) + extra

def log_value(value: i32) -> void:
    print(value)
    return

print(add(40, 2))
print(len_plus("cool", 3))
f = add
print(f(7, 8))
log_value(11)
```

In native builds, annotated parameters and return types lower to real native LLVM types, while unannotated parameters still use the normal dynamic `CoolVal` calling convention. The tree-walk interpreter and bytecode VM accept this syntax too, but currently ignore the annotations and execute the functions dynamically.

### Typed Bindings And Module Visibility

Cool also supports typed bindings for ordinary code plus explicit module export visibility:

```python
public const VERSION: str = "1.0"
private const SECRET: str = "hidden"

count: i32 = 3

def next_count(value: i32) -> i32:
    local: i32 = value + count
    return local
```

`cool check` validates typed bindings, immutable reassignments, missing returns on typed functions, non-exhaustive local-enum `match` blocks, trait implementations, generic trait bounds, and private/exported module surfaces. `import "helper.cool"` only flattens public exports, and `import helper` only exposes public names on the module namespace.

### Enums, Traits, Generics, And `match`

Cool now has a typed-language layer that stays aligned across interpreter, VM, and native builds:

```python
trait Named:
    def name(self) -> str

class User implements Named:
    def __init__(self, value: str):
        self.value = value

    def name(self) -> str:
        return self.value

enum Option[T]:
    Some(value: T)
    None

def first[T](items: list[T]) -> Option[T]:
    if len(items) > 0:
        return Option.Some(items[0])
    return Option.None

def show_name[T: Named](item: T) -> str:
    return item.name()

match first([1, 2, 3]):
    Option.Some(value):
        print(value)
    Option.None:
        print("empty")

print(show_name(User("Ada")))
```

Generic structs use the same surface:

```python
struct Box[T]:
    value: T

box: Box[int] = Box(41)
print(box.value)
```

For application-style error handling, the bundled `option` and `result` modules provide reusable helpers:

```python
import option
import result

value = option.some(41)
print(option.unwrap_or(option.none(), 0))

outcome = result.ok("done")
print(result.unwrap(outcome))
```

### Extern Declarations (LLVM backend)

Declare linked external symbols directly in Cool:

```python
extern def abs(x: i32) -> i32

extern def c_strlen(text: str) -> usize:
    symbol: "strlen"
    cc: "c"
    library: "c"
    link_kind: "shared"
    ownership: "borrowed"
    lifetime: "caller"
    section: ".text.host"

print(abs(-42))
print(c_strlen("hello"))
```

`extern def` is only available in compiled programs (`cool build`). It uses the same ABI type names as `ffi.func`, with optional `symbol:` / `library:` / `link_kind:` metadata for native link flows, `cc:` calling-convention metadata, `section:` placement metadata, `weak: true` for weak symbols, and `ownership:` / `lifetime:` annotations so generated docs and bindings can describe FFI contracts explicitly.

For existing C headers, `cool bindgen` can scaffold this surface for you:

```bash
cool bindgen --library sqlite3 --link-kind shared include/sqlite3.h --output src/sqlite_bindings.cool
```

### Raw Data Declarations And Sections (LLVM backend)

Emit typed raw globals and bind the symbol name to its address:

```python
struct BootHeader:
    magic: u32
    flags: u32
    checksum: i32

def boot_entry():
    section: ".text.boot"
    print(read_u32(BOOT_HEADER))

data BOOT_HEADER: BootHeader = BootHeader(
    magic=464367618,
    flags=0,
    checksum=-464367618,
):
    section: ".data.boot"

boot_entry()
```

On Mach-O targets, use `segment,section` names such as `__TEXT,__boot` or `__DATA,__bootdata`. `data` declarations are LLVM-only.

### Native Compiler (LLVM)

Compile Cool programs to native binaries via an LLVM backend backed by a C runtime:

```bash
cool build hello.cool                        # compiles → ./hello
./hello                                      # runs natively, no runtime needed

cool build --freestanding hello.cool         # emits → ./hello.o
cool build --emit sharedlib hello.cool       # emits → ./libhello.<so|dylib|dll>
cool build --emit staticlib hello.cool       # emits → ./libhello.a
cool build --target i386-unknown-linux-gnu --emit llvm-ir hello.cool
                                            # emits cross-target LLVM IR → ./hello.ll
cool build --target x86_64-unknown-linux-gnu --cpu x86-64-v3 --cpu-features +popcnt hello.cool
cool build --no-libc --entry _start hello.cool
                                            # host-free Linux binary path with explicit entry
cool build --linker-script=link.ld hello.cool  # emits → ./hello.o, then links → ./hello.elf
```

`cool build --freestanding` skips the hosted C runtime compile/link step and writes an object file instead. Freestanding builds accept declaration-style top-level programs: `def`, `extern def`, `data`, `struct`, `union`, plus top-level `import core` for the host-free systems helpers. Other top-level executable statements, imports, and classes are rejected. Freestanding `assert` failure paths lower to a direct LLVM trap instead of depending on libc `abort()`. Use `entry: "symbol_name"` metadata on a zero-argument `def` to export an additional raw entry symbol for custom link flows. All raw memory builtins (`read_*`, `write_*`, and `_volatile` variants) are lowered directly to LLVM IR in freestanding mode — no C runtime symbols are needed.

`--linker-script=<path>` (implies `--freestanding`) compiles to a `.o` then invokes LLD (`ld.lld`) to link a kernel image (`.elf`) using the provided GNU linker script. The same effect is available project-wide via `linker_script = "link.ld"` in `cool.toml`. `--entry <symbol>` / `[build].entry` let you control the final linker entry for no-libc or kernel-style outputs, while `--cpu` / `--cpu-features` thread target tuning through the LLVM target machine.

The LLVM backend supports: integers, floats, strings, booleans, variables, arithmetic/bitwise/comparison operators, `if`/`elif`/`else`, `while`/`for` loops, `break`/`continue`, functions (including recursion, default arguments, keyword arguments, top-level typed parameters/return types, and generic defs lowered onto the shared dynamic runtime), enums/tagged unions with `match`, generic structs/unions, classes with `__init__`, inheritance, methods, `implements`, and `super()`, `print()`, `str()`, `isinstance()`, `copy()`, `clone()`, `close()`, `panic()`, `abort()`, `try` / `except` / `else` / `finally`, `raise`, lists, dicts, tuples, slicing, `range()`, `len()`, `min()`, `max()`, `sum()`, `round()`, `sorted()`, `abs()`, `int()`, `float()`, `bool()`, integer width helpers (`i8`, `u8`, `i16`, `u16`, `i32`, `u32`, `i64`, `isize`, `usize`, `word_bits`, `word_bytes`), source-relative file imports like `import "helper.cool"`, project/package imports like `import foo.bar`, LLVM-native `extern def` declarations with `symbol:` / `cc:` / `section:` / `library:` / `link_kind:` / `weak:` / `ownership:` / `lifetime:` metadata, LLVM-native ordinary `def` signatures with ABI-style parameter/return annotations, LLVM-native raw `data` declarations with `section:` placement, native `import ffi` (`ffi.open`, `ffi.func`), native `import math`, native `import os`, native `import sys`, native `import path` (`join`, `basename`, `dirname`, `ext`, `stem`, `split`, `normalize`, `exists`, `isabs`), native `import platform` (`os`, `arch`, `family`, `runtime`, `runtime_profile`, `memory_model`, `panic_policy`, `thread_safety`, `stdlib_split`, `exe_ext`, `shared_lib_ext`, `path_sep`, `newline`, and runtime capability helpers), native `import core` (page/address helpers, allocator hooks, string/formatting helpers, lightweight collections, MMIO/register helpers, and Linux syscall helpers), bundled `import std.memory` / `import std.runtime`, native `import csv` (`rows`, `dicts`, `write`), native `import datetime` (`now`, `format`, `parse`, `parts`, `add_seconds`, `diff_seconds`), native `import hashlib` (`md5`, `sha1`, `sha256`, `digest`), native `import toml` (`loads`, `dumps`), native `import yaml` (`loads`, `dumps` for a config-oriented YAML subset), native `import sqlite` (`execute`, `query`, `scalar`), native `import http` (`get`, `post`, `head`, `getjson`; requires host `curl`), native `import socket` (`connect`, `listen`, `accept`, `connect_udp`, `bind_udp`, `send`, `recv`, `sendto`, `recvfrom`, address helpers, and byte APIs), native `import subprocess` (`run`, `call`, `check_output`), native `import argparse` (`parse`, `help`), native `import logging` (`basic_config`, `log`, `debug`, `info`, `warning`, `warn`, `error`), native `import test` (`equal`, `not_equal`, `truthy`, `falsey`, `is_nil`, `not_nil`, `fail`, `raises`), native `import time`, native `import random` (`seed`, `random`, `randint`, `uniform`, `choice`, `shuffle`), native `import json` (`loads`, `dumps`, `loads_lines`, `dumps_lines`, `pointer`, `transform`), native `import string` (`split`, `join`, `strip`, `lstrip`, `rstrip`, `upper`, `lower`, `replace`, `startswith`, `endswith`, `find`, `count`, `title`, `capitalize`, `format`), native `import list` (`sort`, `reverse`, `map`, `filter`, `reduce`, `flatten`, `unique`), native `import re` (`match`, `search`, `fullmatch`, `findall`, `sub`, `split`), native `import collections` (`Queue`, `Stack`), native `open()` / file methods (`read`, `read_bytes`, `readline`, `readlines`, `write`, `write_bytes`, `writelines`, `close`), and `with` / context managers on normal exit, control-flow exits (`return`, `break`, `continue`), caught exceptions, and unhandled native raises, plus f-strings, ternary expressions, list comprehensions, `in`/`not in`, inline assembly, and raw memory operations.

**LLVM limitations:** none currently tracked in the roadmap feature matrix.

| Feature | Interpreter | Bytecode VM | LLVM |
| ------- | :---------: | :---------: | :--: |
| Variables, arithmetic, comparisons | ✅ | ✅ | ✅ |
| `if`/`elif`/`else`, `while`/`for` loops | ✅ | ✅ | ✅ |
| `break`/`continue` | ✅ | ✅ | ✅ |
| Functions, recursion, default/keyword args | ✅ | ✅ | ✅ |
| Classes with `__init__`, inheritance, methods, `super()` | ✅ | ✅ | ✅ |
| Lists, indexing, slicing, `len()`, `range()` | ✅ | ✅ | ✅ |
| Dicts (`{k:v}`, `d[k]`, `d[k]=v`, `in`) | ✅ | ✅ | ✅ |
| Tuples (literals, index, unpack, `in`) | ✅ | ✅ | ✅ |
| `str()`, `isinstance()`, `min()`, `max()`, `sum()`, `round()`, `sorted()`, `abs()`, `int()`, `float()`, `bool()` | ✅ | ✅ | ✅ |
| `import math` | ✅ | ✅ | ✅ |
| `import os` | ✅ | ✅ | ✅ |
| `import sys` | ✅ | ✅ | ✅ |
| `import path` (`join`, `basename`, `dirname`, `ext`, `stem`, `split`, `normalize`, `exists`, `isabs`) | ✅ | ✅ | ✅ |
| `import platform` (`os`, `arch`, `family`, `runtime`, `runtime_profile`, `memory_model`, `panic_policy`, `thread_safety`, `stdlib_split`, `exe_ext`, `shared_lib_ext`, `path_sep`, `newline`, `is_windows`, `is_unix`, `has_ffi`, `has_raw_memory`, `has_extern`, `has_inline_asm`) | ✅ | ✅ | ✅ |
| `import core` (`word_bits`, `word_bytes`, `page_size`, page/paging helpers; allocator hooks are LLVM-only) | ✅ | ✅ | ✅ |
| `import csv` (`rows`, `dicts`, `write`) | ✅ | ✅ | ✅ |
| `import datetime` (`now`, `format`, `parse`, `parts`, `add_seconds`, `diff_seconds`) | ✅ | ✅ | ✅ |
| `import hashlib` (`md5`, `sha1`, `sha256`, `digest`) | ✅ | ✅ | ✅ |
| `import toml` (`loads`, `dumps`) | ✅ | ✅ | ✅ |
| `import yaml` (`loads`, `dumps`) | ✅ | ✅ | ✅ |
| `import sqlite` (`execute`, `query`, `scalar`) | ✅ | ✅ | ✅ |
| `import http` (`get`, `post`, `head`, `getjson`; requires `curl`) | ✅ | ✅ | ✅ |
| `import socket` (`connect`, `listen`, `accept`, `connect_udp`, `bind_udp`, `send`, `recv`, `sendto`, `recvfrom`, address helpers, byte APIs) | ✅ | ✅ | ✅ |
| Bundled networking modules (`url`, `websocket`, `rpc`, `graphql`, `mail`, `feed`, `calendar`, `cluster`) | ✅ | ✅ | ✅ |
| `import argparse` (`parse`, `help`) | ✅ | ✅ | ✅ |
| `import logging` (`basic_config`, `log`, `debug`, `info`, `warning`, `warn`, `error`) | ✅ | ✅ | ✅ |
| `import test` (`equal`, `not_equal`, `truthy`, `falsey`, `is_nil`, `not_nil`, `fail`, `raises`) | ✅ | ✅ | ✅ |
| `import time` | ✅ | ✅ | ✅ |
| `import random` (`seed`, `random`, `randint`, `uniform`, `choice`, `shuffle`) | ✅ | ✅ | ✅ |
| `import json` (`loads`, `dumps`, `loads_lines`, `dumps_lines`, `pointer`, `transform`) | ✅ | ✅ | ✅ |
| `import string` (`split`, `join`, `strip`, `lstrip`, `rstrip`, `upper`, `lower`, `replace`, `startswith`, `endswith`, `find`, `count`, `title`, `capitalize`, `format`) | ✅ | ✅ | ✅ |
| `import list` (`sort`, `reverse`, `map`, `filter`, `reduce`, `flatten`, `unique`) | ✅ | ✅ | ✅ |
| `import re` (`match`, `search`, `fullmatch`, `findall`, `sub`, `split`) | ✅ | ✅ | ✅ |
| `import collections` (`Queue`, `Stack`) | ✅ | ✅ | ✅ |
| `import subprocess` (`run`, `call`, `check_output`) | ✅ | ✅ | ✅ |
| Integer width helpers (`i8`, `u8`, `i16`, `u16`, `i32`, `u32`, `i64`, `isize`, `usize`, `word_bits`, `word_bytes`) | ✅ | ✅ | ✅ |
| `with` / context managers (normal/control-flow exits, caught exceptions, and unhandled native raises) | ✅ | ✅ | ✅ |
| f-strings | ✅ | ✅ | ✅ |
| Ternary expressions | ✅ | ✅ | ✅ |
| List comprehensions | ✅ | ✅ | ✅ |
| `in` / `not in` | ✅ | ✅ | ✅ |
| Closures / lambdas | ✅ | ✅ | ✅ |
| General `import` | ✅ | ✅ | ✅ |
| `try` / `except` / `finally` / `raise` | ✅ | ✅ | ✅ |
| `import ffi` | ✅ | ❌ | ✅ |
| Typed top-level `def` signatures (`x: i32`, `-> f64`, `-> void`) | ✅* | ✅* | ✅ |
| `extern def` / `data` declarations (`symbol:`, `cc:`, `section:`) | ❌ | ❌ | ✅ |
| Inline assembly | ❌ | ❌ | ✅ |
| Raw memory access (`malloc`, `free`, `read_i8/u8/i16/u16/i32/u32/i64`, `write_i8/u8/i16/u16/i32/u32/i64`, plus volatile variants) | ❌ | ❌ | ✅ |
| x86 port I/O (`outb`, `inb`, `write_serial_byte`) | ❌ | ❌ | ✅ |

\* Parsed and accepted, but annotations are currently ignored outside the LLVM backend.

### Inline Assembly (LLVM backend)

Emit AT&T-syntax inline assembly directly from Cool:

```python
asm("nop")                       # single instruction, no I/O
asm("syscall", "")               # with explicit (empty) constraint string
```

`asm()` is only valid in compiled programs (`cool build`).

### Raw Memory (LLVM backend)

Allocate and manipulate memory directly:

```python
buf = malloc(8)          # allocate 8 bytes, returns address as int
write_i64(buf, 99)       # store a 64-bit integer
val = read_i64(buf)      # load it back  →  99

write_u16(buf, 65535)    # store an unsigned 16-bit value
u = read_u16(buf)        # load it back  →  65535

write_i32(buf, -1234)    # store a signed 32-bit value
n = read_i32(buf)        # load it back  →  -1234

write_f64(buf, 1.5)      # store a double
f = read_f64(buf)        # load it back  →  1.5

write_byte(buf, 0xFF)    # store one byte
b = read_byte(buf)       # load it back  →  255

sbuf = malloc(64)
write_str(sbuf, "hi")    # write a null-terminated string
s = read_str(sbuf)       # read it back  →  "hi"

free(buf)
free(sbuf)
```

Memory functions are LLVM-backend only. Pointers are plain integers (the address). For fixed-width arithmetic without touching memory, use `i8()`, `u8()`, `i16()`, `u16()`, `i32()`, `u32()`, and `i64()`.

For MMIO or device-register style access, append `_volatile` to the scalar helpers:

```python
write_u32_volatile(buf, 0xDEADBEEF)
status = read_u32_volatile(buf)
```

Volatile variants exist for `byte`, `i8`, `u8`, `i16`, `u16`, `i32`, `u32`, `i64`, and `f64`.

### x86 Port I/O And Serial Output (LLVM backend)

Emit x86 `IN`/`OUT` instructions for direct device access — the building block for 16550 UART serial output in bare-metal kernels:

```python
# Write a byte to an I/O port (e.g. COM1 data register at 0x3F8)
outb(0x3F8, 65)           # sends 'A' to COM1

# Convenience: write_serial_byte hardwires port 0x3F8
write_serial_byte(72)     # sends 'H'
write_serial_byte(105)    # sends 'i'

# Read a byte from a port (e.g. COM1 line status at 0x3FD)
status = inb(0x3FD)
```

`outb`, `inb`, and `write_serial_byte` are x86/x86-64 only; a clear error is raised on other targets. For MMIO-based serial (ARM, RISC-V), use `write_u8_volatile(uart_base_addr, byte)` instead. All three are LLVM-only and freestanding-safe — no C runtime symbols in the output object file.

### Bundled Shell (`apps/shell.cool`)

Cool also ships with a fully interactive shell written in Cool:

```text
ls [path]          cd <path>          pwd
cat <file>         mkdir <dir>        touch <file>
rm <file>          mv <src> <dst>     cp <src> <dst>
head <file> [n]    tail <file> [n]    wc <file>
grep <pat> <file>  find <pattern>     echo <text>
write <file> <txt> run <file.cool>    source <file>
set VAR=value      alias name=cmd     history
clear
```

- Pipes: `ls | grep cool`, `cat file | head 5`
- Environment variables: `set PATH=/usr/bin`, use as `$PATH`
- Tab completion and up-arrow history navigation in interactive TTY sessions
- Shell scripting via `source <file>`

Interactive terminal apps such as `apps/edit.cool`, `apps/top.cool`, `apps/snake.cool`, and `apps/control.cool`
expect a real TTY because they rely on `import term` raw-mode input and screen control.

---

## Getting Started

**Prerequisites:** Rust (stable, edition 2021). LLVM 17 is required only for native compilation (`cool build`).

```bash
# Build
cargo build --release

# Compile a native binary
./target/release/cool build hello.cool
./hello

# Run a source file with the tree-walk interpreter
./target/release/cool hello.cool

# Start the REPL
./target/release/cool

# Run with the bytecode VM
./target/release/cool --vm hello.cool

# Compile to a native binary (requires LLVM 17)
./target/release/cool build hello.cool
./hello

# Launch Cool shell
./target/release/cool apps/shell.cool

# Show all CLI options
./target/release/cool help

# Compare native Cool vs Rust on the bundled benchmark workloads
cargo run --release --bin bench_compare -- --runs 5 --warmups 1
```

### Release Gate

Run the repo-level release gate before tagging or pushing a release-critical change:

```bash
bash scripts/release_gate.sh
```

The gate runs `cargo fmt --check`, builds the `cool` binary, runs the full Rust test suite, statically checks representative Cool examples, verifies interpreter / VM / LLVM parity on a closure smoke program, and confirms freestanding object output still works.

### Release Candidate Build

Build a compiler release-candidate distribution after the gate passes:

```bash
bash scripts/release_candidate.sh
```

The command runs the release gate by default, builds `target/release/cool`, and writes a platform-specific payload under `dist/release-candidate/<version>/<platform>/`. The payload includes the release binary, README, changelog, roadmap, license, installer, install docs, release scripts, generated release notes, `checksums.txt`, and `manifest.json` with the Cargo version, git commit, worktree state, host platform, Rust toolchain, and release-gate status. It also writes `dist/release-candidate/cool-<version>-<platform>.tar.gz` plus `dist/release-candidate/latest.json` for CI or downstream packaging.

If the gate was already run in the same environment, use:

```bash
bash scripts/release_candidate.sh --skip-gate
```

The script only creates local distribution artifacts; it does not create git tags, push commits, upload GitHub releases, or publish packages. The `Release Candidate` GitHub Actions workflow runs the same script on manual dispatch or `v*` tag pushes and uploads the generated `dist/release-candidate/**` tree as a workflow artifact.

### Release Promotion And Installer Channels

Promote a validated release candidate into upload-ready release assets:

```bash
bash scripts/promote_release.sh --version 1.0.0
```

Promotion validates the RC manifest, `checksums.txt`, archive layout, release-gate status, git commit, and worktree cleanliness before writing `dist/releases/<version>/`. The promoted directory contains the platform tarball, platform manifest/checksum sidecars, `RELEASE.md`, `SHA256SUMS`, `release.json`, `latest.json`, and `install.sh`. By default the command does not create tags, push tags, upload GitHub releases, or publish packages; use `--create-tag` only when you explicitly want a local annotated tag.

Install from a promoted local artifact:

```bash
bash install.sh \
  --from dist/releases/1.0.0/cool-1.0.0-macos-arm64.tar.gz \
  --prefix "$HOME/.local"
```

Install from a hosted GitHub release:

```bash
curl -fsSL https://raw.githubusercontent.com/codenz92/cool-lang/master/install.sh \
  | bash -s -- --version 1.0.0 --prefix "$HOME/.local"
```

Use `--verify-sha256 <hash>` with the archive hash from `SHA256SUMS` when installing from a downloaded asset. See `docs/INSTALL.md` for local, hosted, mirror, and smoke-test details.

### Published Release Trust

Generate SBOM, provenance, trust metadata, and optional detached signatures for a promoted release:

```bash
bash scripts/trust_release.sh generate --version 1.0.0
```

`scripts/promote_release.sh` runs the trust generator by default. The trust layer writes `sbom.spdx.json`, `provenance.intoto.json`, `trust.json`, and `TRUST_SHA256SUMS`; when `--sign-key <private-key.pem>` is provided it also signs `SHA256SUMS`, `release.json`, provenance, SBOM, and trust metadata using OpenSSL detached signatures. Verify the unsigned hash chain, or verify signatures with a public key:

```bash
bash scripts/trust_release.sh verify --version 1.0.0
bash scripts/trust_release.sh verify --version 1.0.0 --verify-key release-signing-public.pem
```

Dry-run GitHub Release publishing:

```bash
bash scripts/publish_release.sh --version 1.0.0
```

Publish with the GitHub CLI after trust verification:

```bash
bash scripts/publish_release.sh --version 1.0.0 --publish --no-draft
```

The `Published Release` GitHub Actions workflow builds the RC, promotes it, generates trust metadata, optionally signs with `COOL_RELEASE_SIGNING_KEY_B64`, verifies the result, and uploads or publishes the final assets. The installer can verify release metadata before installing:

```bash
bash install.sh --version 1.0.0 --verify-metadata
```

### Project workflow

```bash
# Scaffold a new project
cool new myapp
cd myapp

# Alternative scaffolds
cool new toolkit --template lib
cool new echoer --template service
cool new kernelkit --template freestanding

# Interpret during development
cool src/main.cool

# Run tests
cool test

# Run native benchmarks
cool bench

# Inspect the project and dependency health
cool pkg info
cool pkg doctor

# Generate API docs
cool doc
cool doc --format html --output docs/API.html

# Run concurrent health checks from a pulse manifest
cool apps/pulse.cool --file pulse.toml

# Add dependencies
cool add toolkit --path ../toolkit
cool add theme --git https://github.com/acme/theme.git
cool install

# List project tasks
cool task list

# Run a manifest task
cool task build
cool task bench
cool task doc

# Compile for release
cool build                   # reads cool.toml, produces ./myapp
./myapp
cool build --profile strict  # checked build that requires annotated top-level defs
cool build --freestanding    # reads cool.toml, produces ./myapp.o
cool build --target i386-unknown-linux-gnu --emit llvm-ir
cool build --emit assembly   # writes ./myapp.s
cool build --emit llvm-ir    # writes ./myapp.ll
cool build --emit staticlib  # writes ./libmyapp.a
cool build --emit sharedlib  # writes ./libmyapp.<so|dylib|dll>
cool build --cpu native --cpu-features +sse4.2,+popcnt
cool build --no-libc --entry _start
# (or set linker_script in cool.toml to produce ./myapp.elf via LLD)

# Package a release artifact
cool bundle                  # writes dist/*.tar.gz plus metadata/symbol sidecars
cool bundle --target i386-unknown-linux-gnu
cool release --bump minor
```

`cool.toml` format:

```toml
[project]
name = "myapp"
version = "0.1.0"
main = "src/main.cool"
output = "myapp"              # optional, defaults to name
sources = ["src", "lib"]      # optional additional module roots
linker_script = "link.ld"     # optional; enables kernel image output via LLD (cool build → myapp.elf)

[build]
profile = "dev"               # optional: dev, release, freestanding, or strict
emit = "binary"               # optional: binary, object, assembly, llvm-ir, staticlib, or sharedlib
target = "x86_64-unknown-linux-gnu"  # optional: explicit LLVM target triple
cpu = "x86-64-v3"             # optional: explicit LLVM target CPU
cpu_features = "+popcnt"      # optional: explicit LLVM target feature string
entry = "_start"              # optional: explicit linker entry for no-libc/kernel outputs
incremental = true            # optional: enable the project-local native build cache
reproducible = true           # optional: deterministic tool output and normalized source paths
debug = true                  # optional: emit DWARF/line locations for native builds
no_libc = false               # optional: skip the hosted runtime/libc assumptions where supported

[toolchain]
cool = "1.0.0"               # optional: pin the Cool CLI version expected by this project
cc = "clang"                 # optional: C compiler for hosted native links
ar = "llvm-ar"               # optional: archiver for static libraries
lld = "ld.lld"               # optional: linker for kernel-image flows

[native]
libraries = ["sqlite3"]                     # optional: link -lsqlite3
# libraries = [{ name = "mylib", kind = "static" }]
search = ["native/lib"]                     # optional: extra -L search paths
rpath = ["@loader_path/../lib"]             # optional: runtime library search path

[capabilities]
file = true                 # optional: allow open(), os file helpers, sqlite, etc.
network = true              # optional: allow http/socket access
env = true                  # optional: allow getenv-style host environment access
process = false             # optional: allow subprocess/os.popen and jobs.command()

[dependencies]
toolkit = { path = "../toolkit" }   # imported as `toolkit.*`
theme = { git = "https://github.com/acme/theme.git" }   # fetched into .cool/deps/theme

[tasks.build]
description = "Build a native binary"
run = "cool build"

[tasks.test]
description = "Run Cool tests"
run = "cool test"

[tasks.publish]
description = "Validate and package a source distribution"
run = "cool publish"

[tasks.bench]
description = "Run native benchmarks"
run = "cool bench"

[tasks.doc]
description = "Generate API docs"
run = "cool doc --output docs/API.md"

[tasks.fmt]
description = "Format Cool source files"
run = "cool fmt"
```

`cool build` accepts either the legacy flat-key manifest or the preferred `[project]` table shown above. `sources` extends module search roots for `import foo.bar`, and `[dependencies]` now supports both local `path` dependencies and vendored `git` dependencies. `[build].profile` controls the default build workflow: `dev` runs `cool check` before compile, `strict` runs `cool check --strict`, `freestanding` makes `cool build` default to object output, and `release` keeps the plain hosted compile path. `[build].emit` (or `cool build --emit ...`) selects the final artifact explicitly: hosted/freestanding object files, standalone assembly, LLVM IR, static libraries, shared libraries, or normal binaries. `[build].target`, `[build].cpu`, and `[build].cpu_features` (or the matching CLI flags) thread explicit target tuning into native output and packaging metadata. `[build].entry` / `--entry` choose the final linker entry for no-libc or kernel-style flows, and `[build].no_libc` / `--no-libc` switch hosted binary output onto the host-free path where supported. `[native]` provides extra libraries, frameworks, search paths, and rpaths for the native linker; `extern def` can also attach `library:` and `link_kind:` metadata directly to individual bindings. `[capabilities]` sets the runtime permission policy for file, network, env, and process access across interpreter, VM, and native builds, and the same values are surfaced by `platform.capabilities()`, `cool pkg capabilities`, and package metadata. `[build].incremental` controls the project-local native build cache under `.cool/cache/build`, while `[build].reproducible` normalizes source paths and enables deterministic archive/linker settings where supported by the selected toolchain. `[build].debug` enables native debug info and line locations, and `[toolchain]` can pin the expected Cool CLI version plus external `cc`/`ar`/`lld` tools. When no explicit emit is set, `--linker-script` / `linker_script` still produce a kernel image (`.elf`). Use `cool add` to update `cool.toml`, and `cool install` / `cool install --locked` to materialize git dependencies under `.cool/deps`, verify dependency checksums, and maintain `cool.lock`. `cool new` also scaffolds `tests/test_main.cool`, `benchmarks/bench_main.cool`, and starter `[tasks.publish]`, `[tasks.doc]`, and `[tasks.fmt]` tasks, so packaging, docs, and formatting work immediately in new projects; it also supports `--template app|lib|service|freestanding` for different starting points. By default the benchmark runner discovers files named `bench_*.cool` or `*_bench.cool` under `benchmarks/`. By default the test runner discovers files named `test_*.cool` or `*_test.cool` under `tests/`. Use `cool test --vm` or `cool test --compile` to run the same files through the VM or native backend.

`cool task` reads the `[tasks]` section from `cool.toml`. Task entries can be strings, lists of shell commands, or tables with `run`, `deps`, `cwd`, `env`, and `description` fields.

Inside test files, `import test` gives you assertion helpers like `test.equal(...)`, `test.truthy(...)`, `test.is_nil(...)`, and `test.raises(...)`.

For tooling, `cool ast <file.cool>` prints the parsed AST as JSON, `cool inspect <file.cool>` summarizes top-level imports and symbols as JSON, `cool symbols [file.cool]` prints a resolved symbol index across reachable modules as JSON, `cool diff <before.cool> <after.cool>` compares top-level changes as JSON, `cool modulegraph <file.cool>` resolves reachable imports and prints the resulting graph as JSON, `cool check [file.cool]` performs static checks: unresolved imports, import cycles, duplicate symbols, typed/local binding mismatches, immutable reassignments, private export/import validation, missing returns on typed functions, typed `def` call/return mismatches, non-exhaustive local-enum `match` blocks, trait implementation errors, and generic trait-bound failures, and `cool doc [file.cool]` generates module/type/API docs as Markdown, HTML, or JSON. `cool check --strict` additionally requires every top-level `def` to have fully annotated parameters and a return type, making it suitable as a CI gate for typed codebases. `cool lsp` starts a JSON-RPC Language Server Protocol server on stdin/stdout for editor integration (VS Code, Neovim, Helix, etc.) with typed diagnostics, completions, hover, go-to-definition, document symbols, and workspace symbol search. `cool pkg` adds a Cool-native project workflow layer with `info`, `deps`, `tree`, `capabilities`, and `doctor` subcommands.

## VS Code

A first-party VS Code extension now lives in [`editors/vscode/`](editors/vscode/). It registers `.cool` files as a language, adds syntax highlighting and indentation rules, and connects VS Code to `cool lsp`.

To install it locally:

```bash
cargo build --release
cd editors/vscode
npm install
npx @vscode/vsce package
```

Then in VS Code run `Extensions: Install from VSIX...` and choose the generated `.vsix` file. If `cool` is not already on your `PATH`, point the extension at your binary:

```json
{
  "cool.lsp.serverCommand": ["/absolute/path/to/cool", "lsp"]
}
```

---

## CLI Reference

| Command | Description |
| ------- | ----------- |
| `cool` | Start the REPL |
| `cool <file.cool>` | Run a file with the tree-walk interpreter |
| `cool --vm <file.cool>` | Run a file with the bytecode VM |
| `cool --compile <file.cool>` | Compile to a native binary (LLVM) |
| `cool ast <file.cool>` | Print the parsed AST as JSON |
| `cool inspect <file.cool>` | Print a JSON summary of top-level symbols |
| `cool symbols [file.cool]` | Print a resolved JSON symbol index for reachable modules |
| `cool diff <before.cool> <after.cool>` | Print a JSON summary of top-level changes |
| `cool modulegraph <file.cool>` | Print the resolved import graph as JSON |
| `cool check [--strict] [file.cool]` | Statically check imports, cycles, duplicate symbols, typed bindings, immutable reassignments, missing returns, export validation, type mismatches, local-enum `match` exhaustiveness, and trait/bound validation; `--strict` enforces full annotations |
| `cool doc [file.cool]` | Generate API docs as Markdown, HTML, or JSON |
| `cool bindgen <header.h>` | Generate starter Cool FFI bindings from a C header subset |
| `cool build` | Build the project described by `cool.toml` |
| `cool build --profile <name> [file.cool]` | Build with `dev`, `release`, `freestanding`, or `strict` profile rules |
| `cool build --emit <kind> [file.cool]` | Emit `binary`, `object`, `assembly`, `llvm-ir`, `staticlib`, or `sharedlib` artifacts |
| `cool build --target <triple> [file.cool]` | Emit native code for an explicit LLVM target triple |
| `cool build --cpu <name> --cpu-features <spec> [file.cool]` | Tune native output for an explicit target CPU / feature set |
| `cool build <file.cool>` | Compile a single file to a native binary |
| `cool build --freestanding [file.cool]` | Emit a freestanding object file (`.o`) without linking the hosted runtime |
| `cool build --no-libc --entry <symbol> [file.cool]` | Link a host-free Linux binary when the program provides an explicit low-level entry |
| `cool build --linker-script=<ld> [file.cool]` | Compile freestanding and link a kernel image (`.elf`) via LLD |
| `cool bench [path ...]` | Compile and benchmark native Cool programs |
| `cool bundle [--target <triple>]` | Build and package the project into a distributable tarball with metadata and symbol-map sidecars |
| `cool release [--bump patch] [--target <triple>]` | Bump version, bundle, emit artifact metadata, and git-tag a release |
| `cool publish [--dry-run]` | Validate dependencies and package a source distribution for publishing |
| `cool pkg <info\|deps\|tree\|capabilities\|doctor>` | Inspect project metadata, dependencies, and manifest capability policy |
| `cool lsp` | Start the language server (LSP) on stdin/stdout |
| `cool install [--locked\|--frozen]` | Fetch or verify dependencies and maintain `cool.lock` with checksums |
| `cool add <name> ...` | Add a path or git dependency to `cool.toml` |
| `cool layout <artifact>` | Inspect section, symbol, entry-point, and archive-member layout for native artifacts |
| `cool fmt [--check] [path ...]` | Reformat Cool source files or report files that need formatting |
| `cool test [path ...]` | Discover and run Cool tests |
| `cool task [name|list ...]` | List or run manifest-defined project tasks |
| `cool new <name> [--template kind]` | Scaffold an app, library, service, or freestanding project |
| `cool help` | Show usage help |

### Native benchmarks

Use `cool bench` inside a project to compile and time files under `benchmarks/` (or pass explicit `.cool` benchmark files/directories). Add `--profile` to capture a runtime hotspot summary for each benchmark run; the runtime also writes that report to `COOL_PROFILE_OUT` when the environment variable is set. For the Cool repo itself, the bundled harness in [benchmarks/README.md](/Users/jamie/cool-lang/benchmarks/README.md) still compares native Cool binaries against matched Rust binaries for integer loops, string processing, list/dict work, and raw-memory kernels.

### API docs

Use `cool doc` to turn a module graph into API documentation. By default it emits Markdown to stdout, but `--format html` and `--format json` are also supported, and `--output <path>` writes the result to disk. Inside a project, `cool doc` with no file argument uses the manifest `main` entry and documents every reachable local module; pass `--private` to include private functions, bindings, classes, and methods in the output.

### Bindgen and layout inspection

Use `cool bindgen` to turn a simple C header into starter Cool bindings: today it understands plain `#define` constants, enum constants, struct/union layouts, typedef aliases, and straightforward function prototypes. `--library` and `--link-kind` attach native-link metadata directly to the generated `extern def` blocks so a bound header can move quickly into a buildable Cool module.

Use `cool layout` on `.o`, `.a`, executables, `.elf` kernel images, or shared libraries when you need to inspect sections, symbols, archive members, or final entry points. `--json` is intended for tooling and CI checks around embedded/kernel link layouts.

### Native artifact selection

Use `cool build --emit ...` when you want something other than the default binary or freestanding object. `object` writes a plain `.o`, `assembly` writes `.s`, `llvm-ir` writes `.ll`, `staticlib` writes `lib<name>.a` (including the hosted runtime object when needed), and `sharedlib` writes `lib<name>.<so|dylib|dll>`. The same choice can live in `cool.toml` via `[build].emit = "..."`, and CLI `--emit` overrides the manifest.

### Cross-compilation

Use `cool build --target <triple>` to emit LLVM IR, assembly, objects, static libraries, shared libraries, or binaries for an explicit target triple, or set `[build].target = "..."` in `cool.toml` to make that the project default. `cool bundle` and `cool release` accept the same `--target` override and carry the chosen target into `dist/` archive names plus metadata. `--cpu` and `--cpu-features` let you tune the selected target more precisely. Cross-target freestanding/object-style outputs work directly through LLVM; hosted binaries and hosted library output additionally require a C toolchain for that target (`clang`, or an explicit `COOL_CC` / `CC` toolchain).

### Native linking and no-libc flows

Use `[native]` in `cool.toml` for additional libraries, frameworks, search paths, and rpaths that the native linker should see for the whole project. For one-off bindings, `extern def` can specify `library:` and `link_kind:` directly, which is often enough for a focused FFI module.

For low-level targets, `cool build --no-libc --entry <symbol>` links a Linux binary without the hosted runtime, while `--linker-script=<path>` / `linker_script = "..."` keep the kernel-image path available through LLD. The `core.syscall*`, `core.mmio_*`, and `core.reg_*` helpers are intended for these freestanding or no-libc outputs.

### Incremental and reproducible builds

Native project builds now use a local content-addressed cache under `.cool/cache/build` by default. Use `cool build --incremental` / `--no-incremental`, or `[build].incremental = true|false`, to control cache reuse. For release-oriented workflows, `[build].reproducible = true` (or `cool build --reproducible`) normalizes debug/source paths and enables deterministic archive/linker settings where supported by the selected toolchain. `[toolchain]` can pin the expected Cool CLI version plus the external `cc`, `ar`, and `lld` tools used by hosted and freestanding native outputs.

### Native stack traces and debug info

`cool build --debug` emits native debug info and line locations, and unhandled native exceptions now print a stack trace with function names and line numbers. That same tracing machinery backs `cool bench --profile`, so production-style builds can keep diagnostics lightweight while benchmark runs still surface hotspots.

### Cool-native package workflow

Use `cool pkg` when you want project/package introspection without dropping to ad hoc shell scripts. `cool pkg info` summarizes the current project, `deps` and `tree` show dependency shape, `capabilities` prints the manifest permission policy, and `doctor` checks that the main file, dependency roots, and lockfile are in place.

`cool pkg install`, `cool pkg add`, and `cool pkg publish` forward to the main package-management commands, so teams can keep a single Cool-native workflow surface for local project operations.

### Pulse and Control

`apps/pulse.cool` is a concurrent health-check runner for TOML manifests, and `apps/control.cool` is its terminal dashboard companion. Both use `import jobs`, so they exercise the same concurrency and capability path available to ordinary Cool applications.

`pulse.toml` manifests declare checks under `[checks.<name>]` using either `command = "..."`, `url = "..."`, or `sleep = ...`:

```toml
[checks.homepage]
url = "https://example.com/health"
method = "GET"

[checks.migrations]
command = "cool pkg doctor"
```

Run them directly with `cool apps/pulse.cool --file pulse.toml`, or use `cool apps/control.cool --file pulse.toml` for the TTY dashboard.

### Release artifacts

`cool bundle` now writes three release-oriented outputs under `dist/`: the archive itself (`.tar.gz`), a `.metadata.json` sidecar describing the bundled project/artifact/build profile, and a `.symbols.txt` sidecar generated from `nm`/`llvm-nm` when available. The archive also embeds `metadata.json` plus `symbols/<artifact>.symbols.txt`, so downstream packaging or CI jobs can inspect artifact identity without unpacking the full project tree manually.

### Source publishing and locked installs

Use `cool publish` to validate the current project as a source distribution, verify or generate `cool.lock`, write `dist/<name>-<version>.publish.json`, and package a `.coolpkg.tar.gz` archive. `cool publish --dry-run` performs the same validation without archiving. `cool install --locked` and `cool install --frozen` now verify dependency checksums and manifest selectors against `cool.lock`, giving path/git dependencies the same reproducibility guarantees the native build pipeline now expects.

---

## Example

```python
# hello.cool
def greet(name, greeting="Hello"):
    print(f"{greeting}, {name}!")

greet("world")
greet("Cool", greeting="Hey")

# Classes
class Animal:
    def __init__(self, name):
        self.name = name

    def speak(self):
        return "..."

class Dog(Animal):
    def speak(self):
        return f"{self.name} says woof!"

dog = Dog("Rex")
print(dog.speak())

# List comprehension
squares = [x * x for x in range(10) if x % 2 == 0]
print(squares)

# map / filter / zip
evens = list(filter(lambda x: x % 2 == 0, range(10)))
doubled = list(map(lambda x: x * 2, evens))
pairs = list(zip(evens, doubled))
print(pairs)

# FFI — call libm directly
import ffi
libm = ffi.open("libm")
sqrt_fn = ffi.func(libm, "sqrt", "f64", ["f64"])
print(sqrt_fn(2.0))    # 1.4142135623730951
```

More examples are in the [`examples/`](examples/) directory, including `examples/coolboard/` for a native SQLite-backed note service and `examples/kernel_demo/` for a freestanding VGA-text kernel/object demo.

---

## Project Structure

```text
src/
  lexer.rs          Token scanner with INDENT/DEDENT handling
  parser.rs         Recursive descent parser → AST
  ast.rs            AST node definitions
  interpreter.rs    Tree-walk interpreter (+ FFI via libloading)
  compiler.rs       AST → bytecode compiler
  opcode.rs         Bytecode instruction set and VM value types
  vm.rs             Bytecode virtual machine
  llvm_codegen.rs   LLVM native compiler (with embedded C runtime)
  tooling.rs        Static analysis: AST dump, inspect, symbols, check, diff
  lsp.rs            Language server protocol (LSP) over stdin/stdout
  main.rs           CLI entry point, REPL, build/new/lsp subcommands

apps/
  shell.cool        The Cool interactive shell
  calc.cool         Calculator REPL
  notes.cool        Note-taking app
  top.cool          Process viewer
  edit.cool         Text editor
  snake.cool        Snake game
  http.cool         HTTP client
  browse.cool       TUI file browser (two-pane layout, file preview)
  pulse.cool        Concurrent health-check runner
  control.cool      Terminal dashboard for pulse manifests
  pulsekit.cool     Shared pulse/control check engine

cmd/
  lib/projectlib.cool Shared manifest/project helpers for bundled commands
  new.cool          Project scaffolder for `cool new`
  task.cool         Manifest task runner used by `cool task`
  add.cool          Dependency manifest updater for `cool add`
  install.cool      Dependency installer for `cool install`
  pkg.cool          Cool-native project/package workflow
  bundle.cool       Project bundler for `cool bundle`
  release.cool      Release manager for `cool release`

stdlib/
  jobs.cool         Structured concurrency helpers
  bytes.cool        Byte strings, hex, UTF-8, and binary packing helpers
  base64.cool       Base64 encode/decode on byte lists and text
  codec.cool        Pluggable codec dispatch for text/binary formats
  glob.cool         Wildcard matching and recursive file discovery
  tempfile.cool     Temporary files/directories with cleanup-aware handles
  process.cool      PID/env/signal/runtime inspection helpers
  fswatch.cool      Polling file watching with snapshots and diffs
  daemon.cool       Background service helpers with pid files and restart policy
  sandbox.cool      Constrained file/process automation helpers
  sync.cool         File/state snapshot, conflict, and reconciliation helpers
  store.cool        Persistent namespaced key-value storage with transactions
  cache.cool        In-memory and disk-backed TTL caches with invalidation helpers
  memo.cool         Deterministic memoization helpers built on cache backends
  package.cool      Semver, manifest, and dependency-tree helpers
  compress.cool     Gzip, tar, zip, and checksum helpers for packaged assets
  archive.cool      Higher-level archive pack/list/unpack helpers
  bundle.cool       Single-file app bundle packaging and asset extraction
  doc.cool          Markdown, manpage, and HTML document rendering helpers
  template.cool     Variable, loop, and partial string/file templating
  lexer.cool        Token scanning and syntax-highlighting support
  parser.cool       Token stream helpers and small DSL parsing primitives
  ast.cool          Source summaries for imports, symbols, and declarations
  inspect.cool      Portable value and module inspection helpers
  diff.cool         Line/text diff, stats, and merge-assist helpers
  patch.cool        Unified patch creation and application helpers
  project.cool      Manifest, scaffold, and workspace metadata helpers
  release.cool      Release planning, changelog, tag, and artifact helpers
  repo.cool         Git status, branch, diff, and summary helpers
  modulegraph.cool  Import graph resolution, cycle checks, and DOT output
  plugin.cool       Plugin descriptors, discovery, registries, and lifecycle hooks
  lsp.cool          JSON-RPC/LSP message, diagnostic, completion, and hover helpers
  ffiutil.cool      FFI signature parsing and wrapper-generation helpers
  shell.cool        Shell quoting, splitting, aliasing, completion, and source parsing
  event.cool        Pub/sub events, listeners, timers, and message queues
  workflow.cool     Step graphs, checkpoints, resumability, and automation flows
  agent.cool        Task plans, executor handlers, and memory helpers
  retry.cool        Retry policies, backoff, jitter, and failure classification
  metrics.cool      Counters, gauges, histograms, timers, and Prometheus text
  trace.cool        Trace/span IDs, events, annotations, and span export helpers
  profile.cool      Runtime sample recording, hotspot summaries, and flame text
  bench.cool        Lightweight benchmark cases, suites, stats, and comparison
  notebook.cool     Executable note cells, saved outputs, Markdown, and JSON files
  secrets.cool      Secret lookup, redaction, encrypted vaults, and env injection
  decimal.cool      Exact decimal arithmetic with scale-aware formatting
  money.cool        Decimal-safe currency values, formatting, conversion, allocation
  stats.cool        Descriptive statistics, percentiles, histograms, and sampling
  vector.cool       Vector arithmetic, dot/cross products, norms, and distances
  matrix.cool       Small matrix shapes, transpose, multiplication, and determinants
  geom.cool         Points, rectangles, circles, areas, bounds, and intersections
  graph.cool        Graph nodes/edges, BFS/DFS, shortest paths, DAG and cycle helpers
  tree.cool         Generic tree construction, traversal, search, map, and filter
  pipeline.cool     Composable map/filter/reduce and named value pipelines
  stream.cool       Lazy-style stream adapters, ranges, filtering, mapping, chunking
  table.cool        Tabular rows, selection, sorting, grouping, rendering, and CSV
  search.cool       Local text indexing, query scoring, result sorting, highlighting
  embed.cool        Bag-of-words embeddings, cosine similarity, nearest-neighbor search
  ml.cool           Standardization, min-max scaling, KNN, confusion, and accuracy
  crypto.cool       Key derivation, random tokens, signatures, and symmetric envelopes
  ansi.cool         ANSI styling, cursor movement, and box drawing helpers
  color.cool        RGB/HSL/HSV, palettes, hex, gradients, luminance, and contrast
  theme.cool        Reusable palettes, spacing scales, and text-style presets
  tui.cool          Deterministic labels, buttons, panels, lists, focus, and events
  scene.cool        Lightweight ASCII/TUI scene graph helpers
  image.cool        In-memory images, pixel edits, crop/resize, grayscale, and PPM
  audio.cool        Sample buffers, PCM/WAV records, normalize/mix/trim helpers
  sprite.cool       ASCII sprite frames, tiles, sheets, flipping, and animation
  game.cool         Worlds, timers, entities, input state, loops, and collisions
  html.cool         Escaping, tag stripping, and small extraction helpers
  xml.cool          Lightweight XML parsing, text extraction, and serialization
  unicode.cool      Unicode categories, normalization, width, and grapheme helpers
  locale.cool       Locale parsing plus number/currency formatting helpers
  config.cool       Config loading for json/toml/yaml/ini/env formats
  schema.cool       Typed validation rules and structured errors

editors/vscode/
  extension.js      VS Code extension entry point and LSP client
  package.json      Extension manifest
  syntaxes/         TextMate grammar for `.cool` files

coolc/
  compiler_vm.cool  Self-hosted compiler

examples/
  hello.cool            Variables, loops, functions — start here
  data_structures.cool  Lists, dicts, tuples, comprehensions
  oop.cool              Classes, inheritance, operator overloading
  functional.cool       Closures, lambdas, map/filter, memoize
  errors_and_files.cool try/except/finally, file I/O, JSON, dirs
  stdlib.cool           math, random, re, json, time, collections
  ffi_demo.cool         Calling C libraries (libm, libc) via FFI
  coolboard/           Native SQLite-backed note service example
  kernel_demo/         Freestanding VGA-text kernel/object demo
```

---

## Roadmap

| Phase | Status |
| ----- | ------ |
| 1 — Core interpreter | ✅ Complete |
| 2 — Real language features | ✅ Complete |
| 3 — Cool shell | ✅ Complete |
| 4 — Quality of life (f-strings, lambdas, comprehensions…) | ✅ Complete |
| 5 — Shell: more commands | ✅ Complete |
| 6 — Standard library (json, re, time, random…) | ✅ Complete |
| 7 — Cool applications (editor, calculator, snake…) | ✅ Complete |
| 8 — Compiler (bytecode VM, LLVM, FFI, build tooling) | ✅ Complete |
| 9 — Self-hosted compiler | ✅ Complete |
| 10 — Production readiness and ecosystem | ✅ Complete |
| 11 — Freestanding systems foundation | ✅ Complete |
| 12 — Static semantic core | ✅ Complete |
| 13 — Typed language features | ✅ Complete |
| 14 — Runtime and memory model | ✅ Complete |
| 15 — Native toolchain and distribution | ✅ Complete |
| 16 — Systems interop and targets | ✅ Complete |
| 17 — Signature features and flagship software | ✅ Complete |
| 18 — Release hardening | ✅ Complete |
| 19 — Release candidate and distribution | ✅ Complete |
| 20 — Release promotion and installer channels | ✅ Complete |
| 21 — Published release automation and supply-chain trust | ✅ Complete |

See [`ROADMAP.md`](ROADMAP.md) for the full breakdown.

---

## Self-Hosted Compiler

The self-hosted compiler lives in `coolc/compiler_vm.cool` — a lexer, recursive descent parser, code generator, and bytecode VM all written in Cool itself.

Project tooling is starting to move over too: `cool new`, `cool task`, `cool add`, `cool install`, `cool pkg`, `cool bundle`, and `cool release` now delegate to `cmd/*.cool`, and shared manifest helpers now live in `cmd/lib/projectlib.cool`, so packaged CLI workflows now run in Cool rather than Rust.

It supports:

- Full language: INDENT/DEDENT, if/elif/else, while/for loops, break/continue
- Functions with def/return, closures with upvalue capture
- Classes with inheritance and method dispatch
- Built-in self-test suite covering arithmetic, control flow, closures, classes, inheritance, and FizzBuzz
- Bootstrap mode compiles `compiler_vm.cool` with itself and reports lexing, parsing, and codegen progress

---

## License

MIT
