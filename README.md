# Simply

A small programming language written in Rust.

Simply is built around a straightforward idea: programming should be easy to read without hiding what is actually happening.

```si
name is "Simply"
age is 18

age -> age + 1

Say "Hello, " + name
Say age
```

The syntax is intentionally simple. Variable declarations use `is`, reassignment uses `->`, and blocks end explicitly with `end`.

Simply is currently an interpreted language. The project includes a lexer, parser, abstract syntax tree, semantic checker, evaluator, formatter, command-line interface, tests, and a growing collection of examples.

> **Note**
>
> Simply is still a work in progress. The language and its features may change as the project evolves.

---

## What Simply Can Do

The current implementation includes:

* Variable declarations with `is`
* Type inference and optional type annotations
* Type-safe reassignment with `->`
* Integers, floats, booleans, strings, and `Unit`
* Arrays, lists, tuples, hashes, trees, and matrices
* Conditional execution with `if` and `else`
* Iteration with `for` and `while`
* `break` and `continue`
* Functions with parameters and return values
* Static semantic checking with `check`
* Collection pipelines with `filter`, `map`, `sum`, and `count`
* Built-in functions such as `range`, `length`, `contains`, and `type_of`
* Loading values from another source file with `open`
* Explicit message dispatch with `send`
* Token inspection
* AST inspection
* Source formatting
* Source-aware diagnostics and error messages

There is still more to build, but the core language is already usable.

---

## Getting Started

### Requirements

To build Simply from source, you need:

* Rust
* Cargo

Run the smoke example directly from the project root:

```bash
cargo run -- examples/99-smoke/smoke.si
```

You can also install the CLI locally:

```bash
cargo install --path .
```

After installation, run a Simply program with:

```bash
simply examples/99-smoke/smoke.si
```

The CLI accepts `.si` source files.

### Prebuilt Releases

Prebuilt binaries can be published through the project's GitHub Releases page.

Once Simply is installed, programs can be executed directly:

```bash
simply program.si
```

---

## CLI

The command-line interface is intentionally small.

```text
simply <file.si>
simply check <file.si>
simply --check <file.si>
simply --tokens <file.si>
simply --ast <file.si>
simply --format <file.si>
simply --help
```

### Run a Program

```bash
simply examples/01-basics/values.si
```

### Check a Program

The `check` command runs the lexer, parser, and semantic analyzer without executing the program.

```bash
simply check examples/99-smoke/smoke.si
```

You can also use:

```bash
simply --check examples/99-smoke/smoke.si
```

### Inspect Tokens

```bash
simply --tokens examples/01-basics/values.si
```

### Inspect the AST

```bash
simply --ast examples/01-basics/values.si
```

These commands are useful when debugging programs or working on the language implementation.

### Format Source

```bash
simply --format examples/04-control-flow/while.si
```

Formatting currently prints the formatted result to standard output instead of modifying the source file.

### Help

```bash
simply --help
```

---

## The Language

### Variables

Variables are declared using `is`:

```si
name is "Ada"
age is 18
active is true
```

Simply infers the type from the assigned value.

Types can also be written explicitly:

```si
age as Int is 18

scores as List[Int] is list [10, 20, 30]
```

To update an existing variable, use `->`:

```si
age is 18

age -> age + 1
```

Variables are type-safe, so changing a variable to an incompatible type is rejected:

```si
age is 18

age -> "hello"
```

---

### Conditions

Blocks end explicitly with `end`.

```si
score is 75

if score >= 60:
    Say "passed"
else:
    Say "try again"
end
```

Conditions must evaluate to `Bool`.

---

### Loops

#### `for`

```si
for name in list ["Ada", "Lin"]:
    Say name
end
```

#### `while`

```si
count is 0

while count < 3:
    Say count
    count -> count + 1
end
```

Both `break` and `continue` are supported:

```si
for number in range(0, 10):
    if number == 5:
        continue
    end

    if number == 8:
        break
    end

    Say number
end
```

---

### Functions

Functions are declared with `fn`.

```si
fn add(left as Int, right as Int) gives Int:
    return left + right
end

total is add(10, 20)

Say total
```

Parameters and return types can be omitted when explicit annotations are unnecessary.

Functions have their own local scope and support `return`.

---

## Collections

Simply currently supports several collection types.

### Arrays and Lists

```si
cities is array ["Jakarta", "Citra", "Lima"]

queue is list ["first", "second"]

queue add "third"
queue remove "first"

Say cities[1]
Say queue
```

### Tuples

```si
point is (10, "Ada")

(x, y) is point

Say x
Say point[1]
```

### Hashes

```si
settings is hash:
    retries is 3
    mode is "quiet"
end

Say settings["mode"]
```

Trees and matrices are also supported.

More collection examples can be found in:

```text
examples/06-collections/
```

---

