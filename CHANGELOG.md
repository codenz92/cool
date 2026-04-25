# Changelog

All notable changes to the Cool language project.

## [Unreleased] - Phase 11 In Progress

### Phase 11 — Freestanding Systems Foundation (In Progress)

#### Numeric And Memory Primitives

- Volatile LLVM raw-memory helpers for MMIO/device-style access: `read_*_volatile` / `write_*_volatile` across byte, `i8`/`u8`, `i16`/`u16`, `i32`/`u32`, `i64`, and `f64`
- Interpreter and bytecode VM now reserve LLVM-only raw-memory builtins and raise the same backend-specific guidance (`compile with cool build`) instead of a missing-name error

#### Data Layout And ABI

- `union` declarations with typed fields (`i8`–`i64`, `u8`–`u64`, `f32`/`f64`, `bool`), keyword construction, and zero default init across all runtimes (interpreter, VM, LLVM)
- Interpreter/VM: `union` lowered to a class with zero-defaulted fields; all fields independently accessible
- LLVM: `[max_size x i8]` body, bitcast-based field access (all fields at offset 0), zero-arg ctor via `calloc`, kwarg construction emits inline stores

## [1.1.0] - 2026-04-24 - Phase 10 Complete

### Phase 10 — Production Readiness And Ecosystem (Complete)

#### Language Server Protocol

- `cool lsp` — full LSP server over stdin/stdout: parse/lex diagnostics on open/change, keyword and builtin completions, module-name completions after `import`, hover signatures for functions/classes/structs, go-to-definition across open files, document symbols, workspace symbol search

#### Struct And Systems Data Layout

- `struct` definitions with typed fields (`i8`–`i64`, `u8`–`u64`, `f32`/`f64`, `bool`), positional + keyword construction, and coercion on init across all runtimes (interpreter, VM, LLVM)
- `packed struct` — `packed struct Name:` syntax, consecutive byte layout with no inter-field padding, LLVM packed attribute, stable GEP-based field access
- Stable binary struct layout in LLVM — real LLVM struct types, GEP field access for locals, side-table dispatch for dynamic paths

#### Networking

- `import socket` — TCP client (`connect`) and server (`listen`, `accept`) with `send`, `recv`, `readline`, and `close` across all runtimes

#### Packaging And Release Tooling

- `cool bundle` — build + distributable tarball with `[bundle].include` from `cool.toml`
- `cool release [--bump patch|minor|major]` — version bump + bundle + git tag
- Semver constraint checking in `cool install` — `^`, `~`, `>=`, `>=,<`, `=`, `*` against installed versions, recorded in lockfile

#### Developer Tooling

- `cool ast <file.cool>` — pretty-printed JSON AST dump
- `cool inspect <file.cool>` — JSON summary of top-level imports and symbols
- `cool symbols [file.cool]` — resolved JSON symbol index across reachable modules
- `cool diff <before.cool> <after.cool>` — JSON summary of added, removed, and changed top-level symbols
- `cool modulegraph <file.cool>` — resolved import-graph inspection across project sources and dependencies
- `cool check [file.cool]` — static unresolved-import, import-cycle, and duplicate-symbol diagnostics (`--json` flag for machine-readable output)

#### Applications

- `browse` — TUI file browser (`coolapps/browse.cool`): two-pane layout, directory traversal, file preview, arrow-key navigation, written entirely in Cool

## [1.0.0] - 2026-04-17 - The Complete Language

Cool now has a working interpreter, bytecode VM, LLVM backend, FFI, a self-hosted compiler, full bootstrap self-hosting for `coolc/compiler_vm.cool`, and a steadily growing standard library. That library now includes cross-runtime `csv`, `datetime`, `hashlib`, `toml`, `yaml`, `sqlite`, `http`, `argparse`, `logging`, and `test`, plus native LLVM `try` / `except` / `finally` / `raise` support with matching `with` / context-manager cleanup through caught exceptions. The first dedicated systems-language checkpoint is also in place now: fixed-width integer helpers (`i8` / `u8` / `i16` / `u16` / `i32` / `u32` / `i64`) and wider LLVM raw-memory reads/writes for 8/16/32-bit values.

### Phase 1 - Core Interpreter (Complete)

The foundational tree-walk interpreter.

- [x] Lexer with tokens, indentation, INDENT/DEDENT handling
- [x] Recursive descent parser producing AST
- [x] Variables, assignment, augmented assignment (`+=`, `-=`, etc.)
- [x] All primitive types: integers, floats, strings, booleans, nil
- [x] Arithmetic operators (`+`, `-`, `*`, `/`, `%`)
- [x] Comparison operators (`==`, `!=`, `<`, `<=`, `>`, `>=`)
- [x] Logical operators (`and`, `or`, `not`)
- [x] Control flow: `if` / `elif` / `else`
- [x] Loops: `while`, `for`
- [x] Loop control: `break` / `continue`
- [x] Functions: `def`, `return`
- [x] Closures (functions capture their scope)
- [x] Collections: Lists, Dicts with full method support
- [x] Built-in functions: `print()`, `input()`, `str()`, `int()`, `float()`, `len()`
- [x] Multi-line strings (triple quotes)
- [x] Comments (`#`)

