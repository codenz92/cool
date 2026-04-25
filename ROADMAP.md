# Cool Language Roadmap

## Direction

> North star: Cool should be known as a native-first, high-level systems language — not as a scripting language with extra backends.

### Product Position

- Primary identity: compiled language for real software, with interpreter/VM modes as development tools and compatibility layers
- Unique bet: one language across interpreter, VM, native binary, and freestanding object output
- Competitive edge: Python-level readability with stronger deployment, ABI, layout, and systems reach

### Roadmap Rules

- Backend parity is a feature: semantics must stay aligned across interpreter, VM, native, and freestanding subsets whenever the language surface overlaps
- Native-first work wins priority over shell-first work
- Compile-time structure, diagnostics, and tooling matter more than novelty syntax
- Systems interop must feel first-class, not bolted on
- Cool should not chase full Python compatibility as a goal in itself

### Current Critical Path

1. Finish the freestanding systems foundation
2. Add a real static semantic core for ordinary `def`/module code
3. Add typed language features that make large compiled programs pleasant and safe
4. Harden the native toolchain so shipping Cool binaries feels routine
5. Turn systems interop into a signature advantage (`bindgen`, ABI, targets, link flow)

## Legend

- [x] Done
- [~] Partial / in progress
- [ ] Not started

---

## Phase 1 — Core Interpreter ✅

> Goal: a working tree-walk interpreter that can run real programs

- [x] Lexer (tokens, indentation, INDENT/DEDENT)
- [x] Recursive descent parser (AST)
- [x] Variables, assignment, augmented assignment (`+=`, `-=`, etc.)
- [x] Integers, floats, strings, booleans, nil
- [x] Arithmetic operators (`+`, `-`, `*`, `/`, `%`)
- [x] Comparison operators (`==`, `!=`, `<`, `<=`, `>`, `>=`)
- [x] Logical operators (`and`, `or`, `not`)
- [x] `if` / `elif` / `else`
- [x] `while` loops
- [x] `for` loops
- [x] `break` / `continue`
- [x] Functions (`def`, `return`)
- [x] Closures (functions capture their scope)
- [x] Lists (create, index, append, pop, len, sort, etc.)
- [x] Dicts (create, index, contains, keys, values, items)
- [x] `print()`, `input()`, `str()`, `int()`, `float()`, `len()`
- [x] Multi-line strings (triple quotes)
- [x] Comments (`#`)

---

## Phase 2 — Real Language Features ✅

> Goal: enough features to write real programs

- [x] Classes (`class`, `__init__`, methods, `self`)
- [x] Inheritance (`class Dog(Animal)`)
- [x] `isinstance()`
- [x] `try` / `except` / `else` / `finally`
- [x] `raise` (exceptions)
- [x] Exception propagation through function calls
- [x] Tuples (create, index, iterate, unpack)
- [x] Tuple unpacking (`a, b = (1, 2)`)
- [x] `in` / `not in` operator (lists, tuples, strings, dicts)
- [x] Default parameters (`def greet(name, greeting="Hello")`)
- [x] `*args` (variadic functions)
- [x] Keyword arguments (`greet("Jamie", greeting="Hi")`)
- [x] `import math` (sqrt, floor, ceil, pi, pow, etc.)
- [x] `import os` (listdir, mkdir, remove, rename, exists, getenv, getcwd, join, popen)
- [x] `import sys` (argv, exit)
- [x] File I/O (`open`, `read`, `write`, `readlines`, `close`, `append` mode)
- [x] `**` power operator
- [x] `//` floor division
- [x] `string.format()` (`"Hello {}".format(name)`)
- [x] Bitwise operators (`&`, `|`, `^`, `~`, `<<`, `>>`)
- [x] Hex / binary / octal literals (`0xFF`, `0b1010`, `0o777`)
- [x] `\x` escape sequences in strings (`"\x1b"` for ANSI)
- [x] Slicing (`lst[1:3]`, `s[2:]`, `s[:5]`, negative indices)
- [x] Multi-line collection literals (dicts/lists/tuples spanning multiple lines)
- [x] `runfile()` built-in (run a `.cool` file from Cool code)

---

## Phase 3 — Cool Shell ✅

> Goal: a working interactive shell written entirely in Cool