## Pipelines

Pipelines provide a simple way to process collections step by step.

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

Current pipeline stages include:

* `filter`
* `map`
* `sum`
* `count`

---

## Loading Another Source File

The `open` statement loads a value from another Simply source file.

```si
open "imported-values.si" as imported

Say imported[1]
```

The imported source file can return a value:

```si
return list [10, 20, 30]
```

Paths are resolved relative to the file performing the `open`.

This mechanism currently behaves more like loading a source value than a full module or import system.

---

## Message Dispatch

Simply includes a small and explicit message-dispatch mechanism.

A function can receive a value:

```si
fn greet(self):
    return "Hello " + self["name"]
end
```

A value can contain the associated data:

```si
person is hash:
    name is "Ada"
end
```

A message can then be dispatched with `send`:

```si
Say send(person, "greet")
```

There are no classes, constructors, inheritance, or hidden method machinery. The behavior is intentionally explicit.

---

## Built-ins

Some of the currently available built-in functions are:

| Function                      | Description                                |
| ----------------------------- | ------------------------------------------ |
| `range(start, end)`           | Creates a range of integers                |
| `length(value)`               | Returns the size of a collection or string |
| `count(value)`                | Counts a collection or value               |
| `contains(collection, value)` | Checks whether a value exists              |
| `type_of(value)`              | Returns the runtime type name              |
| `print(value)`                | Prints a value                             |

Working examples can be found in:

```text
examples/08-standard-library/
```

---

## Errors and Static Checking

One of the goals of Simply is to provide useful errors instead of failing somewhere deep inside the interpreter.

For example:

```bash
simply check program.si
```

The semantic checker can detect issues such as:

* Unknown variables or functions
* Type mismatches
* Invalid reassignment
* Incorrect function arguments
* Invalid return types
* Invalid use of `break` or `continue`
* Invalid conditions
* Collection and indexing problems
* Pipeline mistakes

When an error occurs, Simply attempts to point to the relevant location:

```text
Error: Semantic error at program.si:2:1: error[E0003]: type mismatch

  2 | age -> "hello"
    | ^
```

Some problems can only be detected at runtime. Those are handled by the evaluator.

---

## Under the Hood

The interpreter is divided into a small set of straightforward stages:

```text
Source
  ↓
Lexer
  ↓
Parser
  ↓
AST
  ↓
Semantic Analyzer
  ↓
Evaluator
  ↓
Value
```

The main components live in `src/`:

| File           | Purpose                                 |
| -------------- | --------------------------------------- |
| `lexer.rs`     | Converts source code into tokens        |
| `parser.rs`    | Converts tokens into an AST             |
| `ast.rs`       | Defines the language structure          |
| `semantic.rs`  | Checks names, scopes, and types         |
| `evaluator.rs` | Executes programs                       |
| `error.rs`     | Handles diagnostics and error reporting |
| `formatter.rs` | Formats source code                     |
| `main.rs`      | Provides the CLI entry point            |

The implementation is written in Rust.

---

## Examples

The repository includes examples organized by language feature:

```text
examples/
├── 01-basics/
├── 02-variables/
├── 03-operators/
├── 04-control-flow/
├── 05-functions/
├── 06-collections/
├── 07-pipelines/
├── 08-standard-library/
├── 09-quality/
└── 99-smoke/
```

The `99-smoke` example is a good starting point if you want to quickly see the language in action.

Run an example with:

```bash
cargo run -- examples/01-basics/values.si
```

---

## Development

If you're working on Simply itself, run the test suite:

```bash
cargo test
```

Format the code:

```bash
cargo fmt
```

Check formatting without modifying files:

```bash
cargo fmt -- --check
```

Before pushing changes, it is recommended to run:

```bash
cargo fmt -- --check
cargo test
```

When changing the language, try to keep these parts aligned:

1. The implementation
2. The tests
3. The examples
4. The documentation

This makes it easier to verify that a feature works correctly from implementation through user-facing behavior.

---

## Project Status

Simply is still under active development.

It is currently an interpreter with static semantic checking and a growing standard library.

The project does not currently include:

* A bytecode VM
* A native compiler backend
* A package manager
* Classes
* Inheritance

These features may come later, but the current priority is keeping the core language small, understandable, explicit, and consistent.

---

## Repository Structure

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
├── Cargo.toml
├── LICENSE
└── README.md
```

---

## Contributing

Simply is still evolving, and feedback and improvements are welcome.

When changing language behavior, please update the relevant:

* Implementation
* Tests
* Examples
* Documentation

Small contributions are welcome too. Finding a confusing error message, awkward syntax, or inconsistent behavior can be just as useful as adding a new feature.

---

## License

Simply is licensed under the MIT License.

Copyright (c) 2026 Simply Contributors.

See the [LICENSE](LICENSE) file for more information.