### Phase 2 - Real Language Features (Complete)

Enough features to write real programs.

- [x] Classes (`class`, `__init__`, methods, `self`)
- [x] Inheritance (`class Dog(Animal)`)
- [x] `isinstance()` built-in
- [x] Exception handling: `try` / `except` / `else` / `finally`
- [x] Exception raising: `raise`
- [x] Exception propagation through function calls
- [x] Tuples (create, index, iterate, unpack)
- [x] Tuple unpacking (`a, b = (1, 2)`)
- [x] `in` / `not in` operator
- [x] Default parameters
- [x] `*args` (variadic functions)
- [x] Keyword arguments at call site
- [x] Standard library imports: `math`, `os`, `sys`
- [x] File I/O: `open`, `read`, `write`, `readlines`, `close`
- [x] `**` power operator, `//` floor division
- [x] `string.format()`
- [x] Bitwise operators (`&`, `|`, `^`, `~`, `<<`, `>>`)
- [x] Hex / binary / octal literals (`0xFF`, `0b1010`, `0o777`)
- [x] `\x` escape sequences in strings
- [x] Slicing (`lst[1:3]`, negative indices)
- [x] Multi-line collection literals
- [x] `runfile()` built-in

### Phase 3 - Cool Shell (Complete)

A working interactive shell written entirely in Cool.

- [x] ASCII banner on startup
- [x] `help` command
- [x] File system: `pwd`, `ls`, `cd`, `cat`, `mkdir`, `touch`, `rm`, `mv`
- [x] Text output: `echo`, `write`
- [x] Script execution: `run`, `history`, `clear`, `exit`

### Phase 4 - Quality of Life (Complete)

Features that make the language pleasant to use.

- [x] f-strings (`f"Hello {name}!"`)
- [x] `nonlocal` / `global` keywords
- [x] Lambda expressions (`lambda x: x * 2`)
- [x] Ternary expression (`x if condition else y`)
- [x] List comprehensions (`[x * 2 for x in items]`)
- [x] `assert` statement
- [x] Context managers (`with open(...) as f`)
- [x] `super()` for calling parent methods
- [x] Operator overloading (`__add__`, `__str__`, `__eq__`, `__len__`, etc.)
- [x] Type constructors: `list()`, `tuple()`, `dict()`, `set()`
- [x] Better error messages (line + column + source snippet)
- [x] `**kwargs` support
- [x] Multiline expressions with `\`
- [x] Functional helpers: `sorted()`, `reversed()`, `enumerate()`, `zip()`
- [x] `map()`, `filter()` built-ins
- [x] Utility built-ins: `type()`, `repr()`, `abs()`, `min()`, `max()`, `sum()`
- [x] Reflection: `hasattr()`, `getattr()`
- [x] String methods: `.upper()`, `.lower()`, `.strip()`, `.split()`, `.replace()`, `.find()`, `.count()`, `.startswith()`, `.endswith()`

### Phase 5 - Shell: More Commands (Complete)

A shell powerful enough for real use.

- [x] `cp` — copy files
- [x] `grep` — search file contents
- [x] `head` / `tail` — first/last N lines
- [x] `wc` — word/line/char count
- [x] `find` — search for files by name
- [x] Pipes: `ls | grep cool`
- [x] Environment variables (`set VAR=value`, `$VAR`)
- [x] Tab completion
- [x] Up-arrow history navigation
- [x] Shell scripting (`source <file>`)
- [x] `alias` command

### Phase 6 - Standard Library (Complete)

A built-in library shipped with the language across runtimes.

- [x] `string` module — `split`, `join`, `strip`, `upper`, `lower`, `replace`, etc.
- [x] `list` module — `sort`, `reverse`, `map`, `filter`, `reduce`, `flatten`, `unique`
- [x] `math` module (expanded) — `gcd`, `lcm`, `factorial`, `hypot`, `degrees`, `radians`, `sinh`, `cosh`, `tanh`, etc.
- [x] `json` module — `loads` / `dumps` with full JSON support
- [x] `re` module — `match`, `search`, `fullmatch`, `findall`, `sub`, `split`
- [x] `time` module — `time()`, `sleep()`, `monotonic()`
- [x] `random` module — `random()`, `randint()`, `choice()`, `shuffle()`, `uniform()`, `seed()`
- [x] `collections` module — `Queue` and `Stack` classes
- [x] `csv` module — row parsing, header-based dict parsing, and CSV writing
- [x] `datetime` module — local timestamps, formatting/parsing, parts, and duration helpers
- [x] `hashlib` module — `md5`, `sha1`, `sha256`, and digest helpers
- [x] `toml` module — `loads` / `dumps` helpers for tables, arrays, strings, numbers, and booleans
- [x] `yaml` module — `loads` / `dumps` for a config-oriented YAML subset
- [x] `sqlite` module — path-based embedded database access with `execute`, `query`, and `scalar`
- [x] `http` module — `get`, `post`, `head`, and `getjson` helpers backed by host `curl`
- [x] `argparse` module — positional/flag parsing and generated help text
- [x] `logging` module — leveled logs, timestamps, formatters, and file/stdout handlers
- [x] `test` module — in-language assertions and `test.raises()` helpers across runtimes
- [x] Package system — `import foo.bar` loads `foo/bar.cool`

### Phase 7 - Cool Applications (Complete)

Real applications written entirely in Cool.

- [x] `calc` — Calculator REPL with persistent variables
- [x] `notes` — Note-taking app (new, show, append, delete, search)
- [x] `top` — Process/task viewer
- [x] `edit` — Nano-like text editor (arrow keys, Ctrl+S, Ctrl+X)
- [x] `snake` — ASCII Snake game with real-time input
- [x] `http` — HTTP client (`get`, `post`, `head`, `getjson`)

### Phase 8 - Compiler (Complete)

Bytecode VM and LLVM backend for native binaries.

- [x] Bytecode VM (compile AST to bytecode, run on VM)
- [x] LLVM backend (compile Cool → LLVM IR → native binary)
- [x] FFI (`import ffi` — load shared libs, call C functions)
- [x] `cool build` command (compile to native binary)
- [x] `cool new` command (scaffold new projects with `cool.toml`)
- [x] `cool test` command (discover and run `test_*.cool` / `*_test.cool` files with interpreter, VM, or native runners)
- [x] Inline assembly (`asm("template")`)
- [x] Pointer types / raw memory access (`malloc`, `free`, `read_i64`, `write_i64`)
- [x] Lists in LLVM
- [x] `for` loops in LLVM
- [x] `range()` in LLVM
- [x] `len()` in LLVM
- [x] List concatenation in LLVM
- [x] Function calls in LLVM
- [x] Recursion in LLVM
- [x] Variable assignment with expressions
- [x] **Classes in LLVM** (`class`, `__init__`, methods, attribute access)

### Phase 9 - Self-Hosted Compiler (Complete)

The compiler written in Cool itself.

- [x] Lexer in Cool (`coolc/compiler_vm.cool`)
- [x] Recursive descent parser in Cool
- [x] Code generator (AST → bytecode) in Cool
- [x] Bytecode VM in Cool (to execute compiled programs)
- [x] Bootstrap: self-hosted compiler compiles itself

---

## [0.9.0] - Pre-release

### Added

- Initial project structure
- Basic interpreter implementation
- REPL support

---

## Migration Notes

### From v0.x to v1.0

The `Cool/` directory has been renamed to `coolapps/`. Update your commands:

```bash
# Old (deprecated)
cool Cool/shell.cool
run Cool/snake.cool

