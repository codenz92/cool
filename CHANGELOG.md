# Changelog

All notable changes to the Cool language project.

## [Unreleased] - Phase 17 Complete

### LLVM Backend

- LLVM native builds now emit callable helper functions for lambda expressions and nested function closures, store shared capture cells in closure records, and activate capture frames when closures are called directly or via callable stdlib helpers such as `list.map`
- The previous roadmap-tracked LLVM limitation for closures/lambdas has been removed from the backend coverage matrix

### Phase 14 — Runtime And Memory Model

- New runtime builtins `copy()`, `clone()`, `close()`, `panic()`, and `abort()` now behave consistently across interpreter, VM, and hosted native builds, with shallow-copy semantics for containers/instances, explicit rejection of opaque resource handles, deterministic close dispatch, and fatal panic/abort paths
- `import platform` now exposes runtime-model metadata across all runtimes: `runtime_profile()`, `memory_model()`, `panic_policy()`, `thread_safety()`, and `stdlib_split()` alongside the existing capability and host-platform helpers
- New bundled `stdlib/std/runtime.cool` and `stdlib/std/memory.cool` make the Phase 14 model usable from normal Cool code: `std.runtime.describe()`, `std.memory.deep_clone()`, `std.memory.Scope`, and `std.memory.Arena` document and operationalize the runtime/ownership story
- Hosted native builds now carry a clearer runtime policy: native `panic()` prints diagnostics plus a stack trace, `abort()` terminates immediately, struct values support explicit shallow copy through layout metadata, and runtime cleanup integrates with the existing `with`/stack-trace/profiling paths

### Phase 13 — Typed Language Features

- New first-class typed language syntax across interpreter, VM, and native builds: `enum`, `match`, `trait`, `implements`, generic `def`, generic `struct`, generic `union`, richer type annotations like `Option[int]`, `list[str]`, and `dict[str, int]`, plus pattern forms such as `Option.Some(value)`, `Option.None`, literals, and `_`
- New lowering/runtime path maps enums and `match` onto the existing class/object model so tagged unions, generic structs, and trait-checked classes behave consistently across all three execution modes instead of being native-only syntax
- `cool check` now understands trait implementations, generic bounds, typed collection surfaces, generic constructor calls, enum variant construction, and non-exhaustive `match` blocks for local enums, with concrete diagnostics for trait/bound failures and typed collection mismatches
- New bundled `stdlib/option.cool` and `stdlib/result.cool` provide practical typed error-handling helpers (`some`, `none`, `ok`, `err`, `unwrap`, `unwrap_or`, `unwrap_err`, predicates) on top of the new enum surface
- Formatter, AST/inspect/doc tooling, and module export indexing now understand typed-language declarations, including enum variants, trait methods, generic parameters, and `implements` clauses

### Phase 6 Follow-on — Terminal, UI, And Presentation Modules

- New bundled `stdlib/ansi.cool`, `stdlib/color.cool`, `stdlib/theme.cool`, `stdlib/tui.cool`, and `stdlib/scene.cool` cover ANSI styling/cursor helpers, RGB/HSL/HSV color operations, palettes, reusable theme styles/spacing, deterministic TUI widgets/layout/focus/event state, and lightweight ASCII scene graphs
- `import term` now includes deterministic mouse event records, mouse tracking escape toggles, and mutable in-memory screen buffers (`screen`, `screen_put`, `screen_text`, `screen_clear`) in addition to raw mode, cursor control, sizing, key polling, and output helpers
- The terminal/UI tranche is covered across interpreter, VM, and LLVM native builds, including native runtime support for the new `term` screen/mouse helpers

### Phase 6 Follow-on — Media And Game Development Modules

- New bundled `stdlib/image.cool`, `stdlib/audio.cool`, `stdlib/sprite.cool`, and `stdlib/game.cool` add in-memory image buffers, crop/resize/grayscale/PPM conversion, lightweight audio sample buffers with PCM/WAV-style records, ASCII sprite tiles/sheets/animation helpers, and entity/world/timer/input/collision/main-loop primitives
- The media/game tranche is covered across interpreter, VM, and LLVM native builds with deterministic tests that avoid real TTY, audio device, or image codec dependencies

### Phase 6 Follow-on — Data Modules (Pass 1)

