# Simply

Simply is a small, statically checked programming language and interpreter
implemented in Rust. It is designed for readable programs, predictable
runtime behavior, useful diagnostics, and a compact command-line workflow.

> **Project status:** Simply is under active development. The current
> implementation prioritizes correctness, deterministic behavior, and measured
> optimizations over a large feature surface.

## Highlights

- Newline-oriented `.si` source files with concise syntax.
- `Say` output without a newline and `Sayln` output with a newline.
- Type inference with optional explicit annotations.
- Immutable bindings by default; use `mut` for reassignment or mutation.
- Functions with typed parameters and optional return types.
- Nested functions and escaping closures with lexical binding resolution.
- Arrays, lists, tuples, hashes, trees, and matrices.
- Tuple and sequence destructuring declarations with nested targets, wildcards,
  and lazy rest suffixes.
- Structural match patterns for enums, positional and named-field structs,
  sequences, partial hashes, and whole-value aliases.
- Pipelines with `where`, `derive`, `take_while`, `drop_while`, `skip`, `distinct`,
  `step_by`, boolean `any`/`all` terminals, `partition`, and aggregate terminals.
- Streaming CSV pipelines for selecting, deriving, and rewriting large files.
- Flow chunking and file checkpoints for resumable long-running pipelines.
- Parallel scalar `where`/`derive` flow workers with deterministic ordered merge
  and explicit rejection of unsupported expressions.
- Relative imports with isolated evaluation and explicit named exports.
- Structured lexer, parser, semantic, and runtime diagnostics.
- Formatter, REPL, native Simply tests, and runtime benchmarks.
- Deterministic hash/tree iteration and copy-on-write collection storage.
- String indexing, slicing, character inspection, and text file I/O provide
  foundations for future self-hosted compiler development.
- Scalar mathematics, vector and matrix operations, and population statistics
  provide a numerical foundation for future scientific and ML-oriented work.

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

## Security model

Simply is a trusted local scripting language, not a sandbox. A `.si` program
can read and write files and otherwise access resources available to the
operating-system process running it. Only run programs from sources you trust;
Simply does not restrict filesystem access to the project or source directory.

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
    Sayln "excellent"
else if score >= 60:
    Sayln "passed"
else:
    Sayln "try again"
end

for item in values:
    Sayln item
end

while counter < 3:
    Sayln counter
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
Sayln result
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

Arrays are fixed-length ordered values; both arrays and lists support indexed
access and writes through mutable bindings, while only lists support `add` and
`remove`. Tuples are fixed-length and heterogeneous. Hashes and trees use
string keys and sorted, deterministic iteration; hashes are mutable and trees
are read-only. Matrices use two-dimensional row/column indexing and require
rectangular numeric rows for matrix operations. Ranges are lazy half-open
integer sequences. Matrix multiplication returns floating-point cells.

### Structs and messages

Structs declare nominally typed, positional data. Messages can read and update
their receiver's fields, and their return value is an ordinary expression
value:

```simply
type Person:
    name as String
    age as Int
end

on Person receive greet:
    return "Hello " + name
end

on Person receive introduce(to as String):
    return "Hello " + to + ", I'm " + name
end

on Person receive rename(new_name as String):
    name -> new_name
end

on Person receive get_age:
    return age
end

person is Person("Budi", 17)
person :: rename("Ada")
name is person :: greet
age is person :: get_age
Sayln name
Sayln age
```

Inside a message, declared fields are available as local bindings. Reassigning
one updates that field on the persistent instance; its declared type is
enforced. Struct values share identity when copied to another binding, so
messages sent through either binding observe the same state. Message dispatch
works both as a statement and as an expression. State is accessed through
messages; direct struct field access, automatic getters, and setters are not
supported.

### Enums and matching

Enums are nominal tagged values with zero or one typed payload per variant.
`match` is an expression, and enum matches must cover every variant or include
an irrefutable fallback pattern:

```simply
enum Result:
    Ok as Int
    Error as String
end

result is Result::Ok(42)
message is match result:
    Result::Ok(value):
        value
    Result::Error(error):
        error
end
Sayln message
```

Struct patterns match fields positionally in declaration order and compose
with enum and tuple patterns:

```simply
type Person:
    name as String
    age as Int
end

enum Result:
    Ok as Person
    Error as String
end

result is Result::Ok(Person("Andi", 17))
match result:
    Result::Ok(Person(name, _)):
        Sayln name
    Result::Error(error):
        Sayln error
end
```

Patterns recursively support identifiers, `_`, tuples, and enum variants with
an optional single payload pattern, plus positional Struct patterns. For
example, `(left, (right, _))` matches nested tuples,
`Result::Ok((left, right))` destructures a tuple payload, and
`Person(name, Address(city))` matches Struct fields in declaration order.
Struct patterns require the nominal type. Positional patterns require the
exact field count; named-field patterns can select fields by name.
Identifier bindings exist only within their selected arm; a failed nested
pattern does not expose any partial bindings.
Enum payloads preserve nominal Struct and Enum values; a Struct stored in a
payload retains its shared identity and state. Enum values do not support
message dispatch.

Match patterns are type-checked against the scrutinee, including tuple arity,
nominal enum identity, and Struct identity/arity. Exhaustiveness and
unreachable-arm checks recursively account for enum variants, tuple elements,
and Struct fields. Wildcards, identifiers, and recursively irrefutable
constructor patterns cover their expected type. An arm may add
`if expression` after its pattern; the guard runs only after a successful
pattern, with that arm's bindings in scope, and must evaluate to `Bool`. A
false guard continues to the next arm. Guarded patterns do not count toward
exhaustiveness, and SimplyLang does not reason symbolically about guard
conditions. OR-patterns use `pattern | pattern`; alternatives are tried
left-to-right and must bind the same names with compatible types. Unguarded OR
alternatives contribute their combined coverage, while a guarded OR contributes
none.

Literal patterns support Int, Float, String, and Bool values, including inside
tuples, enum payloads, Struct fields, and OR-patterns. They match exactly with
no numeric coercion; strings use exact equality. Bool is a finite domain, so
`true` and `false` together are exhaustive. Float and String are open domains
and still require a wildcard or identifier for exhaustiveness. Int literal and
inclusive range patterns such as `-10..10`, `10..`, and `..10` are analyzed as
intervals. Their unguarded union is exhaustive only when it covers the entire
Int domain without gaps; guarded patterns do not contribute coverage.

Sequence patterns use brackets, such as `[]`, `[1, 2]`, and
`[[1, 2], [3, 4]]`. Fixed-length patterns match Array or List values only at
exact length. A trailing identifier rest binding, such as `[head, ...tail]`,
matches any length at least as large as its prefix and binds the suffix using
the source collection type. Rest appears at most once and must be last;
`[...rest]` matches sequences of any length. Array and List suffixes preserve
their collection kind, integer ranges preserve their lazy range representation,
and CSV stream suffixes remain lazy. Patterns recursively support nested
constructors and range/literal checks. Fixed-length patterns alone do not cover
arbitrary sequence lengths; use a rest pattern or wildcard for the remainder.

### Destructuring declarations

Declarations can extract values using nested tuple and sequence targets:

```simply
(name, (age, _)) is ("Ada", (37, "ignored"))
[head, ...tail] is [1, 2, 3]
```

Destructuring supports identifiers, `_`, tuples, sequences, and a final
identifier rest target. Bindings are committed only after the whole target
matches; CSV rest values remain lazy. Literal, Range, OR, Alias, Enum, Struct,
and Hash patterns are not destructuring targets.

Destructuring assignment updates existing mutable bindings atomically:

```simply
mut name is "Ada"
mut age is 36
(name, age) -> ("Grace", 37)
```

