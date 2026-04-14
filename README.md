# Cool

A Python-inspired scripting language and interactive OS shell, implemented in Rust.

Cool is a tree-walk interpreted language with Python-like syntax — indentation-based blocks, classes, closures, f-strings, list comprehensions, and more — built on a clean Rust runtime. It also ships with **CoolOS**, a fully-featured interactive shell written entirely in Cool itself.

---

## Features

### Language

- Indentation-based block syntax (Python-style)
- Variables, arithmetic, comparisons, logical and bitwise operators
- `if` / `elif` / `else`, `while`, `for`, `break`, `continue`
- Functions with default args, `*args`, `**kwargs`, keyword args, closures
- Classes with inheritance, `super()`, operator overloading (`__add__`, `__str__`, `__eq__`, `__len__`, etc.)
- Lists, dicts, tuples with full method support
- `set()` built-in (returns deduplicated list)
- Slicing (`lst[1:3]`, negative indices)
- `try` / `except` / `else` / `finally`, `raise`
- f-strings, multi-line strings, `string.format()`
- List comprehensions, lambda expressions, ternary expressions
- `nonlocal` / `global`, `assert`, `with` / context managers
- `import math`, `import os`, `import sys`
- `import string`, `import list`, `import json`, `import re`, `import time`, `import random`, `import collections`
- Package system: `import foo.bar` loads `foo/bar.cool`
- File I/O via `open()`, `read()`, `write()`, `readlines()`
- `runfile()` to execute another `.cool` file at runtime
- `eval(str)` to evaluate a Cool expression or statement at runtime
- `import term` for raw terminal mode, cursor control, and real-time key input (powered by crossterm)
- `os.popen(cmd)` to run shell commands and capture output
- Hex / binary / octal literals, `\x` escape sequences
- REPL mode

### Built-in Functions

`print()`, `input()`, `str()`, `int()`, `float()`, `bool()`, `len()`, `range()`, `type()`, `repr()`, `abs()`, `min()`, `max()`, `sum()`, `round()`, `pow()`, `sorted()`, `reversed()`, `enumerate()`, `zip()`, `map()`, `filter()`, `list()`, `tuple()`, `dict()`, `set()`, `isinstance()`, `hasattr()`, `getattr()`, `assert`, `exit()`

### String Methods

`.upper()`, `.lower()`, `.strip()`, `.lstrip()`, `.rstrip()`, `.split()`, `.join()`, `.replace()`, `.find()`, `.count()`, `.startswith()`, `.endswith()`, `.format()`

### CoolOS Shell (`coolos/shell.cool`)

A fully interactive shell written in Cool:

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
- Tab completion and up-arrow history navigation
- Shell scripting via `source <file>`

---

## Getting Started

**Prerequisites:** Rust (stable, edition 2021)

```bash
# Build
cargo build --release

# Run a file
./target/release/cool hello.cool

# Start the REPL
./target/release/cool

# Launch CoolOS shell
./target/release/cool coolos/shell.cool
```

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
```

More examples are in the [`examples/`](examples/) directory.

---

## Project Structure

```text
src/
  lexer.rs        Token scanner with INDENT/DEDENT handling
  parser.rs       Recursive descent parser → AST
  ast.rs          AST node definitions
  interpreter.rs  Tree-walk interpreter
  main.rs         CLI entry point and REPL

coolos/
  shell.cool      The CoolOS interactive shell
  calc.cool       Calculator REPL
  notes.cool      Note-taking app
  top.cool        Process viewer
  edit.cool       Text editor
  snake.cool      Snake game
  http.cool       HTTP client

examples/
  hello.cool            Variables, loops, functions — start here
  data_structures.cool  Lists, dicts, tuples, comprehensions
  oop.cool              Classes, inheritance, operator overloading
  functional.cool       Closures, lambdas, map/filter, memoize
  errors_and_files.cool try/except/finally, file I/O, JSON, dirs
  stdlib.cool           math, random, re, json, time, collections
```

---

## Roadmap

| Phase | Status |
| ----- | ------ |
| 1 — Core interpreter | ✅ Complete |
| 2 — Real language features | ✅ Complete |
| 3 — CoolOS shell | ✅ Complete |
| 4 — Quality of life (f-strings, lambdas, comprehensions…) | ✅ Complete |
| 5 — Shell: more commands | ✅ Complete |
| 6 — Standard library (json, re, time, random…) | ✅ Complete |
| 7 — CoolOS applications (editor, calculator, snake…) | ✅ Complete |
| 8 — Bytecode VM / LLVM compiler | ⏳ Long term |
| 9 — Real kernel (bare-metal, self-hosting) | ⏳ Very long term |

See [`ROADMAP.md`](ROADMAP.md) for the full breakdown.

---

## License

MIT