- New bundled `stdlib/bytes.cool`, `stdlib/base64.cool`, `stdlib/codec.cool`, `stdlib/html.cool`, `stdlib/config.cool`, and `stdlib/schema.cool` modules cover byte strings, hex/base64, codec dispatch, HTML escaping/extraction, config loading/merging, and typed shape validation across interpreter, VM, and native builds
- `bytes` includes UTF-8/ASCII/Latin-1 conversion, hex helpers, concatenation/slicing, and little/big-endian unsigned integer pack/unpack helpers; `base64` and `codec` build on the same byte-path APIs instead of each reimplementing binary plumbing
- `config` now loads `.json`, `.toml`, `.yaml`, `.ini`, and `.env` styles with format inference plus deep merge and `${NAME}` environment expansion helpers, while `schema` provides reusable `string` / `integer` / `number` / `boolean` / `list_of` / `shape` / `one_of` rules with structured error reporting
- Cross-runtime `ord()` / `chr()` parity is now in place, and the LLVM runtime now treats string length, indexing, slicing, and `for ch in text` iteration in UTF-8 code points instead of raw bytes so the new modules behave consistently in native builds

### Phase 6 Follow-on — Data Modules (Pass 2)

- `import json` now goes beyond basic parse/serialize: `loads_lines()` / `dumps_lines()` cover newline-delimited JSON streams, `pointer()` exposes JSON Pointer lookup with defaults, and `transform()` applies schema-style projections/coercions consistently across interpreter, VM, and native builds
- New bundled `stdlib/xml.cool` adds lightweight XML parsing/serialization plus `text_content()`, `find()`, and `find_all()` helpers for tag-path extraction and small document manipulation
- New bundled `stdlib/unicode.cool` adds Unicode category lookup, canonical/compatibility normalization, display-width estimation, and grapheme-cluster helpers, backed by generated lookup helpers so the same APIs work in native builds as well as the interpreter and VM
- New bundled `stdlib/locale.cool` adds locale tag parsing/normalization, language/region naming helpers, locale matching, locale-aware number parsing/formatting, and currency formatting

### Phase 6 Follow-on — Filesystem And OS Modules

- New bundled `stdlib/glob.cool` adds wildcard path matching, recursive discovery, `**` traversal, and explicit `glob.matches()` / `glob.glob()` / `glob.walk()` helpers that behave consistently across interpreter, VM, and native builds
- New bundled `stdlib/tempfile.cool` adds temporary file/directory creation plus cleanup-aware `TempFile` / `TempDir` handles, `mkstemp()` / `mkdtemp()`, and recursive cleanup helpers for normal hosted workflows
- New bundled `stdlib/process.cool` adds PID/PPID inspection, environment snapshots, signal helpers, liveness checks, and runtime/capability metadata without forcing callers down to raw `os.*` plumbing
- New bundled `stdlib/fswatch.cool` adds polling-based file watching with snapshots, diffs, ignore patterns, timeout-aware `watch()`, and change extraction helpers for rebuild loops, editors, and automation
- New bundled `stdlib/store.cool` adds file-backed namespaced key-value persistence, snapshots, increments, and transactional commit/rollback helpers that behave consistently across interpreter, VM, and native builds
- New bundled `stdlib/daemon.cool` adds hosted service lifecycle helpers with PID files, stdout/stderr logs, exit-code sidecars, cleanup, and restart-policy maintenance flows for long-running automation
- New bundled `stdlib/sandbox.cool` adds constrained file/process helpers around explicit roots, allowlisted commands, and allowlisted environment access so automation code can tighten its own working envelope on top of manifest capabilities
- New bundled `stdlib/sync.cool` adds directory snapshots, persisted baseline state, conflict detection, and source/destination reconciliation helpers for backup, mirroring, and workflow state handoff
- The shared `import os` runtime surface now includes `stat()`, `tmpdir()`, `environ()`, `getpid()`, `getppid()`, `is_alive()`, `kill()`, and `rmdir()` across interpreter, VM, and hosted native builds so the new stdlib modules sit on the same capability-enforced substrate
- While landing the new tranche, the stdlib implementation was also hardened against real cross-runtime differences: helper exports are now private where appropriate, interpreter-visible exception scopes no longer leak into the public APIs, and the native backend path avoids unsupported constructs such as forward class references, `del`, and method/default-argument dispatch traps

### Phase 6 Follow-on — Networking And Services Modules