Every target must already exist, be mutable, and accept the extracted value's
type. Wildcards are ignored; duplicate targets are rejected. The RHS is
evaluated once, and no target changes unless the complete target and all
assignments validate. CSV rest suffixes stay lazy.

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
Sayln total
```

Ordered pipeline steps also include `take N`, `skip N`, `step_by N`,
`take_while condition`, `drop_while condition`, and `distinct`. They compose in
source order and stream lazy ranges and CSV input; order-dependent steps cannot
be combined with `parallel` or `checkpoint`.
`step_by N` keeps the first item and then every Nth item reaching that step;
N must be a positive integer.
The `any` and `all` terminals reduce Boolean pipeline items to a single Boolean,
short-circuiting on the first `true` or `false` respectively. They return `false`
and `true` for empty input.

The evaluator fuses supported `where`/`derive` chains ending in `sum` or
`count`, avoiding intermediate collection materialization where possible.

### Compiler foundations

Strings support Unicode-scalar indexing and slicing, linear-time traversal
after one `characters(text)` conversion, and inspection with `is_ascii_alpha`,
`is_ascii_digit`, and `is_whitespace`. `read_file` and `write_file` handle text
files; `Ask(prompt[, type])` reads user input. The
`examples/11-compiler-foundations/mini-lexer.si` example reads a Simply source
file, scans it one character at a time, and builds token-like values. These
features support future self-hosted compiler development; Simply is not
self-hosted.

### Imports

Import a Simply source file relative to the importing file:

```simply
open "math.si" as math
```

An imported module must return a value; that value is bound to the alias. For
example, a module can return a function for the importer to call. Module-local
function, struct, enum, and message declarations are not imported into the
caller's declaration scope. Modules may also expose selected values with
`export name, other`; use `open "module.si" exposing name, other` to bind only
those named exports directly. Imported names may be renamed with `as`, for
example `open "math.si" exposing add as add_numbers`. Structs and enums can
also be exported and imported this way; their nominal identity and struct
messages are preserved across aliases.

Relative paths use the importing file's directory, including for nested
imports. Absolute paths are also accepted. Canonical paths identify modules
for cycle detection and parsed-program reuse. At runtime each import executes
the module in an isolated evaluator; parsing is cached, but execution and
module-local state are not shared between import occurrences. `simply check`
recursively checks imported source without executing it.

## Built-in Functions

The following table highlights commonly used built-ins; see the
[language reference](docs/language.md) for the complete behavior and
signatures.

| Function | Purpose |
| --- | --- |
| `assert(condition[, message])` | Check a condition and fail with an optional diagnostic message. |
| `range(start, end[, step])` | Create a lazy integer sequence with an optional non-zero step. |
| `length(value)` | Get collection or string size; `count(value)` is a compatibility alias. |
| `enumerate(sequence)` | Return zero-based `(index, value)` pairs for sequences and strings. |
| `zip(left, right)` | Pair sequence values into tuples, stopping at the shorter input. |
| `contains(collection, value)` | Test collection membership or a string substring. |
| `has_key(map, key)` | Test whether a Hash or Tree contains a string key. |
| `get(map, key, default)` | Get a map value or lazily evaluate a fallback. |
| `without_key(map, key)` | Return a Hash or Tree copy with the named key removed. |
| `select_keys(map, keys)` | Copy a map with only the requested string keys. |
| `keys(map)` / `values(map)` / `entries(map)` | Return map keys, values, or `(key, value)` tuples in deterministic key order. |
| `any(collection)` / `all(collection)` | Reduce Boolean collections; pipeline terminals also work on transformed lazy sources. |
| `join(collection, separator)` | Join string values. |
| `total(collection)` | Sum numeric arrays, lists, tuples, or ranges. |
| `sum` pipeline terminal | Sum transformed pipeline values, including CSV streams. |
| `count` pipeline terminal | Count items reaching the end of a transformed pipeline. |
| `trim`, `split`, `replace` | Transform strings. |
| `starts_with`, `ends_with` | Test string prefixes and suffixes. |
| `characters(text)` | Materialize a string as an array of scalar strings. |
| `substring(text, start, length)` | Slice a string by Unicode scalar positions. |
| `to_float(value)`, `to_int(value)` | Convert numeric values or parse numeric strings. |
| `is_ascii_alpha`, `is_ascii_digit`, `is_whitespace` | Inspect one character. |
| `read_file(path)`, `write_file(path, content)` | Read and write UTF-8 text files. |
| `csv_rows(path)`, `csv_row(...)` | Read lazy CSV rows or construct a mixed-type output row. |
| `Ask(prompt[, type])` | Read a line as String or parse it as Int, Float, String, or Bool. |
| `sqrt`, `pow`, `exp`, `log`, `log10`, `sin`, `cos`, `tan` | Scalar mathematical functions. |
| `floor`, `ceil`, `sign`, `abs`, `round`, `clamp` | Numeric rounding, sign, and bounds operations. |
| `vector_add`, `vector_subtract`, `vector_scale`, `dot`, `norm`, `distance`, `normalize` | Numeric vector operations. |
| `shape`, `transpose`, `matrix_add`, `matrix_subtract`, `matrix_scale`, `multiply`, `identity` | Rectangular numeric matrix operations. |
| `mean`, `median`, `variance`, `stddev`, `percentile` | Descriptive statistics; variance uses the population convention. |
| `covariance`, `correlation` | Pairwise population statistics for equal-length sequences. |
| `is_empty(value)` | Test collections and strings. |
| `reverse(sequence)` | Reverse arrays, lists, tuples, or ranges (range results are arrays). |
| `type_of(value)` / `print(value)` | Inspect values or print them. |
| `receiver :: message(arguments)` | Dispatch a message; struct receivers use type-specific behavior. |

Numerical collection functions use existing Simply arrays, lists, and tuples;
there is no separate tensor type. Vector and matrix dimensions are validated,
matrix multiplication uses the standard O(m × n × k) algorithm, and invalid
domains or non-finite results produce runtime diagnostics. See the
[language reference](docs/language.md) and [type system](docs/types.md) for
return types, statistical conventions, errors, and complexity.

## Command Reference

```bash
simply run <file.si>       # Execute a source file
simply check <file.si>     # Lex, parse, and analyze without execution
simply fmt <file.si>       # Format source text
simply tokens <file.si>    # Print lexer tokens
simply ast <file.si>       # Print the parsed AST
simply bench <file.si>     # Measure front-end and runtime phases
simply explain <file.si>      # Explain program structure and runtime model
simply explain-flow <file.si> # Print a Flow execution plan
simply test                # Run direct tests/*.si files
simply repl                # Start the interactive evaluator
simply --help              # Show command usage
simply --version           # Show the installed version
```

Commands are intentionally explicit: `fmt` and `check` are commands, not
duplicated flag aliases. The formatter writes the result to standard output,
so it can be reviewed or redirected safely.

The REPL evaluates ordinary statements as they are entered and accepts
multiline blocks such as functions and conditionals through their closing
`end`; the continuation prompt is shown while a block is open. Prompts are
hidden when input is piped or redirected, keeping pasted scripts and batch
input clean. When a terminal paste has additional lines already buffered, the
REPL also skips prompts until that paste is consumed on Unix. Each value
printed in the REPL is followed by a separator line. Enter `:q`, `:quit`,
`:exit`, `quit`, or `exit` to leave the interactive REPL; Ctrl-D also works.

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
is cheap; a mutation detaches storage when another value still shares it.
Function bodies and imported programs use shared storage where appropriate.

## Repository Layout

```text
.
├── src/
├── docs/
├── examples/
│   ├── 01-basics/
│   ├── 02-variables/
│   ├── 03-operators/
│   ├── 04-control-flow/
│   ├── 05-functions/
│   ├── 06-collections/
│   ├── 07-pipelines/
│   ├── 08-standard-library/
│   ├── 09-quality/
│   ├── 10-flow/
│   ├── 11-compiler-foundations/
│   ├── 12-mathematics/
│   ├── 13-objects/
│   ├── 14-enums/
│   ├── 15-patterns/
│   ├── 16-user-input/
│   ├── 17-modules/
│   ├── 99-bench/
│   └── 99-smoke/
├── tests/
├── Cargo.toml
└── LICENSE
```

`src/` contains the Rust interpreter, `docs/` contains the language and
developer documentation, and `examples/` groups sample programs by topic.
`tests/` contains Rust integration tests and Simply fixtures.

## Documentation

- [Language reference](docs/language.md) — statements, expressions, operators,
  built-ins, and language behavior.
- [Type system](docs/types.md) — inference, annotations, compatibility, and
  numeric/matrix rules.
- [Grammar](docs/grammar.md) — implementation-oriented syntax summary.
- [Semantics](docs/semantics.md) — scopes, mutability, imports, and evaluation.
- [Memory model](docs/memory-model.md) — sharing, copy-on-write, closure
  capture, equality, and value lifetimes.
- [Errors and diagnostics](docs/errors.md) — error categories and stable codes.
- [Performance](docs/performance.md) — optimizations and benchmark workloads.
- [Project contract](docs/project.md) — project layout, imports, and test
  discovery.
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
