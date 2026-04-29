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

### Completed Critical Path

1. Freestanding systems foundation
2. Static semantic core for ordinary `def`/module code
3. Typed language features for larger compiled programs
4. Hardened native toolchain for routine binary shipping
5. Systems interop as a signature advantage (`bindgen`, ABI, targets, link flow)
6. Release gate and CI-backed quality checks for routine shipping

## Legend

- [x] Done
- [~] Partial / in progress
- [ ] Not started

---

## Phase 1 — Core Interpreter ✅ Complete

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

## Phase 2 — Real Language Features ✅ Complete

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

## Phase 3 — Cool Shell ✅ Complete

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

## Phase 4 — Quality of Life ✅ Complete

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

## Phase 5 — Shell: More Commands ✅ Complete

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

## Phase 6 — Standard Library ✅ Complete

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
- [x] `json` extensions — NDJSON line helpers plus JSON Pointer / schema-style transform helpers across interpreter, VM, and native builds
- [x] `xml` module — lightweight XML parsing, serialization, text extraction, and path helpers
- [x] `html` module — escaping/unescaping plus small DOM/text extraction helpers
- [x] `base64` module — base64 encode/decode for strings and bytes-like data
- [x] `codec` module — pluggable encoders/decoders for text and binary formats
- [x] `bytes` module — byte strings, hex helpers, slicing, and binary encoding utilities
- [x] `unicode` module — code point categories, normalization, width, and grapheme helpers
- [x] `locale` module — locale-aware formatting, parsing, and language/region helpers
- [x] `config` module — `.json`, `.ini`, and `.env` style configuration loading helpers
- [x] `schema` module — typed validation rules for dicts, lists, configs, and API payloads

#### Filesystem And OS

- [x] `path` module — path normalization, basename/dirname, extension helpers, and path splitting
- [x] `glob` module — wildcard path matching and recursive file discovery
- [x] `tempfile` module — temporary files/directories with cleanup helpers
- [x] `fswatch` module — file watching for rebuild loops, editors, and automation
- [x] `process` module — PID info, signals, environment inspection, and runtime metadata
- [x] `platform` module — OS/arch/runtime detection and host capability helpers
- [x] `subprocess` module — structured process spawning, exit codes, stdout/stderr capture
- [x] `daemon` module — service lifecycle helpers, PID files, logs, and restart policies
- [x] `sandbox` module — constrained command/file execution helpers for safer automation
- [x] `sync` module — file/state synchronization, conflict detection, and reconciliation helpers
- [x] `store` module — key-value persistence, namespaces, and transactional update helpers

#### Networking And Services

- [x] `http` module — `get`, `post`, `head`, and `getjson` request helpers across runtimes (requires host `curl`)
- [x] `socket` module — TCP/UDP clients and servers for networking work
- [x] `websocket` module — client/server websocket support for realtime tools and apps
- [x] `rpc` module — lightweight RPC protocol helpers, stubs, and request routing
- [x] `graphql` module — query building, schema helpers, and response extraction
- [x] `url` module — URL parsing, joining, query-string encode/decode, and percent escaping
- [x] `mail` module — SMTP/IMAP-style helpers for notifications and inbox workflows
- [x] `feed` module — RSS/Atom parsing, polling, deduplication, and feed generation
- [x] `calendar` module — recurring schedules, reminders, and date-range planning helpers
- [x] `cluster` module — multi-node coordination primitives for distributed experiments

#### Databases And Storage

- [x] `sqlite` module — embedded database access with queries, params, and row iteration
- [x] `cache` module — in-memory and disk-backed caching with TTL and invalidation helpers
- [x] `memo` module — function memoization and deterministic result caching
- [x] `package` module — package metadata, manifests, semver helpers, and dependency resolution
- [x] `bundle` module — single-file app bundling, asset embedding, and deploy packaging
- [x] `archive` module — higher-level project/archive packaging on top of compress primitives
- [x] `compress` module — gzip/zip/tar helpers for archives and packaged assets

#### Parsing, Language, And Tooling

