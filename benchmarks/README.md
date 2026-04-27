## Native Benchmarks

This suite compares Cool native binaries built with `cool build` against matched Rust binaries built with `rustc -O`.

For project-local native benchmarking of `.cool` files, use `cool bench`. This document covers the repo-maintainer comparison harness for Cool-vs-Rust workload matching.

It targets the LLVM/native backend only. The interpreter and bytecode VM are intentionally out of scope because they answer a different question.

### Workloads

- `integer_loop`: integer-heavy loop with arithmetic, modulo, and bitwise work
- `string_processing`: repeated `count` / `find` / `replace` over the same string
- `list_dict`: list append plus dict set/get passes
- `raw_memory`: explicit `malloc` / `write_i64` / `read_i64` / `free`

### Run

```bash
cargo run --release --bin bench_compare -- --runs 5 --warmups 1
```

Useful options:

- `--filter <name>` to run a single workload
- `--runs <n>` to change the number of measured runs
- `--warmups <n>` to change the number of warmup runs

The runner:

1. Builds `target/release/cool`
2. Compiles the Cool benchmark programs
3. Compiles the Rust benchmark programs
4. Verifies that each Cool/Rust pair prints the same result
5. Reports compile times, per-language runtime summaries, and the Cool/Rust runtime ratio
