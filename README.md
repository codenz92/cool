# Cool

A native-first, high-level systems language with Python-like syntax, an LLVM compiler, FFI, and freestanding build support — all implemented in Rust.

Cool is a high-level systems language with Python-like syntax and a native-first toolchain. Its primary compilation path targets native binaries and freestanding object files through LLVM, while the tree-walk interpreter and bytecode VM provide fast iteration, tooling support, and runtime parity. It also ships with a **foreign function interface**, explicit ABI/data-layout features, and bundled terminal apps written in Cool itself.

---

## Features

### Language

- Native-first toolchain: tree-walk interpreter, bytecode VM, LLVM native compiler, freestanding object output, and kernel image linking via LLD
- Systems reach: `extern def`, `data`, structs/unions, FFI, inline assembly, and raw memory operations
- Indentation-based block syntax (Python-style)
- Variables, arithmetic, comparisons, logical and bitwise operators
- `if` / `elif` / `else`, `while`, `for`, `break`, `continue`
- Functions with default args, `*args`, `**kwargs`, keyword args, closures
- Typed local bindings (`name: Type = expr`) and immutable constants (`const NAME: Type = expr`)
- Classes with inheritance, `super()`, operator overloading (`__add__`, `__str__`, `__eq__`, `__len__`, etc.)
- Lists, dicts, tuples with full method support
- `set()` built-in (returns deduplicated list)
- Slicing (`lst[1:3]`, negative indices)
- `try` / `except` / `else` / `finally`, `raise`
- f-strings, multi-line strings, `string.format()`
- List comprehensions, lambda expressions, ternary expressions
- `nonlocal` / `global`, `assert`, `with` / context managers
- `import math`, `import os`, `import sys`, `import path`, `import platform`, `import core`, `import csv`, `import datetime`, `import hashlib`, `import toml`, `import yaml`, `import sqlite`, `import http`, `import argparse`, `import logging`, `import test`
- `import string`, `import list`, `import json`, `import re`, `import time`, `import random`, `import collections`, `import subprocess`, `import socket` (`http` requires host `curl`)
- `import ffi` — call C functions from shared libraries at runtime
- LLVM-native `extern def` declarations with `symbol:` and `cc:` metadata
- LLVM-native ordinary `def` signatures with typed parameters and return types
- Explicit module export control with `public` / `private` on top-level bindings and declarations
- Build profiles for native workflows: `cool build --profile dev|release|freestanding|strict`
- `cool build --freestanding` to emit object files for custom/freestanding link steps; `--linker-script=<path>` to link a kernel image (`.elf`) via LLD
- `cool new --template app|lib|service|freestanding` for app, library, network-service, and freestanding scaffolds
- x86 port I/O primitives: `outb(port, byte)`, `inb(port)`, `write_serial_byte(byte)` — bare-metal serial output with no C runtime dependency
- Package system: `import foo.bar` loads `foo/bar.cool`
- File I/O via `open()`, `read()`, `write()`, `readlines()`
- `runfile()` to execute another `.cool` file at runtime
- `eval(str)` to evaluate a Cool expression or statement at runtime
- `import term` for raw terminal mode, cursor control, terminal sizing, and real-time key input across interpreter, VM, and native builds (real TTY required for interactive input)
- `os.popen(cmd)` to run shell commands and capture output
- Integer width helpers: `i8`, `u8`, `i16`, `u16`, `i32`, `u32`, `i64`, plus pointer-width `isize`, `usize`, `word_bits()`, and `word_bytes()`
- `struct` definitions with typed fields and positional/keyword construction across all runtimes
- `packed struct` — no inter-field padding, stable binary layout in LLVM
- Hex / binary / octal literals, `\x` escape sequences
- REPL mode

### Built-in Functions

`print()`, `input()`, `str()`, `int()`, `float()`, `bool()`, `len()`, `range()`, `type()`, `repr()`, `abs()`, `min()`, `max()`, `sum()`, `any()`, `all()`, `round()`, `sorted()`, `reversed()`, `enumerate()`, `zip()`, `map()`, `filter()`, `list()`, `tuple()`, `dict()`, `set()`, `isinstance()`, `hasattr()`, `getattr()`, `isize()`, `usize()`, `word_bits()`, `word_bytes()`, `assert`, `exit()`

### String Methods

`.upper()`, `.lower()`, `.strip()`, `.lstrip()`, `.rstrip()`, `.split()`, `.join()`, `.replace()`, `.find()`, `.count()`, `.startswith()`, `.endswith()`, `.format()`

### Platform Module

Inspect host OS/architecture details and runtime capabilities from Cool:

```python
import platform

print(platform.os())              # "macos", "linux", "windows", ...
print(platform.arch())            # "x86_64", "aarch64", ...
print(platform.family())          # "unix" or "windows"
print(platform.runtime())         # "interpreter", "vm", or "native"
print(platform.shared_lib_ext())  # "dylib", "so", or "dll"
print(platform.has_ffi())         # runtime capability flags
print(platform.has_raw_memory())
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

`cool check` validates typed bindings, immutable reassignments, missing returns on typed functions, and private/exported module surfaces. `import "helper.cool"` only flattens public exports, and `import helper` only exposes public names on the module namespace.

### Extern Declarations (LLVM backend)

Declare linked external symbols directly in Cool:

```python
extern def abs(x: i32) -> i32

extern def c_strlen(text: str) -> usize:
    symbol: "strlen"
    cc: "c"
    section: ".text.host"

print(abs(-42))
print(c_strlen("hello"))
```

`extern def` is only available in compiled programs (`cool build`). It uses the same ABI type names as `ffi.func`, with optional `symbol:` aliasing, `cc:` calling-convention metadata, and `section:` placement metadata.

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
cool build --linker-script=link.ld hello.cool  # emits → ./hello.o, then links → ./hello.elf
```

`cool build --freestanding` skips the hosted C runtime compile/link step and writes an object file instead. Freestanding builds accept declaration-style top-level programs: `def`, `extern def`, `data`, `struct`, `union`, plus top-level `import core` for the host-free systems helpers. Other top-level executable statements, imports, and classes are rejected. Freestanding `assert` failure paths lower to a direct LLVM trap instead of depending on libc `abort()`. Use `entry: "symbol_name"` metadata on a zero-argument `def` to export an additional raw entry symbol for custom link flows. All raw memory builtins (`read_*`, `write_*`, and `_volatile` variants) are lowered directly to LLVM IR in freestanding mode — no C runtime symbols are needed.

`--linker-script=<path>` (implies `--freestanding`) compiles to a `.o` then invokes LLD (`ld.lld`) to link a kernel image (`.elf`) using the provided GNU linker script. The same effect is available project-wide via `linker_script = "link.ld"` in `cool.toml`.

The LLVM backend supports: integers, floats, strings, booleans, variables, arithmetic/bitwise/comparison operators, `if`/`elif`/`else`, `while`/`for` loops, `break`/`continue`, functions (including recursion, default arguments, keyword arguments, and top-level typed parameters/return types), classes with `__init__`, inheritance, methods, and `super()`, `print()`, `str()`, `isinstance()`, `try` / `except` / `else` / `finally`, `raise`, lists, dicts, tuples, slicing, `range()`, `len()`, `min()`, `max()`, `sum()`, `round()`, `sorted()`, `abs()`, `int()`, `float()`, `bool()`, integer width helpers (`i8`, `u8`, `i16`, `u16`, `i32`, `u32`, `i64`, `isize`, `usize`, `word_bits`, `word_bytes`), source-relative file imports like `import "helper.cool"`, project/package imports like `import foo.bar`, LLVM-native `extern def` declarations with `symbol:` / `cc:` / `section:` metadata, LLVM-native ordinary `def` signatures with ABI-style parameter/return annotations, LLVM-native raw `data` declarations with `section:` placement, native `import ffi` (`ffi.open`, `ffi.func`), native `import math`, native `import os`, native `import sys`, native `import path` (`join`, `basename`, `dirname`, `ext`, `stem`, `split`, `normalize`, `exists`, `isabs`), native `import platform` (`os`, `arch`, `family`, `runtime`, `exe_ext`, `shared_lib_ext`, `path_sep`, `newline`, and runtime capability helpers), native `import core` (`word_bits`, `word_bytes`, page/address helpers, paging-index helpers, and allocator hooks), native `import csv` (`rows`, `dicts`, `write`), native `import datetime` (`now`, `format`, `parse`, `parts`, `add_seconds`, `diff_seconds`), native `import hashlib` (`md5`, `sha1`, `sha256`, `digest`), native `import toml` (`loads`, `dumps`), native `import yaml` (`loads`, `dumps` for a config-oriented YAML subset), native `import sqlite` (`execute`, `query`, `scalar`), native `import http` (`get`, `post`, `head`, `getjson`; requires host `curl`), native `import subprocess` (`run`, `call`, `check_output`), native `import argparse` (`parse`, `help`), native `import logging` (`basic_config`, `log`, `debug`, `info`, `warning`, `warn`, `error`), native `import test` (`equal`, `not_equal`, `truthy`, `falsey`, `is_nil`, `not_nil`, `fail`, `raises`), native `import time`, native `import random` (`seed`, `random`, `randint`, `uniform`, `choice`, `shuffle`), native `import json` (`loads`, `dumps`), native `import string` (`split`, `join`, `strip`, `lstrip`, `rstrip`, `upper`, `lower`, `replace`, `startswith`, `endswith`, `find`, `count`, `title`, `capitalize`, `format`), native `import list` (`sort`, `reverse`, `map`, `filter`, `reduce`, `flatten`, `unique`), native `import re` (`match`, `search`, `fullmatch`, `findall`, `sub`, `split`), native `import collections` (`Queue`, `Stack`), native `open()` / file methods (`read`, `readline`, `readlines`, `write`, `writelines`, `close`), and `with` / context managers on normal exit, control-flow exits (`return`, `break`, `continue`), caught exceptions, and unhandled native raises, plus f-strings, ternary expressions, list comprehensions, `in`/`not in`, inline assembly, and raw memory operations.

