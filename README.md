# Simply

**Simply** is a small interpreted programming language focused on readable syntax, explicit control flow, predictable semantics, and a compact Rust implementation.

A Simply program is intentionally straightforward:

```si
name is "Simply"
age is 18

age -> age + 1

Say "Hello, " + name
Say age
```

The project currently includes a lexer, parser, AST, semantic analyzer, evaluator, formatter, CLI tooling, integration tests, and a growing set of runnable language examples.

## Highlights

- Readable declaration syntax with `is`
- Type inference and optional explicit type annotations
- Type-stable reassignment with `->`
- Integers, floats, booleans, strings, and `Unit`
- Arrays, lists, tuples, hashes, trees, and matrices
- `if`, `else`, `for`, `while`, `break`, and `continue`
- Functions with optional typed parameters and return types
- Static semantic checking through `check` / `--check`
- Pipelines with `filter`, `map`, `sum`, and `count`
- Built-in functions such as `range`, `length`, `contains`, and `type_of`
- Source-value loading with `open`
- Explicit message dispatch with `send`
- Token, AST, and formatting inspection modes
- Source-aware diagnostics with line and column context

---

## Quick Start

### Requirements

- Rust toolchain
- Cargo

From the project root:

```bash
cargo run -- examples/99-smoke/smoke.si
```

Or install the CLI locally:

```bash
cargo install --path .
```

Then run:

```bash
simply examples/99-smoke/smoke.si
```

> The CLI currently executes a source path directly. There is no `run` subcommand.

---

## CLI

```text
simply <file.si>
simply --tokens <file.si>
simply --ast <file.si>
simply --format <file.si>
simply check <file.si>
simply --check <file.si>
simply --help
```

### Run a program

```bash
cargo run -- examples/01-basics/values.si
```

### Check without executing

```bash
cargo run -- check examples/99-smoke/smoke.si
```

Equivalent option form:

```bash
cargo run -- --check examples/99-smoke/smoke.si
```

Successful checks print:

```text
Checking examples/99-smoke/smoke.si...
No errors found.
```

### Inspect tokens

```bash
cargo run -- --tokens examples/01-basics/values.si
```

### Inspect the parsed AST

```bash
cargo run -- --ast examples/01-basics/values.si
```

### Format source

```bash
cargo run -- --format examples/04-control-flow/while.si
```

Formatting prints the formatted result to standard output; it does not rewrite the source file.

### Help

```bash
cargo run -- --help
```

Only `.si` source files are accepted.

---

# Language Guide

## Variables and Types

Variables are declared with `is`:

```si
name is "Ada"
age is 18
active is true
```

Simply infers the type of the initial value. Explicit annotations are also supported:

```si
age as Int is 18
scores as List[Int] is list [10, 20, 30]
```

Bindings are type-stable. Reassignment uses `->` and must remain compatible with the declared or inferred type:

```si
age is 18
age -> age + 1
```

Changing an `Int` binding into a `String`, for example, is rejected.

### Value categories

The language currently supports:

- `Int`
- `Float`
- `Bool`
- `String`
- `Array[T]`
- `List[T]`
- `Tuple[T1, T2, ...]`
- `Hash`
- `Tree`
- `Matrix`
- `Unit`

`Unit` represents the absence of a returned value.

---

## Operators

Supported operators include:

```text
+  -  *  /  %
>  >=  <  <=
==  !=
and  or  not
```

Examples:

```si
Say 10 + 20
Say 10 * 2
Say 10 > 5
Say true and false
Say not false
```

Numeric operations support integers, floats, and mixed integer/float arithmetic. String concatenation uses `+`:

```si
name is "Ada"
Say "Hello, " + name
```

Boolean operators require boolean values and use short-circuit evaluation.

---

## Conditions

Conditions must evaluate to `Bool`. Blocks are terminated explicitly with `end`.

```si
score is 75

if score >= 60:
    Say "passed"
else:
    Say "try again"
end
```

See `examples/04-control-flow/conditionals.si`.

---

## Loops

### `for`