- `import socket` now covers TCP plus UDP across interpreter, VM, and native builds: `connect_udp()`, `bind_udp()`, `sendto()` / `recvfrom()`, `local_addr()` / `peer_addr()`, and binary-safe `send_bytes()` / `recv_bytes()` / `sendto_bytes()` / `recvfrom_bytes()` are available alongside the existing stream APIs
- New bundled `stdlib/url.cool` adds URL parsing, formatting, joining, query-string encode/decode, and percent escaping so higher-level network helpers do not have to reimplement URL logic ad hoc
- New bundled `stdlib/websocket.cool` adds client handshakes, server-side `accept_client()` flows, frame encode/decode, text/JSON/binary helpers, and ping/pong/close handling that now behaves consistently in interpreter, VM, and hosted native builds
- New bundled `stdlib/rpc.cool`, `stdlib/graphql.cool`, and `stdlib/feed.cool` add JSON-RPC style routing, GraphQL query/render/execute helpers, and RSS/Atom polling plus feed rendering on top of the shared HTTP/socket surface
- New bundled `stdlib/mail.cool` and `stdlib/calendar.cool` add SMTP/IMAP-style workflows plus recurrence, agenda, reminder, and date-range planning helpers for automation-heavy apps and tools
- New bundled `stdlib/cluster.cool` adds simple distributed coordination primitives on top of `store`: membership, leases, barriers, leader selection, and file-backed state for multi-node experiments
- While landing the networking tranche, the bundled modules were tightened for true cross-runtime parity: native builds now reject fewer stdlib portability traps because the new code avoids open-ended slice forms and string-method argument patterns that the interpreter/VM had tolerated more loosely

### Phase 6 Follow-on — Databases And Storage Modules

- New bundled `stdlib/cache.cool` adds in-memory and disk-backed TTL caches with invalidation, prefix clearing, hit/miss stats, and `cache.remember(...)` helpers that now behave consistently across interpreter, VM, and native builds
- New bundled `stdlib/memo.cool` adds deterministic memoization tables on top of those cache backends, plus semantically stable `memo.call(...)` helpers for reusable function-result caching across runtimes
- New bundled `stdlib/package.cool` adds semantic-version parsing/comparison, version bumping and constraint checks, manifest/project inspection, package IDs, and local dependency-tree resolution helpers for project tooling
- New bundled `stdlib/compress.cool`, `stdlib/archive.cool`, and `stdlib/bundle.cool` add pure-Cool gzip/tar/zip helpers, higher-level archive pack/list/unpack flows, and single-file bundle packaging plus asset extraction on top of the same byte-oriented primitives
- Native `open()` file handles now support `read_bytes()` and `write_bytes()` alongside the existing text methods, so archive/cache/package tooling can move binary payloads without dropping to ad hoc host glue
- While landing the storage tranche, the bundled code was hardened against real cross-runtime gaps: helper modules now avoid unsupported `with open(...)` and native method-default traps internally, and the new coverage exercises the same cache/archive/bundle flows through interpreter, VM, and native runs

### Phase 6 Follow-on — Parsing, Language, And Tooling Modules

- New bundled `stdlib/doc.cool`, `stdlib/template.cool`, `stdlib/lexer.cool`, `stdlib/parser.cool`, `stdlib/ast.cool`, and `stdlib/inspect.cool` add document rendering, lightweight templating, token scanning, token-stream helpers, Cool source summaries, and portable value/module inspection
- New bundled `stdlib/diff.cool`, `stdlib/patch.cool`, `stdlib/project.cool`, `stdlib/release.cool`, `stdlib/repo.cool`, and `stdlib/modulegraph.cool` add line diffing, patch application, scaffold/manifest metadata, release planning, git status parsing, and import graph visualization helpers
- New bundled `stdlib/plugin.cool`, `stdlib/lsp.cool`, `stdlib/ffiutil.cool`, and `stdlib/shell.cool` add plugin descriptors/registries, JSON-RPC/LSP message utilities, FFI signature/wrapper helpers, and shell quoting/splitting/alias/completion helpers
- The new tooling tranche is covered across the interpreter, VM, and LLVM backend, including native-portability fixes for template contexts, string helpers, dictionary key ordering, and inspection fallbacks

### Phase 6 Follow-on — Runtime, Automation, And Observability Modules