- [x] `argparse` module — command-line flag parsing, positional args, and help generation
- [x] `logging` module — leveled logs, timestamps, formatters, and file/stdout handlers
- [x] `doc` module — markdown, manpage, and HTML document generation helpers
- [x] `template` module — string/file templating with variables, loops, and partials
- [x] `parser` module — parser combinators and token-stream helpers for DSLs
- [x] `lexer` module — token definitions, scanners, and syntax-highlighting support
- [x] `ast` module — parse Cool source into inspectable AST nodes for tooling and linters
- [x] `inspect` module — runtime inspection for modules, classes, functions, and objects
- [x] `diff` module — text/line diffing, patches, and merge-assist primitives
- [x] `patch` module — unified diff creation/application and file patch tooling
- [x] `project` module — project scaffolding, manifests, templates, and workspace metadata
- [x] `release` module — changelog generation, tagging, artifact assembly, and publish workflows
- [x] `repo` module — git-aware repository inspection, diff/status helpers, and branch metadata
- [x] `modulegraph` module — import graph inspection, cycle detection, and dependency visualization
- [x] `plugin` module — plugin discovery, registration, lifecycle hooks, and capability loading
- [x] `lsp` module — language-server protocol messages, diagnostics, completions, and tooling support
- [x] `ffiutil` module — FFI signatures, type marshaling helpers, and safe wrapper generation
- [x] `shell` module — shell parsing, quoting, completion, aliases, and script execution helpers

#### Runtime, Automation, And Observability

- [x] `jobs` module — background jobs, worker pools, queues, and task orchestration helpers
- [x] `event` module — pub/sub events, listeners, timers, and message buses
- [x] `workflow` module — step graphs, checkpoints, resumability, and automation composition
- [x] `agent` module — task/plan/executor primitives for autonomous tool workflows in Cool
- [x] `retry` module — retry policies, backoff, jitter, and failure classification
- [x] `metrics` module — counters, timers, histograms, and lightweight instrumentation
- [x] `trace` module — spans, trace IDs, and execution tracing helpers
- [x] `profile` module — runtime profiling hooks, flame summaries, and hotspot reporting
- [x] `test` module — assertions, fixtures, discovery helpers, and a standard unit-test API
- [x] `bench` module — lightweight benchmarking helpers for timing and comparison
- [x] `notebook` module — executable notes, cells, saved outputs, and literate-programming helpers
- [x] `secrets` module — secret lookup, redaction, encrypted storage, and runtime injection

#### Math, Data Science, And Finance

- [x] `datetime` module — timestamps, local date formatting/parsing, and duration helpers
- [x] `decimal` module — exact decimal arithmetic for finance and configuration math
- [x] `money` module — decimal-safe currency values, formatting, and exchange abstractions
- [x] `stats` module — descriptive statistics, sampling, percentiles, and distributions
- [x] `vector` module — geometric vectors, transforms, and numeric helper operations
- [x] `matrix` module — small matrix math for graphics, tools, and simulation work
- [x] `geom` module — rectangles, points, intersections, bounds, and spatial utilities
- [x] `graph` module — graph nodes/edges, traversal, shortest path, DAG utilities
- [x] `tree` module — generic tree traversal, mutation, and query helpers
- [x] `pipeline` module — composable data pipelines and stream-style transformations
- [x] `stream` module — lazy iterators, generators, adapters, and chunked processing helpers
- [x] `table` module — tabular display, sorting, formatting, and CSV/console rendering helpers
- [x] `search` module — indexing, query parsing, scoring, and local search helpers
- [x] `embed` module — vector embeddings, similarity search hooks, and semantic indexing helpers
- [x] `ml` module — lightweight inference wrappers and data preprocessing primitives

#### Security And Crypto

- [x] `hashlib` module — `md5`, `sha1`, `sha256`, and digest helpers
- [x] `crypto` module — symmetric encryption, signatures, random bytes, and key helpers

#### Terminal, UI, And Presentation

- [x] `ansi` module — terminal colors, cursor movement, box drawing, and styling helpers
- [x] `term` module — raw terminal mode, key events, mouse event records, cursor control, sizing, and deterministic screen buffers across interpreter / VM / LLVM
- [x] `tui` module — higher-level terminal UI widgets, layout, focus, deterministic event loops, labels, buttons, panels, lists, and screen helpers
- [x] `theme` module — reusable palettes, spacing scales, and text-style presets for TUIs
- [x] `color` module — RGB/HSL/HSV conversion, palettes, gradients, hex parsing/formatting, luminance, and contrast helpers
- [x] `scene` module — lightweight scene graphs for TUI/ASCII/game applications