**LLVM limitations:** closures/lambdas.

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
| `import platform` (`os`, `arch`, `family`, `runtime`, `exe_ext`, `shared_lib_ext`, `path_sep`, `newline`, `is_windows`, `is_unix`, `has_ffi`, `has_raw_memory`, `has_extern`, `has_inline_asm`) | ✅ | ✅ | ✅ |
| `import core` (`word_bits`, `word_bytes`, `page_size`, page/paging helpers; allocator hooks are LLVM-only) | ✅ | ✅ | ✅ |
| `import csv` (`rows`, `dicts`, `write`) | ✅ | ✅ | ✅ |
| `import datetime` (`now`, `format`, `parse`, `parts`, `add_seconds`, `diff_seconds`) | ✅ | ✅ | ✅ |
| `import hashlib` (`md5`, `sha1`, `sha256`, `digest`) | ✅ | ✅ | ✅ |
| `import toml` (`loads`, `dumps`) | ✅ | ✅ | ✅ |
| `import yaml` (`loads`, `dumps`) | ✅ | ✅ | ✅ |
| `import sqlite` (`execute`, `query`, `scalar`) | ✅ | ✅ | ✅ |
| `import http` (`get`, `post`, `head`, `getjson`; requires `curl`) | ✅ | ✅ | ✅ |
| `import argparse` (`parse`, `help`) | ✅ | ✅ | ✅ |
| `import logging` (`basic_config`, `log`, `debug`, `info`, `warning`, `warn`, `error`) | ✅ | ✅ | ✅ |
| `import test` (`equal`, `not_equal`, `truthy`, `falsey`, `is_nil`, `not_nil`, `fail`, `raises`) | ✅ | ✅ | ✅ |
| `import time` | ✅ | ✅ | ✅ |
| `import random` (`seed`, `random`, `randint`, `uniform`, `choice`, `shuffle`) | ✅ | ✅ | ✅ |
| `import json` (`loads`, `dumps`) | ✅ | ✅ | ✅ |
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
| Closures / lambdas | ✅ | ✅ | ❌ |
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

Interactive terminal apps such as `apps/edit.cool`, `apps/top.cool`, and `apps/snake.cool`
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

# Add dependencies
cool add toolkit --path ../toolkit
cool add theme --git https://github.com/acme/theme.git
cool install

# List project tasks
cool task list

# Run a manifest task
cool task build
cool task bench

# Compile for release
cool build                   # reads cool.toml, produces ./myapp
./myapp
cool build --profile strict  # checked build that requires annotated top-level defs
cool build --freestanding    # reads cool.toml, produces ./myapp.o
# (or set linker_script in cool.toml to produce ./myapp.elf via LLD)
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

[dependencies]
toolkit = { path = "../toolkit" }   # imported as `toolkit.*`
theme = { git = "https://github.com/acme/theme.git" }   # fetched into .cool/deps/theme

[tasks.build]
description = "Build a native binary"
run = "cool build"

[tasks.test]
description = "Run Cool tests"
run = "cool test"