- New bundled `stdlib/event.cool`, `stdlib/workflow.cool`, `stdlib/agent.cool`, and `stdlib/retry.cool` add pub/sub event buses, resumable workflow graphs, agent-style task plans/executors, and retry policies with backoff and failure classification
- New bundled `stdlib/metrics.cool`, `stdlib/trace.cool`, `stdlib/profile.cool`, and `stdlib/bench.cool` add counters/gauges/histograms, trace/span records, runtime hotspot summaries, and lightweight benchmark statistics/comparison helpers
- New bundled `stdlib/notebook.cool` and `stdlib/secrets.cool` add executable notebook cells with saved outputs plus redaction, secret lookup, encrypted vault storage, and environment injection helpers
- The automation/observability tranche is covered across the interpreter, VM, and LLVM backend with deterministic assertions around events, workflows, retries, metrics, traces, profiling, benchmarking, notebook persistence, and secret vault round-trips

### Phase 6 Follow-on — Math, Data Science, And Finance Modules

- New bundled `stdlib/decimal.cool`, `stdlib/money.cool`, and `stdlib/stats.cool` add scale-aware decimal arithmetic, decimal-safe currency values/conversion/allocation, descriptive statistics, percentiles, histograms, and sampling helpers
- New bundled `stdlib/vector.cool`, `stdlib/matrix.cool`, `stdlib/geom.cool`, `stdlib/graph.cool`, and `stdlib/tree.cool` add numeric vectors/matrices, geometry primitives, graph traversal/shortest-path/DAG helpers, and generic tree construction/traversal/search helpers
- New bundled `stdlib/pipeline.cool`, `stdlib/stream.cool`, `stdlib/table.cool`, `stdlib/search.cool`, `stdlib/embed.cool`, and `stdlib/ml.cool` add composable data pipelines, stream adapters, tabular rendering/sorting/grouping, local search indexes, bag-of-words embeddings, and lightweight ML preprocessing/KNN/accuracy helpers
- The new math/data/finance tranche is covered across the interpreter, VM, and LLVM backend, including a native decimal parser fix that avoids host `int()` octal interpretation for leading-zero fractional strings

### Phase 6 Follow-on — Security And Crypto Module

- New bundled `stdlib/crypto.cool` adds key derivation, random byte/token helpers, constant-time comparison, HMAC-style signatures, signature verification, and authenticated symmetric envelope encryption/decryption on top of the shared `hashlib`, `bytes`, and `base64` surfaces
- The security/crypto tranche is covered across the interpreter, VM, and LLVM backend with deterministic key, token, sign/verify, encrypt/decrypt, seal/open, and comparison checks

### Signature Capability

- Projects can now declare `[capabilities]` in `cool.toml` for `file`, `network`, `env`, and `process` access, and the interpreter, VM, and native runtime all enforce the same policy for `open()`, `os`, `http`, `socket`, `subprocess`, and related helpers
- Runtime diagnostics for denied capabilities now name both the missing permission and the blocked operation, and `platform.capabilities()` exposes the active manifest policy to Cool code
- Published package metadata, bundle metadata, and `cool pkg capabilities` all record the same permission model so tooling and users can audit capability requirements before execution

### Concurrency

- New bundled `stdlib/jobs.cool` module provides structured concurrency primitives across interpreter, VM, and native builds: task groups, channels, deadlines, cancellation, polling, and `await_all`
- `jobs.command`, `jobs.http_get`, `jobs.http_head`, and `jobs.sleep` provide composable process/network orchestration APIs that honor the manifest capability policy
- LLVM-native string instance method dispatch now covers `.split()`, `.replace()`, `.startswith()`, `.endswith()`, `.find()`, `.count()`, `.title()`, `.capitalize()`, and `.format()`, bringing method-call parity in line with the string module helpers that the new bundled tools rely on

### Flagship Software

- New `cool pkg` command is written in Cool and provides project/package workflow helpers: `info`, `deps`, `tree`, `capabilities`, and `doctor`, plus pass-through `install`, `add`, and `publish` flows
- New `apps/pulse.cool` health-check runner and `apps/control.cool` terminal dashboard provide a practical concurrent CLI/TUI pair driven by TOML manifests and powered by `import jobs`
- New `examples/coolboard/` project is a native SQLite-backed note service with manifest capabilities, package metadata, and pulse checks, exercising the application/tooling path as a realistic backend example
- New `examples/kernel_demo/` project proves the freestanding subset with an explicit `_start`, a linker script, and VGA text-memory output through the core MMIO helpers

