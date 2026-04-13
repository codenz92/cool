# Cool

A Python-inspired scripting language and interactive OS shell, implemented in Rust.

Cool is a tree-walk interpreted language with Python-like syntax — indentation-based blocks, classes, closures, f-strings, list comprehensions, and more — built on a clean Rust runtime. It also ships with **CoolOS**, a fully-featured interactive shell written entirely in Cool itself.

---

## Features

**Language**

- Indentation-based block syntax (Python-style)
- Variables, arithmetic, comparisons, logical and bitwise operators
- `if` / `elif` / `else`, `while`, `for`, `break`, `continue`
- Functions with default args, `*args`, keyword args, closures
- Classes with inheritance, `super()`, operator overloading (`__add__`, `__str__`, etc.)
- Lists, dicts, tuples, sets with full method support
- Slicing (`lst[1:3]`, negative indices)
- `try` / `except` / `else` / `finally`, `raise`
- f-strings, multi-line strings, `string.format()`
- List comprehensions, lambda expressions, ternary expressions
- `nonlocal` / `global`, `assert`, `with` / context managers
- `import math`, `import os`, `import sys`
- File I/O via `open()`, `read()`, `write()`, `readlines()`
- `runfile()` to execute another `.cool` file at runtime
- Hex / binary / octal literals, `\x` escape sequences
- REPL mode

**CoolOS Shell** (`coolos/shell.cool`)

A fully interactive shell written in Cool:

```
ls [path]          cd <path>          pwd
cat <file>         mkdir <dir>        touch <file>
rm <file>          mv <src> <dst>     cp <src> <dst>
head <file> [n]    tail <file> [n]    wc <file>
grep <pat> <file>  echo <text>        write <file> <text>
run <file.cool>    history            clear
```

---

## Getting Started

**Prerequisites:** Rust (stable, edition 2024)

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
```

More examples are in the [`examples/`](examples/) directory.

---

## Project Structure

```
src/
  lexer.rs        Token scanner with INDENT/DEDENT handling
  parser.rs       Recursive descent parser → AST
  ast.rs          AST node definitions
  interpreter.rs  Tree-walk interpreter
  main.rs         CLI entry point and REPL

coolos/
  shell.cool      The CoolOS interactive shell

examples/
  hello.cool
  features.cool   Comprehensive language feature demo
  math_utils.cool
  files.cool
  phase4_features.cool
  v3_features.cool
```

---

## Roadmap

| Phase | Status |
|---|---|
| 1 — Core interpreter | ✅ Complete |
| 2 — Real language features | ✅ Complete |
| 3 — CoolOS shell | ✅ Complete |
| 4 — Quality of life (f-strings, lambdas, comprehensions…) | ✅ Complete |
| 5 — Shell: more commands | 🔄 In progress |
| 6 — Standard library (string, json, re, time, random…) | ⏳ Planned |
| 7 — CoolOS applications (editor, calculator, snake…) | ⏳ Planned |
| 8 — Bytecode VM / LLVM compiler | ⏳ Long term |
| 9 — Real kernel (bare-metal, self-hosting) | ⏳ Very long term |

See [`ROADMAP.md`](ROADMAP.md) for the full breakdown.

---

## License

MIT