#### Media And Game Development

- [x] `image` module — image metadata, pixel access, resize/crop helpers, grayscale, and PPM conversion
- [x] `audio` module — WAV/PCM helpers, sample buffers, silence/square generation, metadata, normalize/mix/trim processing primitives
- [x] `sprite` module — tiny 2D sprite sheets, tiles, frames, text sprites, flipping, and ASCII animation helpers
- [x] `game` module — worlds, timers, entities, input state, collision helpers, ticks, movement, loop helpers, and snapshots

---

## Phase 7 — Cool Applications ✅ Complete

> Goal: write real apps entirely in Cool

- [x] `calc` — calculator REPL with persistent variables, full math library support
- [x] `notes` — note-taking app (new, show, append, delete, search commands)
- [x] `top` — process/task viewer using `ps aux` and system stats (interactive TTY app)
- [x] `edit` — nano-like text editor (arrow keys, Ctrl+S save, Ctrl+X exit, interactive TTY app)
- [x] `snake` — Snake game (ASCII, arrow keys, real-time with raw terminal mode, interactive TTY app)
- [x] `http` — HTTP client (`http get/post/head/getjson <url>`) backed by curl

---

## Phase 8 — Compiler ✅ Complete

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

### LLVM Backend Coverage

The LLVM backend now covers most day-to-day language features, including default/keyword arguments, closures/lambdas, inheritance, `super()`, slicing, `str()`, `isinstance()`, `try` / `except` / `else` / `finally`, `raise`, helpers like `min()`, `max()`, `sum()`, `round()`, `sorted()`, `abs()`, `int()`, `float()`, `bool()`, source-relative file imports like `import "helper.cool"`, project/package imports like `import foo.bar`, native `import ffi` (`ffi.open`, `ffi.func`), built-in `import math` / `import os` / `import sys` / `import path` / `import csv` / `import datetime` / `import hashlib` / `import toml` / `import yaml` / `import sqlite` / `import http` / `import subprocess` / `import argparse` / `import logging` / `import test` / `import time`, the core `random` helpers (`seed`, `random`, `randint`, `uniform`, `choice`, `shuffle`), `json.loads()` / `json.dumps()`, the built-in `string` helpers (`split`, `join`, `strip`, `lstrip`, `rstrip`, `upper`, `lower`, `replace`, `startswith`, `endswith`, `find`, `count`, `title`, `capitalize`, `format`), the pure `list` helpers (`sort`, `reverse`, `map`, `filter`, `reduce`, `flatten`, `unique`), the `re` helpers (`match`, `search`, `fullmatch`, `findall`, `sub`, `split`), `collections.Queue()` / `collections.Stack()`, native `open()` / file methods, and `with` / context managers on normal exit, control-flow exits (`return`, `break`, `continue`), caught exceptions, and unhandled native raises. No roadmap-tracked day-to-day language gap remains in the LLVM column of this matrix:

| Feature | Interpreter | Bytecode VM | LLVM |
| ------- | :-----------: | :-----------: | :----: |
| Classes | ✅ | ✅ | ✅ |
| Ternary expressions | ✅ | ✅ | ✅ |
| List comprehensions | ✅ | ✅ | ✅ |
| `in` / `not in` | ✅ | ✅ | ✅ |
| Dicts | ✅ | ✅ | ✅ |
| Tuples | ✅ | ✅ | ✅ |
| Closures / lambdas | ✅ | ✅ | ✅ |
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
- [x] `socket` module — TCP/UDP client and server helpers with `connect`, `listen`, `accept`, `connect_udp`, `bind_udp`, `send`, `recv`, `sendto`, `recvfrom`, address helpers, and binary-safe byte APIs across all runtimes

### Packaging And Developer Tooling

