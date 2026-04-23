# Cool Language Roadmap

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
- [x] Package system — `import foo.bar` loads `foo/bar.cool` from source directory

### Next Library Targets

#### Data And Serialization

- [x] `csv` module — CSV reader/writer helpers for rows, header-based dicts, and basic quoting/escaping
- [ ] `json` extensions — schema-aware JSON transforms and streaming helpers
- [ ] `toml` module — parse and write TOML for project/config tooling
- [ ] `yaml` module — YAML parsing/serialization for config and automation
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
- [ ] `platform` module — OS/arch/runtime detection and host capability helpers
- [x] `subprocess` module — structured process spawning, exit codes, stdout/stderr capture
- [ ] `daemon` module — service lifecycle helpers, PID files, logs, and restart policies
- [ ] `sandbox` module — constrained command/file execution helpers for safer automation
- [ ] `sync` module — file/state synchronization, conflict detection, and reconciliation helpers
- [ ] `store` module — key-value persistence, namespaces, and transactional update helpers

#### Networking And Services

- [ ] `http` module — request helpers built into the language runtime instead of only the shell app
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

- [ ] `sqlite` module — embedded database access with queries, params, and row iteration
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

- [ ] `hashlib` module — `md5`, `sha1`, `sha256`, and digest helpers
- [ ] `crypto` module — symmetric encryption, signatures, random bytes, and key helpers

#### Terminal, UI, And Presentation

- [ ] `ansi` module — terminal colors, cursor movement, box drawing, and styling helpers
- [ ] `term` module — raw terminal mode, key events, mouse events, and screen buffers
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

The LLVM backend now covers most day-to-day language features, including default/keyword arguments, inheritance, `super()`, slicing, `str()`, `isinstance()`, `try` / `except` / `else` / `finally`, `raise`, helpers like `min()`, `max()`, `sum()`, `round()`, `sorted()`, `abs()`, `int()`, `float()`, `bool()`, source-relative file imports like `import "helper.cool"`, project/package imports like `import foo.bar`, native `import ffi` (`ffi.open`, `ffi.func`), built-in `import math` / `import os` / `import sys` / `import path` / `import csv` / `import subprocess` / `import argparse` / `import logging` / `import test` / `import time`, the core `random` helpers (`seed`, `random`, `randint`, `uniform`, `choice`, `shuffle`), `json.loads()` / `json.dumps()`, the built-in `string` helpers (`split`, `join`, `strip`, `lstrip`, `rstrip`, `upper`, `lower`, `replace`, `startswith`, `endswith`, `find`, `count`, `title`, `capitalize`, `format`), the pure `list` helpers (`sort`, `reverse`, `map`, `filter`, `reduce`, `flatten`, `unique`), the `re` helpers (`match`, `search`, `fullmatch`, `findall`, `sub`, `split`), `collections.Queue()` / `collections.Stack()`, native `open()` / file methods, and `with` / context managers on normal exit, control-flow exits (`return`, `break`, `continue`), caught exceptions, and unhandled native raises. The following features still have notable gaps in LLVM:

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

## Phase 10 — Production Readiness And Ecosystem 🚧 In Progress

> Goal: make Cool feel default for real applications, not just impressive for demos, scripts, and experiments

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
- [x] `subprocess` module — process spawning, exit codes, stdout/stderr capture, and timeouts
- [x] `argparse` module — positional/flag parsing, defaults, and generated help text
- [x] `logging` module — leveled logs, formatters, timestamps, and file/stdout handlers
- [ ] `toml` / `yaml` / `hashlib` / `sqlite` / `socket` / `http` as the first practical application stack

### Packaging And Developer Tooling

- [x] `cool test` command for discovered and explicit Cool test files, with interpreter / VM / native runner modes
- [x] Standard `test` module for in-language unit/integration helpers and assertions (`equal`, `not_equal`, `truthy`, `falsey`, `is_nil`, `not_nil`, `fail`, `raises`)
- [ ] Package/dependency metadata beyond `cool.toml`, including manifests, semver, and dependency resolution
- [ ] App bundling / release tooling (`package`, `bundle`, `release`)
- [ ] AST / inspect / modulegraph / diff helpers for tooling and static analysis
- [ ] Language-server and editor tooling (`lsp`)

### Flagship Cool Software

- [ ] A real package manager or project tool written in Cool
- [ ] A build/task runner that demonstrates modules, subprocesses, and packaging
- [ ] A flagship TUI or networked app that proves Cool works for medium-sized software

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
| 10 — Production Readiness And Ecosystem | 🚧 In Progress |