### Systems Interop And Targets


- New `cool bindgen` subcommand for a practical C-header subset: simple `#define` constants, enum constants, struct/union layouts, typedef aliases, and plain function prototypes now generate starter Cool `extern def` / `struct` / `union` bindings
- `extern def` now accepts richer ABI/link metadata: `library`, `link_kind`, `weak`, `ownership`, and `lifetime` alongside the existing `symbol`, `cc`, and `section` fields
- Native project linking now supports manifest-driven `[native]` libraries, frameworks, search paths, and rpaths, and `extern def` metadata can request specific static/shared link behavior for individual bindings
- `cool build --emit sharedlib` now produces first-class shared-library artifacts (`lib<name>.<so|dylib|dll>`) in addition to binaries, objects, assembly, LLVM IR, and static libraries
- `cool build` now supports `--cpu`, `--cpu-features`, `--entry`, and `--no-libc`, with matching `[build]` manifest fields for explicit target tuning and host-free link flows
- Linux no-libc binaries now use an explicit low-level entry path, while freestanding and no-libc flows share the same raw-memory/MMIO/syscall lowering support where appropriate
- New `cool layout` subcommand inspects section, symbol, entry-point, and archive-member layout for objects, archives, executables, shared libraries, and kernel images; `--json` is intended for tooling/CI
- `import core` now exposes safer address helpers (`addr`, `addr_add`, alignment helpers), string/formatting helpers, lightweight collection constructors, and LLVM-only MMIO/register/syscall helpers; interpreter, VM, and LLVM hosted builds now agree on the pure helper behavior

### Benchmark Tooling

- New `cool bench` subcommand for native-first benchmarking: discovers `bench_*.cool` / `*_bench.cool` under `benchmarks/`, compiles each workload with the LLVM backend, times warmup + measured runs, and reports compile/mean/median/min timings
- `cool new` now scaffolds `benchmarks/bench_main.cool` plus a starter `[tasks.bench]` manifest task so projects get a working benchmark path alongside `cool test`
- Shared Rust-side benchmark helpers now back both `cool bench` and the repo's `bench_compare` maintainer harness, keeping timing/reporting logic aligned

### Native Toolchain UX

- `cool build` now supports named build profiles: `dev`, `release`, `freestanding`, and `strict`
- Manifest-driven defaults via `[build] profile = "..."` in `cool.toml`; CLI `--profile` overrides the manifest setting
- `dev` profile runs a checked build (`cool check` semantics) before native compile, while `strict` profile runs strict annotation checks (`cool check --strict`) before compile
- `freestanding` profile makes manifest-driven `cool build` emit `.o` output by default without needing `--freestanding`
- `cool new` now supports `--template app|lib|service|freestanding`, with template-specific manifests, starter source layouts, tasks, tests, and benchmarks
- `cool build` now supports explicit artifact selection via `--emit` and `[build].emit`, covering hosted/freestanding object output, assembly (`.s`), LLVM IR (`.ll`), static libraries (`.a`), and the existing binary path
- `cool build` now supports explicit LLVM target triples via `--target` and `[build].target`, and native pointer-width lowering (`isize`/`usize`, `word_bits`, `word_bytes`) now follows the selected target instead of the host
- `cool build` now supports incremental native rebuilds with a project-local cache under `.cool/cache/build`, configurable via `[build].incremental` plus `--incremental` / `--no-incremental`
- `cool build` now supports reproducible-build toggles and manifest-driven debug info defaults via `[build].reproducible` / `[build].debug`, plus pinned external tools through `[toolchain] cool|cc|ar|lld`
- Hosted `staticlib` output archives both the generated Cool object and the hosted runtime object, so downstream native link steps can consume a single `lib*.a`
- `--linker-script` / `linker_script` still produce kernel images by default, but `--emit` can now override that final artifact choice when you want intermediate object/assembly/IR output instead

### Debugging, Profiling, And Formatting

- Native builds now emit stack traces for unhandled exceptions, with function names and line tracking threaded through the LLVM runtime
- `cool build --debug` emits native debug info and line locations for LLVM-produced binaries and objects
- `cool bench --profile` now captures runtime hotspot summaries for native benchmarks, and the same report can be redirected through `COOL_PROFILE_OUT`
- New `cool fmt` subcommand reformats `.cool` source files, supports `--check`, and preserves standalone plus inline `#` comments
- `cool new` now scaffolds starter `[tasks.fmt]` tasks alongside build/test/bench/doc workflows