- [x] `cool test` command for discovered and explicit Cool test files, with interpreter / VM / native runner modes
- [x] Standard `test` module for in-language unit/integration helpers and assertions (`equal`, `not_equal`, `truthy`, `falsey`, `is_nil`, `not_nil`, `fail`, `raises`)
- [x] Package/dependency metadata beyond `cool.toml`, including manifests, lockfiles, path/git installs, and semver constraint checking (`^`, `~`, `>=`, `>=,<`, `=`, `*`)
- [x] App bundling / release tooling — `cool bundle` (build + distributable tarball with `[bundle].include` in cool.toml) and `cool release` (version bump + bundle + git tag)
- [x] AST / inspect / symbols / modulegraph / diff / check CLI helpers for tooling and static analysis
- [x] Language-server and editor tooling (`lsp`) — `cool lsp` stdio server: diagnostics, completions, hover, go-to-definition, document symbols, workspace symbols

### Flagship Cool Software

- [x] A real package manager or project tool written in Cool
- [x] A build/task runner that demonstrates modules, subprocesses, and packaging
- [x] A flagship TUI or networked app — `browse` (TUI file browser with two-pane layout, directory traversal, file preview, arrow-key navigation, written entirely in Cool)

---

## Phase 11 — Freestanding Systems Foundation ✅ Complete

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

- [x] `core` subset that avoids host OS facilities
- [x] Serial / console output primitives (`outb`, `inb`, `write_serial_byte` — x86 port I/O via inline asm, freestanding-safe)
- [x] Memory-map and paging helpers
- [x] Pluggable allocator hooks for kernels and runtimes

---

## Phase 12 — Static Semantic Core ✅ Complete

> Goal: give Cool a disciplined compile-time spine so it reads like a high-level language but scales like a serious compiled one

### Declarations And Modules

- [x] Typed parameters and return types for ordinary `def`
- [x] Typed local bindings and module-level constants
- [x] Immutable bindings / constant declarations
- [x] Explicit public/private module visibility
- [x] Export surface rules and import validation for large projects

### Type Checking

- [x] A real type checker for normal program code — v0: literal-type mismatch detection at typed-def boundaries; v1: variable type tracking propagates inferred types from assignments and typed-def return values through the checker
- [x] Assignability and coercion rules for primitives — variable types are tracked and checked at typed-def call sites and return statements
- [x] Compile-time checking of function returns on all code paths
- [x] Module-level symbol/type resolution before codegen
- [x] `cool check --strict` / strict project mode — errors on unannotated top-level `def` params and return types; dunder methods exempted

### Tooling Integration

- [x] Type-aware LSP completions, hover, and diagnostics
- [x] Typed `cool ast` / `inspect` output — `cool inspect` now includes `type_name` on annotated params and `return_type` on typed `def`; untyped fields are omitted from JSON output
- [x] Compiler diagnostics with fix suggestions — type mismatch errors now include actionable conversion hints (e.g. "use str(value) to convert", "use int() to truncate")

---

## Phase 13 — Typed Language Features ✅ Complete

> Goal: make Cool comfortable for large native codebases, not just dynamic-style programs that happen to compile

### Core Language Features

- [x] Enums / tagged unions / algebraic data types
- [x] `match` with exhaustiveness checking
- [x] `Option` / `Result` style standard types and language sugar where justified
- [x] Generic functions
- [x] Generic structs / enums / collections
- [x] Traits / interfaces / protocols for shared behavior
- [x] Trait bounds or equivalent constraints for generic code

### Collections And APIs

- [x] Typed standard collections with clear generic surfaces
- [x] Method/trait design that works consistently across interpreter, VM, and native builds
- [x] Error-handling conventions for compiled application code

---

## Phase 14 — Runtime And Memory Model ✅ Complete

> Goal: define the runtime semantics clearly enough that Cool is trusted for systems-facing native software

### Memory Semantics

- [x] Choose and document the primary memory-management model for high-level values in native code
- [x] Define ownership boundaries between Cool-managed values, raw memory, and FFI-owned memory
- [x] Deterministic cleanup story for native resources beyond `with`
- [x] Large-value move/copy/clone semantics
- [x] Arena/region or other explicit allocator strategies where they make sense

### Runtime Profiles

- [x] Clear hosted vs freestanding runtime profile documentation
- [x] Stable `core`/`std` split for host-free builds
- [x] Native panic / abort / diagnostics policy
- [x] Thread/task safety rules for future concurrency features

---

## Phase 15 — Native Toolchain And Distribution ✅ Complete

> Goal: make shipping Cool software feel more like shipping a serious compiled language product than running a script with extra steps

### Compiler And Build UX