# New
cool coolapps/shell.cool
run coolapps/snake.cool
```

The interpreter and bytecode VM now share full context-manager cleanup semantics, and the LLVM backend also covers default/keyword arguments, inheritance, `super()`, slicing, `str()`, `isinstance()`, `try` / `except` / `else` / `finally`, `raise`, helpers like `min()`, `max()`, `sum()`, `round()`, `sorted()`, `abs()`, `int()`, `float()`, `bool()`, source-relative file imports like `import "helper.cool"`, project/package imports like `import foo.bar`, native `import ffi` (`ffi.open`, `ffi.func`), built-in `import math` / `import os` / `import sys` / `import path` / `import csv` / `import datetime` / `import hashlib` / `import toml` / `import yaml` / `import sqlite` / `import http` / `import subprocess` / `import argparse` / `import logging` / `import test` / `import time`, the core `random` helpers (`seed`, `random`, `randint`, `uniform`, `choice`, `shuffle`), `json.loads()` / `json.dumps()`, the built-in `string` helpers (`split`, `join`, `strip`, `lstrip`, `rstrip`, `upper`, `lower`, `replace`, `startswith`, `endswith`, `find`, `count`, `title`, `capitalize`, `format`), the pure `list` helpers (`sort`, `reverse`, `map`, `filter`, `reduce`, `flatten`, `unique`), the `re` helpers (`match`, `search`, `fullmatch`, `findall`, `sub`, `split`), `collections.Queue()` / `collections.Stack()`, native `open()` / file methods, and `with` / context managers on normal exit, control-flow exits (`return`, `break`, `continue`), caught exceptions, and unhandled native raises, but it still has some limitations:

| Feature | Interpreter | Bytecode VM | LLVM |
| ------- | ----------- | ----------- | ---- |
| Classes | ✅ | ✅ | ✅ |
| `with` / context managers (normal/control-flow exits, caught exceptions, and unhandled native raises) | ✅ | ✅ | ✅ |
| Closures / lambdas | ✅ | ✅ | ❌ |
| `while` loops | ✅ | ✅ | ✅ |
| General `import` | ✅ | ✅ | ✅ |
| `try` / `except` / `finally` / `raise` | ✅ | ✅ | ✅ |
| FFI (`import ffi`) | ✅ | ❌ | ✅ |
| Inline asm | ❌ | ❌ | ✅ |

---

[1.0.0]: https://github.com/codenz92/cool-lang/releases/tag/v1.0.0