- [x] ASCII banner on startup
- [x] `help` — list all commands
- [x] `pwd` — print working directory
- [x] `ls [path]` — list directory contents
- [x] `cd <path>` — change directory
- [x] `cat <file>` — print file contents
- [x] `mkdir <dir>` — create directory
- [x] `touch <file>` — create empty file
- [x] `rm <file>` — delete a file
- [x] `mv <src> <dst>` — move/rename a file
- [x] `echo <text>` — print text
- [x] `write <file> <text>` — write text to file
- [x] `run <file.cool>` — run a Cool program from inside the shell
- [x] `history` — show command history
- [x] `clear` — clear screen (ANSI escape)
- [x] `exit` / `quit` — exit the shell

---

## Phase 4 — Quality of Life ✅

> Goal: remove rough edges, make the language more pleasant to use

- [x] f-strings (`f"Hello {name}!"`)
- [x] `nonlocal` / `global` keywords
- [x] `lambda` expressions (`lambda x: x * 2`)
- [x] Ternary expression (`x if condition else y`)
- [x] List comprehensions (`[x * 2 for x in items]`)
- [x] `assert` statement
- [x] `with` statement / context managers (`with open(...) as f`)
- [x] `super()` for calling parent class methods
- [x] Operator overloading (`__add__`, `__str__`, `__eq__`, `__len__`, etc.)
- [x] `list()`, `tuple()`, `dict()`, `set()` built-in type constructors
- [x] Better error messages (show line + column + source snippet)
- [x] `**kwargs` support
- [x] Multiline expressions with `\` continuation
- [x] `sorted()`, `reversed()`, `enumerate()`, `zip()` built-ins
- [x] `map()`, `filter()` built-ins
- [x] `type()`, `repr()`, `abs()`, `min()`, `max()`, `sum()` built-ins
- [x] `hasattr()`, `getattr()` built-ins
- [x] String methods: `.upper()`, `.lower()`, `.strip()`, `.lstrip()`, `.rstrip()`, `.split()`, `.replace()`, `.find()`, `.count()`, `.startswith()`, `.endswith()`

---

## Phase 5 — Shell: More Commands ✅

> Goal: a shell powerful enough for real use

- [x] `cp <src> <dst>` — copy a file
- [x] `grep <pattern> <file>` — search file contents
- [x] `head <file> [n]` / `tail <file> [n]` — first/last N lines
- [x] `wc <file>` — word/line/char count
- [x] `find <pattern>` — search for files by name
- [x] Pipes: `ls | grep cool`
- [x] Environment variables (`set VAR=value`, `$VAR`)
- [x] Tab completion in interactive TTY shell sessions
- [x] Up-arrow history navigation in interactive TTY shell sessions
- [x] Shell scripting (`source <file>` runs shell scripts line by line)
- [x] `alias` command

---

## Phase 6 — Standard Library ✅

> Goal: a practical built-in library shipped with the language across runtimes

- [x] `string` module — `split`, `join`, `strip`, `upper`, `lower`, `replace`, etc.
- [x] `list` module — `sort`, `reverse`, `map`, `filter`, `reduce`, `flatten`, `unique`
- [x] `math` module (expanded) — `gcd`, `lcm`, `factorial`, `hypot`, `degrees`, `radians`, `sinh`, `cosh`, `tanh`, etc.
- [x] `json` module — `json.loads()` / `json.dumps()` with full JSON support
- [x] `re` module — `re.match()`, `re.search()`, `re.fullmatch()`, `re.findall()`, `re.sub()`, `re.split()`
- [x] `time` module — `time.time()`, `time.sleep()`, `time.monotonic()`
- [x] `random` module — `random.random()`, `random.randint()`, `random.choice()`, `random.shuffle()`, `random.uniform()`, `random.seed()`
- [x] `collections` module — `Queue` and `Stack` classes
- [x] `sqlite` module — path-based embedded database access with `execute`, `query`, and `scalar`
- [x] Package system — `import foo.bar` loads `foo/bar.cool` from source directory

### Next Library Targets

#### Data And Serialization

- [x] `csv` module — CSV reader/writer helpers for rows, header-based dicts, and basic quoting/escaping
- [x] `hashlib` module — `md5`, `sha1`, `sha256`, and digest helpers
- [x] `toml` module — parse and write TOML for project/config tooling
- [x] `yaml` module — config-oriented YAML subset for mappings, sequences, scalars, and null values
- [x] `sqlite` module — path-based embedded database access with queries, params, and scalar reads
- [ ] `json` extensions — schema-aware JSON transforms and streaming helpers
- [ ] `xml` module — lightweight XML parsing and serialization helpers
- [ ] `html` module — escaping/unescaping plus small DOM/text extraction helpers
- [ ] `base64` module — base64 encode/decode for strings and bytes-like data
- [ ] `codec` module — pluggable encoders/decoders for text and binary formats
- [ ] `bytes` module — byte strings, hex helpers, slicing, and binary encoding utilities
- [ ] `unicode` module — code point categories, normalization, width, and grapheme helpers
- [ ] `locale` module — locale-aware formatting, parsing, and language/region helpers
- [ ] `config` module — `.json`, `.ini`, and `.env` style configuration loading helpers
- [ ] `schema` module — typed validation rules for dicts, lists, configs, and API payloads

#### Filesystem And OS

- [x] `path` module — path normalization, basename/dirname, extension helpers, and path splitting
- [ ] `glob` module — wildcard path matching and recursive file discovery
- [ ] `tempfile` module — temporary files/directories with cleanup helpers
- [ ] `fswatch` module — file watching for rebuild loops, editors, and automation
- [ ] `process` module — PID info, signals, environment inspection, and runtime metadata
- [x] `platform` module — OS/arch/runtime detection and host capability helpers
- [x] `subprocess` module — structured process spawning, exit codes, stdout/stderr capture
- [ ] `daemon` module — service lifecycle helpers, PID files, logs, and restart policies
- [ ] `sandbox` module — constrained command/file execution helpers for safer automation
- [ ] `sync` module — file/state synchronization, conflict detection, and reconciliation helpers
- [ ] `store` module — key-value persistence, namespaces, and transactional update helpers

#### Networking And Services

- [x] `http` module — `get`, `post`, `head`, and `getjson` request helpers across runtimes (requires host `curl`)
- [ ] `socket` module — TCP/UDP clients and servers for networking work
- [ ] `websocket` module — client/server websocket support for realtime tools and apps
- [ ] `rpc` module — lightweight RPC protocol helpers, stubs, and request routing
- [ ] `graphql` module — query building, schema helpers, and response extraction
- [ ] `url` module — URL parsing, joining, query-string encode/decode, and percent escaping
- [ ] `mail` module — SMTP/IMAP-style helpers for notifications and inbox workflows
- [ ] `feed` module — RSS/Atom parsing, polling, deduplication, and feed generation
- [ ] `calendar` module — recurring schedules, reminders, and date-range planning helpers
- [ ] `cluster` module — multi-node coordination primitives for distributed experiments

#### Databases And Storage

- [x] `sqlite` module — embedded database access with queries, params, and row iteration
- [ ] `cache` module — in-memory and disk-backed caching with TTL and invalidation helpers
- [ ] `memo` module — function memoization and deterministic result caching
- [ ] `package` module — package metadata, manifests, semver helpers, and dependency resolution
- [ ] `bundle` module — single-file app bundling, asset embedding, and deploy packaging
- [ ] `archive` module — higher-level project/archive packaging on top of compress primitives
- [ ] `compress` module — gzip/zip/tar helpers for archives and packaged assets

#### Parsing, Language, And Tooling

- [x] `argparse` module — command-line flag parsing, positional args, and help generation
- [x] `logging` module — leveled logs, timestamps, formatters, and file/stdout handlers
- [ ] `doc` module — markdown, manpage, and HTML document generation helpers
- [ ] `template` module — string/file templating with variables, loops, and partials
- [ ] `parser` module — parser combinators and token-stream helpers for DSLs
- [ ] `lexer` module — token definitions, scanners, and syntax-highlighting support
- [ ] `ast` module — parse Cool source into inspectable AST nodes for tooling and linters
- [ ] `inspect` module — runtime inspection for modules, classes, functions, and objects
- [ ] `diff` module — text/line diffing, patches, and merge-assist primitives
- [ ] `patch` module — unified diff creation/application and file patch tooling
- [ ] `project` module — project scaffolding, manifests, templates, and workspace metadata
- [ ] `release` module — changelog generation, tagging, artifact assembly, and publish workflows
- [ ] `repo` module — git-aware repository inspection, diff/status helpers, and branch metadata
- [ ] `modulegraph` module — import graph inspection, cycle detection, and dependency visualization
- [ ] `plugin` module — plugin discovery, registration, lifecycle hooks, and capability loading
- [ ] `lsp` module — language-server protocol messages, diagnostics, completions, and tooling support
- [ ] `ffiutil` module — FFI signatures, type marshaling helpers, and safe wrapper generation
- [ ] `shell` module — shell parsing, quoting, completion, aliases, and script execution helpers

#### Runtime, Automation, And Observability

- [ ] `jobs` module — background jobs, worker pools, queues, and task orchestration helpers
- [ ] `event` module — pub/sub events, listeners, timers, and message buses
- [ ] `workflow` module — step graphs, checkpoints, resumability, and automation composition
- [ ] `agent` module — task/plan/executor primitives for autonomous tool workflows in Cool
- [ ] `retry` module — retry policies, backoff, jitter, and failure classification
- [ ] `metrics` module — counters, timers, histograms, and lightweight instrumentation
- [ ] `trace` module — spans, trace IDs, and execution tracing helpers
- [ ] `profile` module — runtime profiling hooks, flame summaries, and hotspot reporting
- [x] `test` module — assertions, fixtures, discovery helpers, and a standard unit-test API
- [ ] `bench` module — lightweight benchmarking helpers for timing and comparison
- [ ] `notebook` module — executable notes, cells, saved outputs, and literate-programming helpers
- [ ] `secrets` module — secret lookup, redaction, encrypted storage, and runtime injection

#### Math, Data Science, And Finance

- [x] `datetime` module — timestamps, local date formatting/parsing, and duration helpers
- [ ] `decimal` module — exact decimal arithmetic for finance and configuration math
- [ ] `money` module — decimal-safe currency values, formatting, and exchange abstractions
- [ ] `stats` module — descriptive statistics, sampling, percentiles, and distributions
- [ ] `vector` module — geometric vectors, transforms, and numeric helper operations
- [ ] `matrix` module — small matrix math for graphics, tools, and simulation work
- [ ] `geom` module — rectangles, points, intersections, bounds, and spatial utilities
- [ ] `graph` module — graph nodes/edges, traversal, shortest path, DAG utilities
- [ ] `tree` module — generic tree traversal, mutation, and query helpers
- [ ] `pipeline` module — composable data pipelines and stream-style transformations
- [ ] `stream` module — lazy iterators, generators, adapters, and chunked processing helpers
- [ ] `table` module — tabular display, sorting, formatting, and CSV/console rendering helpers
- [ ] `search` module — indexing, query parsing, scoring, and local search helpers
- [ ] `embed` module — vector embeddings, similarity search hooks, and semantic indexing helpers
- [ ] `ml` module — lightweight inference wrappers and data preprocessing primitives

#### Security And Crypto

- [x] `hashlib` module — `md5`, `sha1`, `sha256`, and digest helpers
- [ ] `crypto` module — symmetric encryption, signatures, random bytes, and key helpers

#### Terminal, UI, And Presentation

- [ ] `ansi` module — terminal colors, cursor movement, box drawing, and styling helpers
- [~] `term` module — raw terminal mode, key events, mouse events, and screen buffers (runtime parity now covers interpreter / VM / LLVM for raw mode, cursor control, sizing, and key input; mouse and richer screen buffers still open)
- [ ] `tui` module — higher-level terminal UI widgets, layout, focus, and event loops
- [ ] `theme` module — reusable palettes, spacing scales, and text-style presets for TUIs
- [ ] `color` module — RGB/HSL/HSV conversion, palettes, gradients, and contrast helpers
- [ ] `scene` module — lightweight scene graphs for TUI/ASCII/game applications

#### Media And Game Development

- [ ] `image` module — image metadata, resize/crop helpers, and simple format conversion
- [ ] `audio` module — WAV/PCM helpers, metadata, and lightweight processing primitives
- [ ] `sprite` module — tiny 2D sprite sheets, tiles, and ASCII/pixel animation helpers
- [ ] `game` module — timers, entities, input state, collision helpers, and main-loop support

---

## Phase 7 — Cool Applications ✅

> Goal: write real apps entirely in Cool

- [x] `calc` — calculator REPL with persistent variables, full math library support
- [x] `notes` — note-taking app (new, show, append, delete, search commands)
- [x] `top` — process/task viewer using `ps aux` and system stats (interactive TTY app)
- [x] `edit` — nano-like text editor (arrow keys, Ctrl+S save, Ctrl+X exit, interactive TTY app)
- [x] `snake` — Snake game (ASCII, arrow keys, real-time with raw terminal mode, interactive TTY app)
- [x] `http` — HTTP client (`http get/post/head/getjson <url>`) backed by curl

---

## Phase 8 — Compiler ✅

> Goal: compile Cool to native binaries

- [x] Bytecode VM (compile AST to bytecode, run on a VM)
- [x] LLVM backend (compile Cool to LLVM IR → native binary via embedded C runtime)
- [x] FFI (`import ffi` — load shared libs, call C functions from Cool)
- [x] `cool build` command (compile a `.cool` project to a native binary)
- [x] `cool new` command (scaffold a new Cool project with `cool.toml`)
- [x] Inline assembly (`asm("template")`) — LLVM only
- [x] Raw memory access (`malloc`, `free`, `read_i64`, `write_i64`, etc.) — LLVM only
- [x] Lists in LLVM
- [x] `for` loops in LLVM
- [x] `range()` in LLVM
- [x] `len()` in LLVM
- [x] List concatenation in LLVM
- [x] Functions and recursion in LLVM
- [x] Classes in LLVM (`class`, `__init__`, methods, attribute access)
- [x] Ternary expressions in LLVM (`x if cond else y`)
- [x] List comprehensions in LLVM (`[expr for x in iter if cond]`)
- [x] `in` / `not in` in LLVM (lists and strings)
- [x] Dicts in LLVM (`{k: v}`, `d[k]`, `d[k] = v`, `k in d`, `len(d)`)
- [x] Tuples in LLVM (literals, indexing, unpacking, `in`/`not in`, `len()`)

### Known LLVM Limitations

The LLVM backend now covers most day-to-day language features, including default/keyword arguments, inheritance, `super()`, slicing, `str()`, `isinstance()`, `try` / `except` / `else` / `finally`, `raise`, helpers like `min()`, `max()`, `sum()`, `round()`, `sorted()`, `abs()`, `int()`, `float()`, `bool()`, source-relative file imports like `import "helper.cool"`, project/package imports like `import foo.bar`, native `import ffi` (`ffi.open`, `ffi.func`), built-in `import math` / `import os` / `import sys` / `import path` / `import csv` / `import datetime` / `import hashlib` / `import toml` / `import yaml` / `import sqlite` / `import http` / `import subprocess` / `import argparse` / `import logging` / `import test` / `import time`, the core `random` helpers (`seed`, `random`, `randint`, `uniform`, `choice`, `shuffle`), `json.loads()` / `json.dumps()`, the built-in `string` helpers (`split`, `join`, `strip`, `lstrip`, `rstrip`, `upper`, `lower`, `replace`, `startswith`, `endswith`, `find`, `count`, `title`, `capitalize`, `format`), the pure `list` helpers (`sort`, `reverse`, `map`, `filter`, `reduce`, `flatten`, `unique`), the `re` helpers (`match`, `search`, `fullmatch`, `findall`, `sub`, `split`), `collections.Queue()` / `collections.Stack()`, native `open()` / file methods, and `with` / context managers on normal exit, control-flow exits (`return`, `break`, `continue`), caught exceptions, and unhandled native raises. The following features still have notable gaps in LLVM:

| Feature | Interpreter | Bytecode VM | LLVM |
| ------- | :-----------: | :-----------: | :----: |
| Classes | ✅ | ✅ | ✅ |
| Ternary expressions | ✅ | ✅ | ✅ |
| List comprehensions | ✅ | ✅ | ✅ |
| `in` / `not in` | ✅ | ✅ | ✅ |
| Dicts | ✅ | ✅ | ✅ |
| Tuples | ✅ | ✅ | ✅ |
| Closures / lambdas | ✅ | ✅ | ❌ |
| General `import` | ✅ | ✅ | ✅ |
| `import ffi` | ✅ | ❌ | ✅ |
| Inline assembly | ❌ | ❌ | ✅ |
| Raw memory | ❌ | ❌ | ✅ |

---

## Phase 9 — Self-Hosted Compiler ✅ Complete

> Goal: write the Cool compiler in Cool itself, capable of compiling real Cool programs

The self-hosted compiler lives in `coolc/compiler_vm.cool`. It includes a full lexer, recursive descent parser, code generator, and bytecode VM — all written in Cool. It can compile and execute a substantial subset of the Cool language.

### What works

- [x] Lexer — identifiers, numbers, strings, operators, multi-char ops
- [x] Recursive descent parser with correct operator precedence
- [x] Code generator (AST → bytecode)
- [x] Bytecode VM that executes the compiled output
- [x] `print(<expr>)`
- [x] Variable assignment (`x = 1`)
- [x] Arithmetic (`+`, `-`, `*`, `/`)
- [x] Comparison operators (`==`, `!=`, `<`, `>`, `<=`, `>=`)
- [x] Lists (`[1, 2, 3]`)
- [x] Strings
- [x] Multi-statement programs
- [x] Indentation / INDENT / DEDENT handling in the self-hosted lexer
- [x] `if` / `elif` / `else` with compileable bodies
- [x] `while` and `for` loops with compileable bodies
- [x] `break` / `continue` (jump patching)
- [x] `def` with a real function body and call frames
- [x] `return` values
- [x] Closures / upvalue capture
- [x] Classes and method dispatch
- [x] Full test suite: arithmetic, variables, if/elif/else, while, for, break/continue, functions, closures, lists, classes, inheritance, FizzBuzz

### Self-hosting achievement

- [x] Bootstrap: compiles `compiler_vm.cool` with itself (full self-hosting)

---

## Phase 10 — Production Readiness And Ecosystem ✅ Complete

> Goal: make Cool feel default for real applications, not just impressive for demos and experiments

### Runtime Parity

- [x] Bytecode VM: full `with` / context-manager cleanup semantics, including `return`, `break`, `continue`, and exceptions
- [x] LLVM: custom `with` / context managers on normal exit and control-flow exits (`return`, `break`, `continue`)
- [x] LLVM: native `open()` / file methods and `with open(...)` on normal exit and control-flow exits
- [x] LLVM: `with` / context-manager unwinding for caught and unhandled native exceptions
- [x] LLVM: `try` / `except` / `finally` / `raise`
- [x] LLVM: broader `import` support beyond built-in native modules
- [x] LLVM: `import ffi`

### First-Wave Library Modules

- [x] `path` module — path normalization, basename/dirname, extension helpers, splitting, and joins
- [x] `csv` module — row parsing, header-based dict parsing, and CSV writing
- [x] `datetime` module — local timestamps, formatting/parsing, parts, and duration helpers
- [x] `hashlib` module — `md5`, `sha1`, `sha256`, and digest helpers
- [x] `toml` module — `loads` / `dumps` helpers for tables, arrays, strings, numbers, and booleans
- [x] `yaml` module — `loads` / `dumps` for a config-oriented YAML subset
- [x] `sqlite` module — path-based embedded database access with `execute`, `query`, and `scalar`
- [x] `http` module — `get`, `post`, `head`, and `getjson` request helpers (requires host `curl`)
- [x] `subprocess` module — process spawning, exit codes, stdout/stderr capture, and timeouts
- [x] `argparse` module — positional/flag parsing, defaults, and generated help text
- [x] `logging` module — leveled logs, formatters, timestamps, and file/stdout handlers
- [x] `socket` module — TCP client (`connect`) and server (`listen`, `accept`) with `send`, `recv`, `readline`, and `close` across all runtimes

### Packaging And Developer Tooling

- [x] `cool test` command for discovered and explicit Cool test files, with interpreter / VM / native runner modes
- [x] Standard `test` module for in-language unit/integration helpers and assertions (`equal`, `not_equal`, `truthy`, `falsey`, `is_nil`, `not_nil`, `fail`, `raises`)
- [x] Package/dependency metadata beyond `cool.toml`, including manifests, lockfiles, path/git installs, and semver constraint checking (`^`, `~`, `>=`, `>=,<`, `=`, `*`)
- [x] App bundling / release tooling — `cool bundle` (build + distributable tarball with `[bundle].include` in cool.toml) and `cool release` (version bump + bundle + git tag)
- [x] AST / inspect / symbols / modulegraph / diff / check CLI helpers for tooling and static analysis
- [x] Language-server and editor tooling (`lsp`) — `cool lsp` stdio server: diagnostics, completions, hover, go-to-definition, document symbols, workspace symbols

### Flagship Cool Software

- [ ] A real package manager or project tool written in Cool
- [x] A build/task runner that demonstrates modules, subprocesses, and packaging
- [x] A flagship TUI or networked app — `browse` (TUI file browser with two-pane layout, directory traversal, file preview, arrow-key navigation, written entirely in Cool)

---

## Phase 11 — Freestanding Systems Foundation 🚧 In Progress

> Goal: move Cool toward bare-metal and kernel work with a deliberate systems subset, instead of treating OS support as just “more LLVM features”

### Numeric And Memory Primitives

- [x] Fixed-width integer helpers: `i8`, `u8`, `i16`, `u16`, `i32`, `u32`, `i64`
- [x] LLVM raw-memory reads/writes for signed and unsigned 8/16/32-bit values, alongside the existing byte and 64-bit helpers
- [x] Volatile read/write variants for MMIO and device-driver code
- [x] Pointer-width aliases and target word-size helpers

### Data Layout And ABI

- [x] `struct` definitions with typed fields (`i8`–`i64`, `u8`–`u64`, `f32`/`f64`, `bool`), positional + keyword construction, and coercion on init across all runtimes
- [x] Stable binary layout for structs (LLVM struct types, GEP-based field access, side-table dynamic dispatch for function-parameter path)
- [x] `packed` structs — `packed struct Name:` syntax, consecutive byte layout with no inter-field padding, LLVM packed attribute, GEP + side-table paths both honoured
- [x] `union` support — `union Name:` syntax, all fields share offset 0 (interpreter/VM: class lowering with zero defaults; LLVM: `[max_size x i8]` body, bitcast field access, GEP fast path)
- [x] `extern` declarations with calling-convention and symbol control
- [x] Linker-section placement for functions and data

### Freestanding Build Mode

- [x] `cool build --freestanding`
- [x] Object / kernel image output without libc assumptions
- [x] Explicit entry points for freestanding functions
- [x] Linker-script support (`--linker-script=<path>` CLI flag and `linker_script` in `cool.toml`; links via LLD to `.elf`)
- [x] Panic / abort strategy for no-host targets

### Core Systems Runtime

- [ ] `core` subset that avoids host OS facilities
- [x] Serial / console output primitives (`outb`, `inb`, `write_serial_byte` — x86 port I/O via inline asm, freestanding-safe)
- [ ] Memory-map and paging helpers
- [ ] Pluggable allocator hooks for kernels and runtimes

---

## Phase 12 — Static Semantic Core

> Goal: give Cool a disciplined compile-time spine so it reads like a high-level language but scales like a serious compiled one

### Declarations And Modules

- [x] Typed parameters and return types for ordinary `def`
- [ ] Typed local bindings and module-level constants
- [ ] Immutable bindings / constant declarations
- [ ] Explicit public/private module visibility
- [ ] Export surface rules and import validation for large projects

### Type Checking

- [ ] A real type checker for normal program code, not just FFI/data-layout declarations
- [ ] Assignability and coercion rules for primitives, structs, unions, and tuples
- [ ] Compile-time checking of function returns on all code paths
- [ ] Module-level symbol/type resolution before codegen
- [ ] `cool check --strict` / strict project mode

### Tooling Integration

- [ ] Type-aware LSP completions, hover, and diagnostics
- [ ] Typed `cool ast` / `inspect` output for tooling consumers
- [ ] Compiler diagnostics with fix suggestions for type and import issues

---

## Phase 13 — Typed Language Features

> Goal: make Cool comfortable for large native codebases, not just dynamic-style programs that happen to compile

### Core Language Features

- [ ] Enums / tagged unions / algebraic data types
- [ ] `match` with exhaustiveness checking
- [ ] `Option` / `Result` style standard types and language sugar where justified
- [ ] Generic functions
- [ ] Generic structs / enums / collections
- [ ] Traits / interfaces / protocols for shared behavior
- [ ] Trait bounds or equivalent constraints for generic code

### Collections And APIs

- [ ] Typed standard collections with clear generic surfaces
- [ ] Method/trait design that works consistently across interpreter, VM, and native builds
- [ ] Error-handling conventions for compiled application code

---

## Phase 14 — Runtime And Memory Model

> Goal: define the runtime semantics clearly enough that Cool is trusted for systems-facing native software

### Memory Semantics

- [ ] Choose and document the primary memory-management model for high-level values in native code
- [ ] Define ownership boundaries between Cool-managed values, raw memory, and FFI-owned memory
- [ ] Deterministic cleanup story for native resources beyond `with`
- [ ] Large-value move/copy/clone semantics
- [ ] Arena/region or other explicit allocator strategies where they make sense

### Runtime Profiles

- [ ] Clear hosted vs freestanding runtime profile documentation
- [ ] Stable `core`/`std` split for host-free builds
- [ ] Native panic / abort / diagnostics policy
- [ ] Thread/task safety rules for future concurrency features

---

## Phase 15 — Native Toolchain And Distribution

> Goal: make shipping Cool software feel more like shipping a serious compiled language product than running a script with extra steps

### Compiler And Build UX

- [ ] Incremental compilation
- [ ] Cross-compilation via explicit target triples
- [ ] Build profiles (`dev`, `release`, `freestanding`, and stricter checked modes)
- [ ] Reproducible builds and toolchain pinning
- [ ] Better binary/object/library output selection

### Debugging, Docs, And Observability

- [ ] Native debug info and better stack traces
- [ ] Profiling and benchmark tooling
- [ ] `cool fmt`
- [ ] First-class doc generation for modules, types, and APIs
- [ ] Better release artifact metadata and symbol maps

### Packaging

- [ ] Registry-quality package/distribution workflow
- [ ] Stronger lockfile and dependency reproducibility guarantees
- [ ] Project templates aimed at native apps, libraries, services, and freestanding targets

---

## Phase 16 — Systems Interop And Targets

> Goal: make Cool unusually strong at crossing the line between high-level application code and low-level native/system boundaries

### C / ABI Interop

- [ ] `cool bindgen` for C headers
- [ ] Richer ABI metadata for `extern def`
- [ ] Better native library loading/linking workflows
- [ ] Static library and shared library output
- [ ] Clear FFI ownership/lifetime annotations

### Targeting And Linking

- [ ] Linker-script support and explicit entry-point control
- [ ] Object / kernel image output without libc assumptions
- [ ] Target CPU / feature flags
- [ ] No-libc syscall/runtime support where appropriate
- [ ] Better section/link layout tooling for embedded/kernel use

### Systems Libraries

- [ ] Register/MMIO helpers built on the existing raw-memory primitives
- [ ] Safer pointer/address abstractions on top of raw integer addresses
- [ ] Host-independent `core` facilities for allocators, strings, formatting, and collections

---

## Phase 17 — Signature Features And Flagship Software

> Goal: give Cool a recognizable identity beyond “another compiled language” and prove it with software people care about

### Signature Capability

- [ ] Capability/permission model declared in `cool.toml` for file/network/env/process access
- [ ] Runtime enforcement and diagnostics for denied capabilities
- [ ] Capability-aware stdlib APIs and package metadata

### Concurrency

- [ ] Structured concurrency primitives
- [ ] Tasks, cancellation, deadlines, and channels
- [ ] Process/network orchestration APIs that compose with the stdlib
- [ ] Clear semantics across interpreter, VM, and native builds

### Flagship Software

- [ ] A real package manager / project tool written in Cool
- [ ] A substantial native CLI people would actually use outside the repo
- [ ] A flagship TUI or desktop-like terminal app that demonstrates the compiled language story
- [ ] A service/backend example large enough to stress the type system, tooling, and packaging
- [ ] A freestanding/kernel/boot-path demo that proves the systems subset is not theoretical

---

## Summary

| Phase | Status |
| ----- | ------ |
| 1 — Core Interpreter | ✅ Complete |
| 2 — Real Language Features | ✅ Complete |
| 3 — Cool Shell | ✅ Complete |
| 4 — Quality of Life | ✅ Complete |
| 5 — Shell: More Commands | ✅ Complete |
| 6 — Standard Library | ✅ Complete |
| 7 — Cool Applications | ✅ Complete |
| 8 — Compiler (bytecode VM + LLVM + FFI) | ✅ Complete |
| 9 — Self-Hosted Compiler | ✅ Complete |
| 10 — Production Readiness And Ecosystem | ✅ Complete |
| 11 — Freestanding Systems Foundation | 🚧 In Progress |
| 12 — Static Semantic Core | ⏳ Planned |
| 13 — Typed Language Features | ⏳ Planned |
| 14 — Runtime And Memory Model | ⏳ Planned |
| 15 — Native Toolchain And Distribution | ⏳ Planned |
| 16 — Systems Interop And Targets | ⏳ Planned |
| 17 — Signature Features And Flagship Software | ⏳ Planned |