- [x] Incremental compilation
- [x] Cross-compilation via explicit target triples
- [x] Build profiles (`dev`, `release`, `freestanding`, and stricter checked modes)
- [x] Reproducible builds and toolchain pinning
- [x] Better binary/object/library output selection

### Debugging, Docs, And Observability

- [x] Native debug info and better stack traces
- [x] Profiling and benchmark tooling
- [x] `cool fmt`
- [x] First-class doc generation for modules, types, and APIs
- [x] Better release artifact metadata and symbol maps

### Packaging

- [x] Registry-quality package/distribution workflow
- [x] Stronger lockfile and dependency reproducibility guarantees
- [x] Project templates aimed at native apps, libraries, services, and freestanding targets

---

## Phase 16 — Systems Interop And Targets ✅ Complete

> Goal: make Cool unusually strong at crossing the line between high-level application code and low-level native/system boundaries

### C / ABI Interop

- [x] `cool bindgen` for C headers
- [x] Richer ABI metadata for `extern def`
- [x] Better native library loading/linking workflows
- [x] Static library and shared library output
- [x] Clear FFI ownership/lifetime annotations

### Targeting And Linking

- [x] Linker-script support and explicit entry-point control
- [x] Object / kernel image output without libc assumptions
- [x] Target CPU / feature flags
- [x] No-libc syscall/runtime support where appropriate
- [x] Better section/link layout tooling for embedded/kernel use

### Systems Libraries

- [x] Register/MMIO helpers built on the existing raw-memory primitives
- [x] Safer pointer/address abstractions on top of raw integer addresses
- [x] Host-independent `core` facilities for allocators, strings, formatting, and collections

---

## Phase 17 — Signature Features And Flagship Software ✅ Complete

> Goal: give Cool a recognizable identity beyond “another compiled language” and prove it with software people care about

### Signature Capability

- [x] Capability/permission model declared in `cool.toml` for file/network/env/process access
- [x] Runtime enforcement and diagnostics for denied capabilities
- [x] Capability-aware stdlib APIs and package metadata

### Concurrency

- [x] Structured concurrency primitives
- [x] Tasks, cancellation, deadlines, and channels
- [x] Process/network orchestration APIs that compose with the stdlib
- [x] Clear semantics across interpreter, VM, and native builds

### Flagship Software

- [x] A real package manager / project tool written in Cool
- [x] A substantial native CLI people would actually use outside the repo
- [x] A flagship TUI or desktop-like terminal app that demonstrates the compiled language story
- [x] A service/backend example large enough to stress the type system, tooling, and packaging
- [x] A freestanding/kernel/boot-path demo that proves the systems subset is not theoretical

---

## Phase 18 — Release Hardening ✅ Complete

> Goal: turn the completed feature roadmap into a repeatable release process with one command that exercises the critical product surface before shipping.

### Release Gate

- [x] Repo-local `scripts/release_gate.sh` command for formatting, build, and full Rust test coverage
- [x] Static Cool checks for representative application and service examples
- [x] Cross-runtime parity smoke covering interpreter, bytecode VM, and LLVM native execution
- [x] Freestanding object-output smoke test for the systems subset
- [x] GitHub Actions workflow that runs the same release gate on pushes, pull requests, and manual dispatches
- [x] README documentation for running the gate locally before release or push

---

## Phase 19 — Release Candidate And Distribution ✅ Complete

> Goal: turn a hardened Cool build into a traceable release-candidate payload that can be uploaded, mirrored, or promoted without rebuilding.

### Release Candidate Packaging

- [x] Repo-local `scripts/release_candidate.sh` command that runs the release gate by default and builds the optimized `cool` compiler binary
- [x] Platform-specific payload layout under `dist/release-candidate/<version>/<platform>/`
- [x] SHA-256 checksums for the packaged binary, documentation, release scripts, and generated release notes
- [x] Machine-readable `manifest.json` with package version, git commit, branch, dirty-state flag, host platform, Rust toolchain, and release-gate status
- [x] Generated `RELEASE_NOTES.md` and compressed `cool-<version>-<platform>.tar.gz` distribution archive
- [x] `latest.json` pointer for downstream automation and artifact mirrors
- [x] GitHub Actions `Release Candidate` workflow that packages and uploads distribution artifacts on manual dispatch or `v*` tag pushes
- [x] README documentation for local RC builds, skip-gate repackaging, and publish/tag boundaries