### API Documentation

- New `cool doc` subcommand generates first-class API docs for reachable modules, functions, classes, methods, structs, unions, and top-level bindings
- Human-readable output supports Markdown (default) and standalone HTML; `--json` / `--format json` emits a structured report for tooling
- With no file argument inside a project, `cool doc` uses `cool.toml`'s `main` entry and documents every reachable local module; `--private` includes private members and marks them in rendered output
- `cool new` now scaffolds a starter `[tasks.doc]` task so fresh projects can write `docs/API.md` without extra setup

### Release Artifacts

- `cool bundle` now emits `dist/<artifact>.metadata.json` plus `dist/<artifact>.symbols.txt` alongside the `.tar.gz` archive
- Bundled archives embed the same metadata and symbol map as `metadata.json` and `symbols/<artifact>.symbols.txt`, so packaged releases carry their own inspection data
- Metadata records project identity, build profile, artifact kind/path, bundle includes, and manifest dependency specs; symbol maps are generated from `nm` / `llvm-nm` when available
- `cool bundle` / `cool release` now accept `--target` and preserve manifest/CLI target triples in archive names plus release metadata
- `cool release` now surfaces the generated metadata and symbol sidecars as part of the release flow

### Packaging And Reproducibility

- New `cool publish` subcommand validates a source distribution, ensures `cool.lock` is present and verified, writes `dist/<name>-<version>.publish.json`, and packages a `.coolpkg.tar.gz` archive
- `cool publish --dry-run` performs the same lockfile/hash validation without writing the archive
- `cool install` now supports `--locked` and `--frozen`, records dependency checksums in `cool.lock`, and rejects manifest/source drift when dependencies or selectors no longer match the lockfile
- `cool bundle` now verifies or generates `cool.lock` before packaging, so binary artifacts and source packages share the same dependency reproducibility expectations
- `cool new` now scaffolds starter `[tasks.publish]` tasks for projects that want a source-distribution workflow from day one

### Phase 12 — Static Semantic Core (In Progress)

#### Typed Function Signatures

- Ordinary `def` now accepts optional typed parameters (`x: i32`) and return types (`-> i32`) using the same ABI type names as `extern def`
- LLVM backend emits native-typed function signatures (`i32 @foo(i32)`) for annotated defs; unboxes typed params at function entry for the body, re-boxes at return; call sites detect native-typed callees and convert automatically
- Interpreter and VM accept the annotation syntax and execute functions dynamically (annotations ignored at runtime)
- Parser fix: `->` return type is parsed after `)` and before `:`, keeping `lambda x: expr` syntax unambiguous

#### Type Checker (v0)

- New type-checking pass in `cool check`: collects typed `def` signatures across all reachable modules, then checks call sites and return statements for obvious literal-type mismatches
- Flags: string literal passed to an integer param, integer literal passed to a `str` param, float literal passed to an integer type (precision loss), nil passed to typed params, return type mismatches with literal returns
- Leaves untyped functions and non-literal expressions (variables, calls, arithmetic) unchecked — no false positives on existing code
- Runs automatically as part of `cool check`; exits non-zero on type errors
- `cool check --strict` additionally requires every top-level `def` to have fully annotated parameters and a return type; dunder methods (`__init__`, `__str__`, etc.) are exempted; violations are errors
- Type checker v1: variable type tracking — inferred types from literal assignments (`x = "hello"` → `x: str`) and typed-def return values (`x = add(1, 2)` → `x: i32`) are recorded in a per-scope environment and checked at subsequent typed-def call sites and `return` statements; catches `add(bad_str, 2)` and `return str_var` in an `-> i32` function
- `cool inspect` output now includes `type_name` on annotated parameters and `return_type` on typed `def` and `extern def`; untyped fields are omitted so existing tooling JSON consumers are not broken
- Type mismatch error messages now include actionable fix suggestions: "use str(value) to convert", "use int(value) to convert", "use int() to truncate (precision may be lost)"

---

## [Unreleased] - Phase 11 Complete

### Phase 11 — Freestanding Systems Foundation (Complete)

#### Cross-Runtime Platform Introspection

