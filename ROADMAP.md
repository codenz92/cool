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
- [x] `import os` (listdir, mkdir, remove, rename, exists, getcwd, join)
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
- [x] Tab completion
- [x] Up-arrow history navigation
- [x] Shell scripting (`source <file>` runs shell scripts line by line)
- [x] `alias` command

---

## Phase 6 — Standard Library ✅

> Goal: a built-in library written in Cool itself

- [x] `string` module — `split`, `join`, `strip`, `upper`, `lower`, `replace`, etc.
- [x] `list` module — `sort`, `reverse`, `map`, `filter`, `reduce`, `flatten`, `unique`
- [x] `math` module (expanded) — `gcd`, `lcm`, `factorial`, `hypot`, `degrees`, `radians`, `sinh`, `cosh`, `tanh`, etc.
- [x] `json` module — `json.loads()` / `json.dumps()` with full JSON support
- [x] `re` module — `re.match()`, `re.search()`, `re.fullmatch()`, `re.findall()`, `re.sub()`, `re.split()`
- [x] `time` module — `time.time()`, `time.sleep()`, `time.monotonic()`
- [x] `random` module — `random.random()`, `random.randint()`, `random.choice()`, `random.shuffle()`, `random.uniform()`, `random.seed()`
- [x] `collections` module — `Queue` and `Stack` classes (written in Cool itself)
- [x] Package system — `import foo.bar` loads `foo/bar.cool` from source directory

---

## Phase 7 — Cool Applications ✅

> Goal: write real apps entirely in Cool

- [x] `calc` — calculator REPL with persistent variables, full math library support
- [x] `notes` — note-taking app (new, show, append, delete, search commands)
- [x] `top` — process/task viewer using `ps aux` and system stats
- [x] `edit` — nano-like text editor (arrow keys, Ctrl+S save, Ctrl+X exit)
- [x] `snake` — Snake game (ASCII, arrow keys, real-time with raw terminal mode)
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

The LLVM backend now covers most day-to-day language features, including default/keyword arguments, inheritance, `super()`, slicing, `str()`, `isinstance()`, helpers like `min()`, `max()`, `sum()`, `round()`, `sorted()`, `abs()`, `int()`, `float()`, `bool()`, built-in `import math` / `import os` / `import sys` / `import time`, the core `random` helpers (`seed`, `random`, `randint`, `uniform`, `choice`, `shuffle`), `json.loads()` / `json.dumps()`, the built-in `string` helpers (`split`, `join`, `strip`, `lstrip`, `rstrip`, `upper`, `lower`, `replace`, `startswith`, `endswith`, `find`, `count`, `title`, `capitalize`, `format`), and the pure `list` helpers (`sort`, `reverse`, `flatten`, `unique`). The following features still remain interpreter/VM-only for now:

| Feature | Interpreter | Bytecode VM | LLVM |
| ------- | :-----------: | :-----------: | :----: |
| Classes | ✅ | ✅ | ✅ |
| Ternary expressions | ✅ | ✅ | ✅ |
| List comprehensions | ✅ | ✅ | ✅ |
| `in` / `not in` | ✅ | ✅ | ✅ |
| Dicts | ✅ | ✅ | ✅ |
| Tuples | ✅ | ✅ | ✅ |
| Closures / lambdas | ✅ | ✅ | ❌ |
| General `import` | ✅ | ✅ | ❌ |
| `try` / `except` | ✅ | ✅ | ❌ |
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