---

## Phase 20 — Release Promotion And Installer Channels ✅ Complete

> Goal: promote a validated release candidate into upload-ready release assets and provide a repeatable install path for local, hosted, and mirrored channels.

### Promotion

- [x] Repo-local `scripts/promote_release.sh` command that validates RC manifests, checksums, archive layout, release-gate status, git commit, and worktree cleanliness before promotion
- [x] Promoted release layout under `dist/releases/<version>/` with platform archive, manifest/checksum sidecars, `RELEASE.md`, `SHA256SUMS`, `release.json`, and `latest.json`
- [x] Safe-by-default tag handling: promotion records the intended tag and only creates a local annotated tag when `--create-tag` is explicitly passed
- [x] GitHub Actions `Release Promotion` workflow that builds an RC, promotes it, and uploads `dist/releases/**` on manual dispatch or `v*` tag pushes

### Installer Channels

- [x] Root `install.sh` installer for local archives, exact URLs, GitHub Releases downloads, alternate mirrors, custom prefixes, checksum verification, and post-install smoke tests
- [x] RC packaging now includes `install.sh`, install documentation, and promotion scripts so distribution payloads are self-describing
- [x] `docs/INSTALL.md` documents local artifact installs, hosted installs, mirror overrides, checksum verification, and smoke-test behavior
- [x] README documentation for promotion, installer usage, checksum verification, and publish/tag boundaries

---

## Phase 21 — Published Release Automation And Supply-Chain Trust ✅ Complete

> Goal: publish verified release assets safely and make the resulting artifacts auditable through hashes, provenance, SBOMs, and optional signatures.

### Trust Metadata

- [x] `scripts/trust_release.sh` / `scripts/trust_release.py` generate and verify release trust metadata after promotion
- [x] SPDX-style `sbom.spdx.json` generated from `Cargo.lock`
- [x] In-toto/SLSA-style `provenance.intoto.json` with release subjects, git commit, workflow/run metadata, and build inputs
- [x] `trust.json` and `TRUST_SHA256SUMS` hash chain covering promoted assets, metadata, signatures, and verification scripts
- [x] Optional OpenSSL detached signatures for `SHA256SUMS`, `release.json`, provenance, SBOM, and trust metadata

### Publishing

- [x] `scripts/publish_release.sh` verifies trust metadata before creating or updating a GitHub Release with `gh`
- [x] GitHub Actions `Published Release` workflow builds the RC, promotes it, generates trust metadata, optionally signs with `COOL_RELEASE_SIGNING_KEY_B64`, verifies the result, and uploads or publishes final assets
- [x] Installer metadata verification through `--verify-metadata`, `--checksums`, `--checksums-signature`, and `--verify-key`
- [x] Release trust documentation for generation, signing, verification, and publishing

---

## Phase 22 — Multi-Platform Release Matrix And Package Channels ✅ Complete

> Goal: ship the trusted release pipeline across real user platforms and generate package-channel metadata from the resulting artifacts.

### Multi-Platform Matrix

- [x] Release candidates support explicit `--platform` labels and emit both `.tar.gz` and `.zip` payload archives
- [x] GitHub Actions `Release Matrix` workflow builds Linux x86_64, macOS x86_64, macOS arm64, and Windows x86_64 release artifacts with per-platform smoke/promotion steps
- [x] `scripts/assemble_matrix_release.sh` / `.py` combine matrix artifacts into one multi-platform release directory
- [x] Multi-platform trust generation and publish dry-run support through `--platform multi`
- [x] Installer defaults to Windows zip assets and macOS/Linux tarball assets, with platform normalization for common host labels

### Package Channels

- [x] `scripts/package_channels.sh` / `.py` generate package-channel metadata from promoted release assets
- [x] Homebrew formula generation for macOS and Linuxbrew tarball installs
- [x] Winget portable manifests generated when a Windows zip artifact is present
- [x] Debian package and apt-style `Packages` indexes generated when a Linux x86_64 tarball is present
- [x] `channels.json`, `CHANNEL_SHA256SUMS`, `latest.json`, and `cool-<version>-package-channels.tar.gz` channel bundle for release uploads and mirrors
- [x] Documentation for package-channel generation, required-platform checks, and matrix assembly