```si
for name in list ["Ada", "Lin"]:
    Say name
end
```

### `while`

```si
count is 0

while count < 3:
    Say count
    count -> count + 1
end
```

### `break` and `continue`

```si
for item in range(0, 5):
    if item == 0:
        continue
    end

    if item == 4:
        break
    end

    Say item
end
```

`break` and `continue` are checked semantically and rejected outside valid loop contexts.

---

## Functions

Functions use `fn`. Parameters can be annotated with `as`, and return types can be declared with `gives`.

```si
fn add(left as Int, right as Int) gives Int:
    return left + right
end

total is add(10, 20)
Say total
```

Functions support:

- Parameters and argument validation
- Optional parameter types
- Optional declared return types
- `return`
- Local function scope
- Reading compatible outer/global bindings

Local declarations do not leak out of a function.

---

## Collections

### Arrays and lists

```si
cities is array ["Jakarta", "Citra", "Lima"]
queue is list ["first", "second"]

queue add "third"
queue remove "first"

Say cities[1]
Say queue
```

### Tuples and destructuring

```si
point is (10, "Ada")

(x, y) is point

Say x
Say point[1]
```

### Hashes and trees

Named collections use explicit blocks:

```si
settings is hash:
    retries is 3
    mode is "quiet"
end

Say settings["mode"]
```

Trees also support consistent named-value indexing.

### Matrices

Matrices contain rectangular numeric rows:

```si
matrix is matrix [[1, 2], [3, 4]]
```

Ragged matrices are rejected.

For more examples, see:

- `examples/06-collections/arrays-lists.si`
- `examples/06-collections/tuples.si`
- `examples/06-collections/hash-tree.si`
- `examples/06-collections/matrices.si`

---

## Pipelines

Pipelines transform arrays or lists through staged operations.

Supported stages include:

- `filter`
- `map`
- `sum`
- `count`

Inside `filter` and `map`, the current element is available as `item`.

```si
numbers is list [10, 20, 30]

total is pipeline:
    numbers
    filter item >= 20
    map item * 2
    sum
end

Say total
```

`sum` and `count` are terminal stages. The semantic analyzer propagates collection element information where it can determine types statically.

See `examples/07-pipelines/collections.si`.

---

## Source Values with `open`

`open` evaluates another Simply source file in isolation and binds its returned value to an alias.

```si
open "imported-values.si" as imported

Say imported[1]
```

The opened file can return a value:

```si
return list [10, 20, 30]
```

Relative paths are resolved from the importing source file.

This is a source-value mechanism rather than a conventional namespace or module import system.

See:

- `examples/08-standard-library/imported-values.si`
- `examples/08-standard-library/builtins.si`

---

## Message Objects

Simply supports explicit message dispatch without classes or inheritance.

A function defines behavior:

```si
fn greet(self):
    return "Hello " + self["name"]
end
```

A value stores data:

```si
person is hash:
    name is "Ada"
end
```

A message is dispatched with `send`:

```si
Say send(person, "greet")
```

The receiver is passed to the target function as its first argument.

This model intentionally avoids hidden object-oriented machinery such as classes, constructors, method syntax, or inheritance.

See `examples/09-quality/message-objects.si`.

---

# Standard Library

The current built-ins include:

| Function | Description |
| --- | --- |
| `range(start, end)` | Returns an integer array from `start` up to, excluding, `end` |
| `length(value)` | Returns the size of a supported collection or string |
| `count(value)` | Alias for `length(value)` when used as a function |
| `contains(collection, value)` | Checks whether a collection contains a value or a string contains a substring |
| `type_of(value)` | Returns the runtime type name as a string |
| `print(value)` | Prints a value and returns `Unit` |

Examples are available in:

- `examples/08-standard-library/builtins.si`
- `examples/08-standard-library/inspection.si`

---

# Static Checking and Errors

The `check` command performs lexing, parsing, and semantic analysis without executing the program:

```bash
simply check program.si
```

The semantic analyzer validates, where statically determinable:

