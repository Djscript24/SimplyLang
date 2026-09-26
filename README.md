# Simply

Simply is a small, statically checked programming language and interpreter
implemented in Rust. It is designed for readable programs, predictable
runtime behavior, useful diagnostics, and a compact command-line workflow.

> **Project status:** Simply is under active development. The current
> implementation prioritizes correctness, deterministic behavior, and measured
> optimizations over a large feature surface.

## Highlights

- Newline-oriented `.si` source files with concise syntax.
- Type inference with optional explicit annotations.
- Immutable bindings by default; use `mut` for reassignment or mutation.
- Functions with typed parameters and optional return types.
- Nested functions and escaping closures with lexical binding resolution.
- Arrays, lists, tuples, hashes, trees, and matrices.
- Pipelines with `where`, `derive`, `partition`, and aggregate terminals.
- Streaming CSV pipelines for selecting, deriving, and rewriting large files.
- Flow chunking and file checkpoints for resumable long-running pipelines.
- Parallel scalar `where`/`derive` flow workers with deterministic ordered merge
  and explicit rejection of unsupported expressions.
- Relative imports with isolated module evaluation.
- Structured lexer, parser, semantic, and runtime diagnostics.
- Formatter, REPL, native Simply tests, and runtime benchmarks.
- Deterministic hash/tree iteration and copy-on-write collection storage.

## Quick Start

### Requirements

- Rust toolchain with Cargo.
- Rust edition 2024 support.

### Run a program

```bash
cargo run -- run examples/99-smoke/smoke.si
```

### Check without executing

```bash
cargo run -- check examples/99-smoke/smoke.si
```

### Format a program

```bash
cargo run -- fmt examples/99-smoke/smoke.si
```

### Explain a Flow

```bash
cargo run -- explain-flow path/to/program.si
```

This validates the source and prints a deterministic execution plan for each
declarative `flow`, including its source kind, operators, terminal, execution
mode, and fusion availability. Programs without flows
are reported explicitly.

### Run the test suite

```bash
cargo test
cargo run -- test
```

`cargo test` runs Rust unit and integration tests. `cargo run -- test`
discovers direct `.si` files in the project's `tests/` directory and executes
them through the normal Simply evaluator.

### Build or install

```bash
cargo build --release
cargo install --path .
```

After installation, use `simply` instead of `cargo run --`, for example:

```bash
simply run path/to/program.si
```


## Language Basics

Simply statements are normally separated by newlines, and blocks close with
`end`. Comments start with `#` outside strings.

### Bindings and mutability

Bindings are immutable by default. A binding must be declared with `mut` when
it will be reassigned or used for collection mutation:

```simply
name is "Simply"
version as Float is 3.14159

mut counter as Int is 0
counter -> counter + 1

mut values as List[Int] is list [1, 2, 3]
values add 4
values[0] -> 10
```

Reassignment uses `->` and must preserve the binding's inferred or declared
type. Indexed writes and list `add`/`remove` operations also require `mut`.

### Type inference and annotations

The type of an unannotated binding is inferred from its initial value:

```simply
title is "Hello"       # String
count is 42            # Int
ratio is 0.5           # Float
enabled is true        # Bool
numbers is list [1, 2] # List[Int]
```

Supported type forms include:

```text
String  Int  Float  Bool  Hash  Tree  Matrix
Array[T]  List[T]  Tuple[T1, T2, ...]
```

### Control flow

```simply
if score >= 90:
    Say "excellent"
else if score >= 60:
    Say "passed"
else:
    Say "try again"
end

for item in values:
    Say item
end

while counter < 3:
    Say counter
    counter -> counter + 1
end
```

Loop variables are immutable by default. Use `for mut item in values:` only
when the loop variable itself must be reassigned.

### Functions

```simply
fn add(left as Int, right as Int) gives Int:
    return left + right
end

result is add(2, 3)
Say result
```

Function parameters are immutable by default and may be prefixed with `mut`.
Typed functions must return a compatible value on every possible execution
path. A function without a return annotation produces `Unit` if it reaches the
end without returning a value.

### Collections and matrices

```simply
cities is array ["Jakarta", "Bandung"]
queue is list ["first", "second"]
profile is hash:
    name is "Ada"
    role is "builder"
end
coordinates is (12, 30)
grid is matrix [[1, 2], [3, 4]]
```

Arrays are fixed-length values, while lists support mutation through mutable
bindings. Hashes and trees use string keys and iterate deterministically.
Matrix dimensions and numeric contents are validated at runtime. Matrix
multiplication returns floating-point cells.

### Pipelines

Pipelines make collection transformations readable:

```simply
numbers is list [1, 2, 3, 4, 5, 6]
total is pipeline:
    numbers
    where item > 3
    derive item * 2
    sum
end
Say total
```

The evaluator fuses supported `where`/`derive` chains ending in `sum` or
`count`, avoiding intermediate collection materialization where possible.

### Imports

Import a Simply source file relative to the importing file:

```simply
open "math.si" as math
```

Imported programs are evaluated with isolated module state and their parsed
programs are cached for reuse.

## Built-in Functions

The standard library includes:

| Function | Purpose |
| --- | --- |
| `range(start, end)` | Create a lazy integer sequence. |
| `length(value)` / `count(value)` | Get collection or string size. |
| `contains(collection, value)` | Test collection membership or a string substring. |
| `any(collection)` / `all(collection)` | Evaluate boolean collections. |
| `join(collection, separator)` | Join string values. |
| `total(collection)` | Sum numeric arrays, lists, or tuples. |
| `trim`, `split`, `replace` | Transform strings. |
| `starts_with`, `ends_with` | Test string prefixes and suffixes. |
| `is_empty(value)` | Test collections and strings. |
| `reverse(sequence)` | Reverse arrays, lists, or tuples. |
| `type_of(value)` / `print(value)` | Inspect values or print them. |
| `send(receiver, message, ...)` | Dispatch a function by message name. |

## Command Reference

```bash
simply run <file.si>       # Execute a source file
simply check <file.si>     # Lex, parse, and analyze without execution
simply fmt <file.si>       # Format source text
simply tokens <file.si>    # Print lexer tokens
simply ast <file.si>       # Print the parsed AST
simply bench <file.si>     # Measure front-end and runtime phases
simply test                # Run direct tests/*.si files
simply repl                # Start the interactive evaluator
simply --help              # Show command usage
simply --version           # Show the installed version
```

Commands are intentionally explicit: `fmt` and `check` are commands, not
duplicated flag aliases. The formatter writes the result to standard output,
so it can be reviewed or redirected safely.

The benchmark command reports average lexing, parsing, semantic-analysis,
runtime, and total time over repeated iterations. Runtime output is suppressed
while measuring.

## Diagnostics and Errors

Simply reports structured diagnostics instead of panicking on user input.
Diagnostics include a category, stable code, source span, source path, source
line, and caret context when available.

The main categories are:

- `Lex` — invalid characters and malformed strings.
- `Parse` — invalid statement or expression syntax.
- `Semantic` — undefined names, invalid types, scopes, or control flow.
- `Runtime` — failures that depend on evaluated values.

Use `check` when static validation is required before execution. Commands exit
non-zero when source validation or runtime execution fails.

## Runtime Model

The implementation follows a conventional interpreter pipeline:

1. The lexer converts source text into tokens.
2. The parser builds an abstract syntax tree.
3. The semantic analyzer validates types, scopes, declarations, and control flow.
4. The evaluator executes the program.
5. Runtime modules implement values, operations, collections, and scopes.

Collections use reference-counted copy-on-write storage. Sharing a collection
is cheap; a mutation detaches storage when another binding still references it.
Function bodies and imported programs use shared storage where appropriate.

## Repository Layout

```text
.
├── src/                    # Rust interpreter implementation
├── docs/                   # Language, semantics, project, and performance docs
├── examples/               # Runnable programs grouped by feature and workload
│   ├── 01-09-*/            # Language and standard-library examples
│   ├── 10-flow/            # Declarative Flow, quality, CSV, and resume demos
│   ├── 99-smoke/           # Small smoke program
│   └── 99-bench/           # Benchmark programs and large input fixtures
├── tests/                  # Native Simply fixtures and Rust integration tests
├── Cargo.toml              # Rust package metadata
└── LICENSE                 # MIT license
```

## Documentation

- [Project documentation](docs/project.md) — project overview and layout.
- [Language reference](docs/language.md) — statements, expressions, operators,
  built-ins, and language behavior.
- [Type system](docs/types.md) — inference, annotations, compatibility, and
  numeric/matrix rules.
- [Grammar](docs/grammar.md) — implementation-oriented syntax summary.
- [Semantics](docs/semantics.md) — scopes, mutability, imports, and evaluation.
- [Errors and diagnostics](docs/errors.md) — error categories and stable codes.
- [Performance](docs/performance.md) — optimizations and benchmark workloads.
- [Project contract](docs/project.md) — project layout and test discovery.
- [Examples](examples/) — runnable programs organized by feature.
- [Tests](tests/) — native fixtures and Rust integration tests.

## Development and Validation

Before submitting changes, run:

```bash
cargo fmt -- --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo run -- test
git diff --check
```

Performance changes should be measured in release mode with the workloads under
`examples/99-bench/`, especially:

```bash
cargo run --release -- bench examples/99-bench/runtime-pipeline.si
cargo run --release -- bench examples/99-bench/function-dispatch.si
cargo run --release -- bench examples/99-bench/closure-capture.si
cargo run --release -- bench examples/99-bench/large-pipeline.si
cargo run --release -- bench examples/99-bench/hash-lookup.si
cargo run --release -- bench examples/99-bench/matrix-workload.si
```

Inspect a valid program without executing it:

```bash
simply explain examples/05-functions/closures.si
```

## License

Simply is distributed under the [MIT License](LICENSE).