- New `platform` module across the interpreter, bytecode VM, and LLVM backend: `os()`, `arch()`, `family()`, `runtime()`, `exe_ext()`, `shared_lib_ext()`, `path_sep()`, `newline()`, `is_windows()`, `is_unix()`, `has_ffi()`, `has_raw_memory()`, `has_extern()`, and `has_inline_asm()`

#### Numeric And Memory Primitives

- Volatile LLVM raw-memory helpers for MMIO/device-style access: `read_*_volatile` / `write_*_volatile` across byte, `i8`/`u8`, `i16`/`u16`, `i32`/`u32`, `i64`, and `f64`
- Pointer-width aliases and host-word helpers across all runtimes: `isize`, `usize`, `word_bits()`, and `word_bytes()`, plus `isize`/`usize` support in native FFI signatures and struct/union field types
- Interpreter and bytecode VM now reserve LLVM-only raw-memory builtins and raise the same backend-specific guidance (`compile with cool build`) instead of a missing-name error

#### Core Systems Runtime

- New host-free `core` module across the interpreter, bytecode VM, and LLVM backend: `word_bits()`, `word_bytes()`, `page_size()`, `page_align_down()`, `page_align_up()`, `page_offset()`, `page_index()`, `page_count()`, `pt_index()`, `pd_index()`, `pdpt_index()`, and `pml4_index()`
- LLVM-native allocator hooks via `core.set_allocator()`, `core.clear_allocator()`, `core.alloc()`, and `core.free()`, plus `malloc()` / `free()` lowering that honors those hooks in hosted and freestanding native builds
- Freestanding builds now allow top-level `import core` so kernel-style entry points can use page helpers and allocator hooks without pulling in host OS modules

#### Data Layout And ABI

- `union` declarations with typed fields (`i8`–`i64`, `u8`–`u64`, `f32`/`f64`, `bool`), keyword construction, and zero default init across all runtimes (interpreter, VM, LLVM)
- Interpreter/VM: `union` lowered to a class with zero-defaulted fields; all fields independently accessible
- LLVM: `[max_size x i8]` body, bitcast-based field access (all fields at offset 0), zero-arg ctor via `calloc`, kwarg construction emits inline stores
- LLVM-native `extern def` declarations with typed params/returns, optional `symbol:` aliasing, optional `cc:` calling-convention metadata, optional `section:` placement, first-class function binding, and matching interpreter/VM diagnostics
- LLVM-native ordinary `def` signatures with typed parameters and return types, native lowering for direct calls and first-class function values, `void` returns, and parser fixes so `lambda x: ...` stays unambiguous
- LLVM-native raw `data` declarations with typed primitive/struct initializers, linker-visible globals, address binding in Cool code, and optional `section:` placement for custom text/data layouts

#### Serial / Console Output Primitives

- `outb(port, byte)` — emit an x86 `OUT` instruction (write byte to I/O port); lowers to inline asm with no C runtime dependency; x86/x86-64 only with a clear error on other targets
- `inb(port)` — emit an x86 `IN` instruction (read byte from I/O port); returns the byte as an integer; same constraints as `outb`
- `write_serial_byte(byte)` — convenience wrapper for `outb(0x3F8, byte)`, hardwired to the COM1 UART data register; zero-dep freestanding-safe serial output for x86 bare-metal debugging
- All three are LLVM-only; interpreter and VM give the standard `compile with cool build` guidance; for MMIO-based serial (ARM, RISC-V) use the existing `write_u8_volatile()` primitives

#### Freestanding Build Mode

- `cool build --freestanding` now emits `.o` object files for single files or manifest-driven projects without compiling/linking the hosted Cool runtime
- Freestanding builds accept declaration-style top-level programs (`def`, `extern def`, `data`, `struct`, `union`) and reject top-level executable statements/imports/classes with explicit diagnostics
- Freestanding codegen now constructs basic `CoolVal` literals (`nil`, ints, floats, bools, strings) directly in LLVM IR so simple exported functions and extern wrappers do not require the hosted runtime just to materialize return values
- Freestanding LLVM `assert` failure paths now lower to a direct trap instead of importing libc `abort()` and hosted print helpers
- Freestanding top-level functions now accept `entry: "symbol"` metadata to export an additional raw entry symbol for custom link flows
- `cool build --linker-script=<path>` (implies `--freestanding`) compiles to a `.o` then invokes LLD (`ld.lld`) to link a kernel image (`.elf`) using a GNU linker script; linker is found by probing `ld.lld`, `lld`, and versioned variants; clear error when LLD is absent
- `linker_script = "link.ld"` field in `cool.toml` enables the same kernel image workflow for project builds; path is resolved relative to the project root; CLI flag takes precedence over the manifest field
- All 38 raw memory builtins (`read_byte`, `read_i8`–`read_f64`, their `_volatile` variants, and matching `write_*` forms) are now lowered directly to LLVM IR in freestanding mode; previously they dispatched to external C runtime symbols (`cool_write_u8_volatile` etc.) that were left undefined in the `.o`, making them silently unusable in freestanding builds