[tasks.bench]
description = "Run native benchmarks"
run = "cool bench"
```

`cool build` accepts either the legacy flat-key manifest or the preferred `[project]` table shown above. `sources` extends module search roots for `import foo.bar`, and `[dependencies]` now supports both local `path` dependencies and vendored `git` dependencies. `[build].profile` controls the default build workflow: `dev` runs `cool check` before compile, `strict` runs `cool check --strict`, `freestanding` makes `cool build` emit `.o` output by default, and `release` keeps the plain hosted compile path. Use `cool add` to update `cool.toml`, and `cool install` to materialize git dependencies under `.cool/deps` and refresh `cool.lock`. `cool new` also scaffolds `tests/test_main.cool` and `benchmarks/bench_main.cool`, so both `cool test` and `cool bench` work immediately in new projects, and now supports `--template app|lib|service|freestanding` for different starting points. By default the benchmark runner discovers files named `bench_*.cool` or `*_bench.cool` under `benchmarks/`. By default the test runner discovers files named `test_*.cool` or `*_test.cool` under `tests/`. Use `cool test --vm` or `cool test --compile` to run the same files through the VM or native backend.

`cool task` reads the `[tasks]` section from `cool.toml`. Task entries can be strings, lists of shell commands, or tables with `run`, `deps`, `cwd`, `env`, and `description` fields.

Inside test files, `import test` gives you assertion helpers like `test.equal(...)`, `test.truthy(...)`, `test.is_nil(...)`, and `test.raises(...)`.

For tooling, `cool ast <file.cool>` prints the parsed AST as JSON, `cool inspect <file.cool>` summarizes top-level imports and symbols as JSON, `cool symbols [file.cool]` prints a resolved symbol index across reachable modules as JSON, `cool diff <before.cool> <after.cool>` compares top-level changes as JSON, `cool modulegraph <file.cool>` resolves reachable imports and prints the resulting graph as JSON, and `cool check [file.cool]` performs static checks: unresolved imports, import cycles, duplicate symbols, typed/local binding mismatches, immutable reassignments, private export/import validation, missing returns on typed functions, and typed `def` call/return mismatches. `cool check --strict` additionally requires every top-level `def` to have fully annotated parameters and a return type, making it suitable as a CI gate for typed codebases. `cool lsp` starts a JSON-RPC Language Server Protocol server on stdin/stdout for editor integration (VS Code, Neovim, Helix, etc.) with typed diagnostics, completions, hover, go-to-definition, document symbols, and workspace symbol search.

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
| `cool check [--strict] [file.cool]` | Statically check imports, cycles, duplicate symbols, typed bindings, immutable reassignments, missing returns, export validation, and type mismatches; `--strict` enforces full annotations |
| `cool build` | Build the project described by `cool.toml` |
| `cool build --profile <name> [file.cool]` | Build with `dev`, `release`, `freestanding`, or `strict` profile rules |
| `cool build <file.cool>` | Compile a single file to a native binary |
| `cool build --freestanding [file.cool]` | Emit a freestanding object file (`.o`) without linking the hosted runtime |
| `cool build --linker-script=<ld> [file.cool]` | Compile freestanding and link a kernel image (`.elf`) via LLD |
| `cool bench [path ...]` | Compile and benchmark native Cool programs |
| `cool bundle` | Build and package the project into a distributable tarball |
| `cool release [--bump patch]` | Bump version, bundle, and git-tag a release |
| `cool lsp` | Start the language server (LSP) on stdin/stdout |
| `cool install` | Fetch git dependencies and write `cool.lock` |
| `cool add <name> ...` | Add a path or git dependency to `cool.toml` |
| `cool test [path ...]` | Discover and run Cool tests |
| `cool task [name|list ...]` | List or run manifest-defined project tasks |
| `cool new <name> [--template kind]` | Scaffold an app, library, service, or freestanding project |
| `cool help` | Show usage help |

### Native benchmarks

Use `cool bench` inside a project to compile and time files under `benchmarks/` (or pass explicit `.cool` benchmark files/directories). For the Cool repo itself, the bundled harness in [benchmarks/README.md](/Users/jamie/cool-lang/benchmarks/README.md) still compares native Cool binaries against matched Rust binaries for integer loops, string processing, list/dict work, and raw-memory kernels.

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

More examples are in the [`examples/`](examples/) directory.

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

cmd/
  lib/projectlib.cool Shared manifest/project helpers for bundled commands
  new.cool          Project scaffolder for `cool new`
  task.cool         Manifest task runner used by `cool task`
  add.cool          Dependency manifest updater for `cool add`
  install.cool      Dependency installer for `cool install`
  bundle.cool       Project bundler for `cool bundle`
  release.cool      Release manager for `cool release`

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

See [`ROADMAP.md`](ROADMAP.md) for the full breakdown.

---

## Self-Hosted Compiler

The self-hosted compiler lives in `coolc/compiler_vm.cool` — a lexer, recursive descent parser, code generator, and bytecode VM all written in Cool itself.

Project tooling is starting to move over too: `cool new`, `cool task`, `cool add`, `cool install`, `cool bundle`, and `cool release` now delegate to `cmd/*.cool`, and shared manifest helpers now live in `cmd/lib/projectlib.cool`, so packaged CLI workflows now run in Cool rather than Rust.

It supports:

- Full language: INDENT/DEDENT, if/elif/else, while/for loops, break/continue
- Functions with def/return, closures with upvalue capture
- Classes with inheritance and method dispatch
- Built-in self-test suite covering arithmetic, control flow, closures, classes, inheritance, and FizzBuzz
- Bootstrap mode compiles `compiler_vm.cool` with itself and reports lexing, parsing, and codegen progress

---

## License

MIT