---

## Phase 23 — Public Release Validation And Ecosystem Readiness ✅ Complete

> Goal: make a public release auditable before publish by validating every promoted asset, trust file, installer path, and package-channel output.

### Release Validation

- [x] `scripts/validate_release.sh` / `.py` validate promoted release metadata, `SHA256SUMS`, platform sidecars, tarball/zip payload layouts, payload manifests, and payload checksums
- [x] Trust validation covers SBOM, provenance, `trust.json`, `TRUST_SHA256SUMS`, release `supply_chain` references, and optional detached signature verification with `--verify-key`
- [x] Package-channel validation covers `channels.json`, `CHANNEL_SHA256SUMS`, channel archive contents, Homebrew formula asset coverage, Winget portable manifests, Debian `.deb` structure, `Packages`, and `Packages.gz`
- [x] Installer smoke validation installs a selected platform archive with checksum metadata and runs `cool help`
- [x] JSON validation reports can be written under `dist/validation/<version>/`

### Matrix And CI Readiness

- [x] `scripts/smoke_matrix_release.sh` / `.py` synthesize Linux x86_64, macOS x86_64, macOS arm64, and Windows x86_64 artifact sets from one promoted host release, then assemble, trust, channelize, and validate the result
- [x] GitHub Actions `Release Validation` workflow checks shell syntax, Python compile status, host release validation, installer smoke behavior, and synthetic matrix/package-channel validation on pushes, pull requests, and manual dispatches
- [x] `Release Matrix` aggregate job validates multi-platform releases with all required platforms before publish or dry-run completion
- [x] `Published Release` workflow validates single-platform promoted releases and uploads validation reports with release assets
- [x] Release candidates and promoted releases include validation scripts and `docs/RELEASE_VALIDATION.md`

---

## Phase 24 — Real Public Release And Post-Release Operations ✅ Complete

> Goal: close the release loop after upload by verifying public download URLs, documenting release-day operations, and giving regressions a clear support path.

### Hosted Release Verification

- [x] `scripts/verify_hosted_release.sh` / `.py` verify GitHub Release or mirror assets from hosted URLs instead of local `dist/` directories
- [x] Hosted verification checks `release.json`, `latest.json`, `SHA256SUMS`, archive hashes/sizes, payload layouts, platform sidecars, trust metadata, package-channel archive checksums, and optional installer smoke behavior
- [x] The verifier supports required-platform checks, signed metadata verification with `--verify-key`, mirror bases with `--base-url`, JSON reports, retained download directories, and local `file://` mirrors for CI smoke tests
- [x] GitHub Actions `Hosted Release Verify` runs on published releases and manual dispatches, uploads hosted verification reports, and validates the full public platform contract

### Release Operations

- [x] `docs/RELEASE_RUNBOOK.md` defines local preflight, matrix release, published verification, rollback, hotfix, and final-record procedures
- [x] `docs/SUPPORT_MATRIX.md` records supported platforms, archive defaults, package channels, verification coverage, and platform support policy
- [x] Release issue templates cover public release checklists and hotfix/regression handling
- [x] Pull request template now calls out validation and release-impact checks
- [x] Release candidates and promoted/matrix releases include hosted verification scripts plus release runbook and support-matrix docs

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
| 11 — Freestanding Systems Foundation | ✅ Complete |
| 12 — Static Semantic Core | ✅ Complete |
| 13 — Typed Language Features | ✅ Complete |
| 14 — Runtime And Memory Model | ✅ Complete |
| 15 — Native Toolchain And Distribution | ✅ Complete |
| 16 — Systems Interop And Targets | ✅ Complete |
| 17 — Signature Features And Flagship Software | ✅ Complete |
| 18 — Release Hardening | ✅ Complete |
| 19 — Release Candidate And Distribution | ✅ Complete |
| 20 — Release Promotion And Installer Channels | ✅ Complete |
| 21 — Published Release Automation And Supply-Chain Trust | ✅ Complete |
| 22 — Multi-Platform Release Matrix And Package Channels | ✅ Complete |
| 23 — Public Release Validation And Ecosystem Readiness | ✅ Complete |
| 24 — Real Public Release And Post-Release Operations | ✅ Complete |
