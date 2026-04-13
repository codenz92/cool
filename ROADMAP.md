# Cool Language & CoolOS Roadmap

## Legend
- [x] Done
- [~] Partial / needs more work
- [ ] Not started

---

## Phase 1 — Cool Language: Core Interpreter ✅
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

## Phase 2 — Cool Language: Real Language Features ✅
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

## Phase 3 — CoolOS Shell ✅
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
- [x] Dict-based command dispatcher
- [x] Relative path resolution (`join_path`)

---

## Phase 4 — Cool Language: Quality of Life ✅
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

## Phase 5 — CoolOS Shell: More Commands 🔄
> Goal: a shell powerful enough for real use

- [x] `cp <src> <dst>` — copy a file
- [x] `grep <pattern> <file>` — search file contents
- [x] `head <file> [n]` / `tail <file> [n]` — first/last N lines
- [x] `wc <file>` — word/line/char count
- [ ] `find <pattern>` — search for files by name
- [ ] Pipes: `ls | grep cool`
- [ ] Environment variables (`set VAR=value`, `$VAR`)
- [ ] Tab completion
- [ ] Up-arrow history navigation
- [ ] Shell scripting (run `.cool` scripts as shell scripts)
- [ ] `alias` command

---

## Phase 6 — Cool Language: Standard Library
> Goal: a built-in library written in Cool itself

- [~] `string` module — core methods already available natively on `str`; module form with `split`, `join`, `strip`, `upper`, `lower`, `replace`, `startswith`, `endswith` not yet importable as `import string`
- [ ] `list` module (sort, reverse, filter, map, reduce as module-level functions)
- [ ] `math` module (expand beyond current basics)
- [ ] `json` module (parse and serialize JSON)
- [ ] `re` module (basic regex)
- [ ] `time` module (timestamp, sleep)
- [ ] `random` module
- [ ] `collections` module (queue, stack, ordered dict)
- [ ] Package system (`import` from subdirectories)

---

## Phase 7 — CoolOS Applications
> Goal: write real apps entirely in Cool that run inside CoolOS

- [ ] `edit` — a simple text editor (like nano)
- [ ] `calc` — a calculator REPL
- [ ] `top` — a process/task viewer
- [ ] `notes` — a simple note-taking app
- [ ] `snake` — Snake game (ASCII)
- [ ] `http` — simple HTTP client (`http get <url>`)

---

## Phase 8 — Cool Language: Compiler (Long Term)
> Goal: compile Cool to native binaries so CoolOS can be self-hosted

- [ ] Bytecode VM (compile AST to bytecode, run on a VM)
- [ ] LLVM backend (compile Cool to LLVM IR → native binary)
- [ ] FFI (call C functions from Cool)
- [ ] Inline assembly (`asm { ... }`)
- [ ] Pointer types / raw memory access
- [ ] `cool build` command (compile a `.cool` project)

---

## Phase 9 — CoolOS: Real Kernel (Very Long Term)
> Goal: a real OS that boots on bare metal and runs Cool as its shell

- [ ] Bootloader (GRUB or custom)
- [ ] Kernel written in Rust (memory management, interrupts, scheduler)
- [ ] VGA / framebuffer text output
- [ ] Keyboard driver
- [ ] Filesystem driver (read/write disk)
- [ ] Process scheduler
- [ ] Cool interpreter embedded in the kernel
- [ ] CoolOS shell boots as PID 1
- [ ] Self-hosting: CoolOS can run `cool build` to recompile itself

---

## Summary

| Phase | Status |
| ----- | ------ |
| 1 — Core Interpreter | ✅ Complete |
| 2 — Real Language Features | ✅ Complete |
| 3 — CoolOS Shell | ✅ Complete |
| 4 — Quality of Life | ✅ Complete |
| 5 — Shell: More Commands | 🔄 In progress |
| 6 — Standard Library | ⏳ Planned |
| 7 — CoolOS Applications | ⏳ Planned |
| 8 — Compiler | ⏳ Long term |
| 9 — Real Kernel | ⏳ Very long term |