#### Self-Hosted Tooling

- Bundled Cool programs are now split by role: end-user apps live under `apps/`, while CLI subcommand implementations like `cool task` and `cool bundle` live under `cmd/`
- `cool new` now delegates to `cmd/new.cool`, moving project scaffolding out of Rust and into Cool itself
- `cool add` now delegates to `cmd/add.cool`, moving dependency manifest updates out of Rust and into Cool itself
- `cool install` now delegates to `cmd/install.cool`, moving dependency fetching and lockfile writing out of Rust and into Cool itself
- `cool bundle` now delegates to `cmd/bundle.cool`, moving packaging logic out of Rust and into Cool itself
- `cool release` now delegates to `cmd/release.cool`, moving version bumping, bundling, and git tagging out of Rust and into Cool itself
- `src/project.rs` now only keeps manifest parsing and module resolution; the old Rust-side add/install/lockfile helpers were removed after those flows moved into Cool
- Shared manifest/project helpers for bundled commands now live in `cmd/lib/projectlib.cool`, so the Cool-side CLI no longer copy-pastes root discovery and dependency parsing across commands

#### Editor Tooling

- First-party VS Code extension under `editors/vscode/`: `.cool` language registration, syntax highlighting, indentation rules, and `cool lsp` integration via `cool.lsp.serverCommand`

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

The interpreter and bytecode VM now share full context-manager cleanup semantics, and the LLVM backend also covers default/keyword arguments, closures/lambdas, inheritance, `super()`, slicing, `str()`, `isinstance()`, `try` / `except` / `else` / `finally`, `raise`, helpers like `min()`, `max()`, `sum()`, `round()`, `sorted()`, `abs()`, `int()`, `float()`, `bool()`, source-relative file imports like `import "helper.cool"`, project/package imports like `import foo.bar`, native `import ffi` (`ffi.open`, `ffi.func`), built-in `import math` / `import os` / `import sys` / `import path` / `import csv` / `import datetime` / `import hashlib` / `import toml` / `import yaml` / `import sqlite` / `import http` / `import subprocess` / `import argparse` / `import logging` / `import test` / `import time`, the core `random` helpers (`seed`, `random`, `randint`, `uniform`, `choice`, `shuffle`), `json.loads()` / `json.dumps()`, the built-in `string` helpers (`split`, `join`, `strip`, `lstrip`, `rstrip`, `upper`, `lower`, `replace`, `startswith`, `endswith`, `find`, `count`, `title`, `capitalize`, `format`), the pure `list` helpers (`sort`, `reverse`, `map`, `filter`, `reduce`, `flatten`, `unique`), the `re` helpers (`match`, `search`, `fullmatch`, `findall`, `sub`, `split`), `collections.Queue()` / `collections.Stack()`, native `open()` / file methods, and `with` / context managers on normal exit, control-flow exits (`return`, `break`, `continue`), caught exceptions, and unhandled native raises. Current backend coverage:

| Feature | Interpreter | Bytecode VM | LLVM |
| ------- | ----------- | ----------- | ---- |
| Classes | ✅ | ✅ | ✅ |
| `with` / context managers (normal/control-flow exits, caught exceptions, and unhandled native raises) | ✅ | ✅ | ✅ |
| Closures / lambdas | ✅ | ✅ | ✅ |
| `while` loops | ✅ | ✅ | ✅ |
| General `import` | ✅ | ✅ | ✅ |
| `try` / `except` / `finally` / `raise` | ✅ | ✅ | ✅ |
| FFI (`import ffi`) | ✅ | ❌ | ✅ |
| Inline asm | ❌ | ❌ | ✅ |

---

[1.0.0]: https://github.com/codenz92/cool-lang/releases/tag/v1.0.0