- Variable and function names
- Scope rules
- Inferred and explicit types
- Reassignment compatibility
- Operator operands
- Function argument counts and types
- Function return types
- Boolean conditions
- Valid `return`, `break`, and `continue` usage
- Tuple and matrix indexing shapes
- Collection constraints
- Pipeline stages

When a value cannot be determined without execution, the analyzer keeps the result conservative and runtime checks remain responsible for dynamic failures.

Diagnostics include an error category, semantic code where applicable, file, line, column, source context, and a caret indicator:

```text
Error: Semantic error at program.si:2:1: error[E0003]: type mismatch

  2 | age -> "hello"
    | ^
```

Runtime diagnostics cover dynamic problems such as:

- Missing values or keys
- Out-of-bounds collection access
- Division by zero
- Integer arithmetic errors
- File loading failures
- Invalid message lookup

---

# Project Architecture

The interpreter follows a deliberately simple pipeline:

```text
Source
  -> Lexer
  -> Parser
  -> AST
  -> SemanticAnalyzer
  -> Evaluator
  -> Value
```

| Module | Responsibility |
| --- | --- |
| `src/lexer.rs` | Tokenization and lexical errors |
| `src/parser.rs` | Syntax parsing and AST construction |
| `src/ast.rs` | Statements, expressions, operators, and language types |
| `src/semantic.rs` | Static name, scope, and type checking |
| `src/evaluator.rs` | Runtime execution and value evaluation |
| `src/error.rs` | Structured diagnostics and source rendering |
| `src/formatter.rs` | Deterministic source formatting |
| `src/main.rs` | CLI entry point and source workflow |

---

# Examples

The repository contains runnable examples organized by topic:

| Directory | Focus |
| --- | --- |
| `examples/01-basics` | Primitive values and output |
| `examples/02-variables` | Assignment, inference, reassignment, and annotations |
| `examples/03-operators` | Arithmetic and boolean logic |
| `examples/04-control-flow` | Conditions, loops, `break`, and `continue` |
| `examples/05-functions` | Function declarations and calls |
| `examples/06-collections` | Arrays, lists, tuples, hashes, trees, and matrices |
| `examples/07-pipelines` | Collection transformation pipelines |
| `examples/08-standard-library` | Built-ins and source-value loading |
| `examples/09-quality` | Scope, short-circuiting, and message objects |
| `examples/99-smoke` | Compact end-to-end smoke example |

Run any example directly:

```bash
cargo run -- examples/06-collections/arrays-lists.si
```

---

# Testing and Development

Run the integration suite:

```bash
cargo test
```

Check Rust formatting:

```bash
cargo fmt -- --check
```

Recommended validation before submitting changes:

```bash
cargo fmt -- --check
cargo test
```

The integration tests exercise:

- Core values and output
- Typed functions
- Collections and pipelines
- Built-ins
- Scope and short-circuit behavior
- Message dispatch
- Loops, `break`, and `continue`
- Runtime error reporting
- Semantic checking
- Tuple and matrix validation
- Relative source-value loading

---

# Current Scope

Simply is currently an interpreter. The project does **not** include:

- A bytecode VM
- A compiler backend
- A package manager
- Classes or inheritance
- Generics
- An ownership system

These capabilities are outside the current implementation scope.

---

## Repository Layout

```text
.
├── src/
│   ├── ast.rs
│   ├── error.rs
│   ├── evaluator.rs
│   ├── formatter.rs
│   ├── lexer.rs
│   ├── main.rs
│   ├── parser.rs
│   └── semantic.rs
├── examples/
│   ├── 01-basics/
│   ├── 02-variables/
│   ├── ...
│   └── 99-smoke/
├── tests/
│   └── integration_examples.rs
└── README.md
```

## Contributing

When changing language behavior, keep the implementation and user-facing examples aligned:

1. Update the relevant parser, semantic, or evaluator logic.
2. Add or adjust an integration test.
3. Update affected examples.
4. Run formatting and tests.
5. Update this README when syntax or CLI behavior changes.

---

## License

This project is licensed under the [MIT License](LICENSE).

Copyright (c) 2026 Simply Contributors.