# Simply

Simply is a small, readable, newline-oriented programming language for
expressing programs with clear, explicit syntax. Values are created with
`is`, updated with `->`, and printed with `Say`.

```si
name is "Simply"
version as Float is 0.1
version -> version + 0.1

Say "Hello, " + name
Say version
```

The interpreter focuses on explicit syntax and predictable behavior. It
includes typed values, functions, collections, pipelines, source-value loading,
and message dispatch without classes or hidden object machinery.

## Project status

Simply is an early-stage interpreter implemented in Rust. The language and
runtime are actively evolving, while the examples and test suite document the
currently supported behavior.

### Highlights

- Readable, newline-oriented syntax
- Inferred and explicitly annotated types
- Functions with local scopes and typed parameters
- Arrays, lists, tuples, hashes, trees, and matrices
- Collection pipelines with `filter`, `map`, `sum`, and `count`
- Structured lexical, parse, semantic, and runtime diagnostics
- Relative source-value loading with `open`
- Deterministic source formatting

## Quick start

Install a stable Rust toolchain with Cargo, then run:

```bash
cargo run -- examples/99-smoke/smoke.si
```

Useful commands:

```bash
cargo run -- check examples/99-smoke/smoke.si
cargo run -- --tokens examples/01-basics/values.si
cargo run -- --ast examples/05-functions/functions.si
cargo run -- --format examples/04-control-flow/while.si
cargo run -- --help
```

To build the release binary:

```bash
cargo build --release
./target/release/simply --help
```

`check` and `--check` lex, parse, and semantically analyze a program without executing it. Normal execution also performs runtime validation for values and operations that cannot be known statically.

## Syntax

Statements are newline-oriented and blocks end with `end`.

```si
score as Int is 87
score -> score + 1

if score >= 60:
    Say "passed"
else:
    Say "try again"
end
```

Comments start with `#`. Strings support `\n`, `\t`, `\r`, `\"`, and `\\`.

## Values and types

The built-in types are:

- `Int`, `Float`, `Bool`, and `String`
- `Array[T]`, `List[T]`, and `Tuple[T1, T2, ...]`
- `Hash`, `Tree`, `Matrix`, and `Unit`

Declarations infer a type unless an annotation is supplied:

```si
count is 3
names as List[String] is list ["Ada", "Lin"]
point is (10, "Ada")
```

Bindings keep a compatible type when reassigned with `->`. Branches, `for` loops, pipelines, and function calls have their own local scopes. A `while` loop does not create a new scope, so a binding created inside it remains available afterward.

## Operators

Simply supports `+`, `-`, `*`, `/`, `%`, `>`, `>=`, `<`, `<=`, `==`, `!=`, `and`, `or`, and unary `not` and `-`. Numbers support integer, float, and mixed arithmetic. Integer arithmetic detects overflow. Division and remainder by zero produce diagnostics. `and` and `or` short-circuit.

`multiply` is the matrix multiplication operator. `transpose` is the matrix transpose operator.

## Control flow

Conditions must evaluate to `Bool`.

```si
for name in list ["Ada", "Lin"]:
    Say name
end

step is 0
while step < 3:
    Say step
    step -> step + 1
end
```

`break` and `continue` are valid inside loops. `if` and `for` introduce local scopes; `while` intentionally keeps the surrounding scope.

## Functions

Functions can have typed parameters and a typed return value:

```si
fn add(left as Int, right as Int) gives Int:
    return left + right
end

total is add(10, 20)
Say total
```

Functions without an explicit `return` produce `Unit`. Parameters and local declarations are isolated from the caller, while functions may read compatible global bindings.

## Collections

Arrays are fixed-length values and lists are mutable. Both support indexed replacement when the value has a compatible type.

```si
cities is array ["Jakarta", "Citra", "Lima"]
queue is list ["first", "second"]

cities[1] -> "Bandung"
queue add "third"
queue remove "first"
Say cities
Say queue
```

Tuples support indexed access and destructuring:

```si
coordinates is (12, 30)
(x, y) is coordinates
Say x
Say coordinates[1]
```

Hashes and trees use named fields. They support string indexing and field access with a dot.

```si
profile is hash:
    name is "Ada"
    role is "builder"
end

catalog is tree:
    featured is profile
end

Say profile.name
Say catalog["featured"].role
```

Matrices contain array rows. Matrix operations validate numeric values, rectangular rows, and compatible dimensions at runtime.

```si
left is matrix [[1, 2], [3, 4]]
right is matrix [[5, 6], [7, 8]]

Say left[0, 1]
Say left + right
Say left multiply right
Say left transpose
```

## Pipelines

Pipelines accept arrays or lists. `filter` and `map` use the current element as `item`; `sum` and `count` are terminal stages.

```si
numbers is list [10, 20, 30, 40]

total is pipeline:
    numbers
    filter item >= 20
    map item * 2
    sum
end

Say total
```

## Built-ins

- `range(start, end)` returns an integer array from `start`, inclusive, to `end`, exclusive.
- `length(value)` returns the size of a supported collection or string.
- `count(value)` is the collection-size alias and a pipeline terminal stage.
- `contains(collection, value)` checks collection membership or a string substring.
- `type_of(value)` returns the runtime type name as a string.
- `print(value)` prints a value and returns `Unit`.

## Source values and messages

`open` evaluates another `.si` file in isolation and binds its returned value to an alias. Relative paths are resolved from the importing file, and cyclic imports are rejected.

```si
open "imported-values.si" as imported
Say imported[1]
```

The imported file must return a value:

```si
return list [10, 20, 30]
```

`send(receiver, "message", ...)` dispatches to a Simply function. The receiver is passed as the first function argument.

```si
fn greet(self):
    return "Hello " + self["name"]
end

person is hash:
    name is "Ada"
end

Say send(person, "greet")
```

## Diagnostics

Errors are structured and include a category, stable code, path, location, source context, and a caret. The `DiagnosticCode` enum is the single source of truth for diagnostic text codes and categories.

Static checking catches names, types, operators, function calls, control flow, collection shapes, and pipeline stages. Runtime diagnostics handle dynamic conditions such as missing files, missing keys, invalid bounds, division by zero, malformed matrix values, and message lookup failures.

## Repository layout

| Path | Purpose |
| --- | --- |
| `src/lexer.rs` | Tokenization and lexical diagnostics |
| `src/parser.rs` | Parsing source into the AST |
| `src/semantic.rs` | Static name, scope, and type analysis |
| `src/evaluator.rs` | Runtime evaluation |
| `src/runtime/` | Runtime values, scopes, collections, and operations |
| `src/error.rs` | Structured diagnostic definitions and rendering |
| `src/formatter.rs` | Deterministic source formatting |
| `src/cli.rs` | Command-line interface |
| `examples/` | Runnable programs and import fixtures |
| `tests/` | Integration and conformance tests |

## Examples

| Directory | Coverage |
| --- | --- |
| `01-basics` | Primitive values, annotations, comments, and output |
| `02-variables` | Inference, reassignment, indexed replacement, and types |
| `03-operators` | Arithmetic, comparisons, precedence, and short-circuiting |
| `04-control-flow` | Conditions, `for`, `while`, `break`, and `continue` |
| `05-functions` | Parameters, return values, and local scope |
| `06-collections` | Arrays, lists, hashes, trees, matrices, and tuples |
| `07-pipelines` | Filtering, mapping, summing, and counting |
| `08-standard-library` | Built-ins, inspection, and import fixtures |
| `09-quality` | Scope, messages, and structured runtime errors |
| `99-smoke` | Integrated end-to-end program |

`examples/08-standard-library/imported-values.si` is an import fixture, not a standalone program. It intentionally contains a top-level `return` for its importing example.

## Development

Run the complete validation suite:

```bash
cargo fmt -- --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

The current implementation is an interpreter. It does not include a bytecode VM, compiler backend, package manager, classes, inheritance, generics, or an ownership system.

## Contributing

Keep changes focused and preserve the language behavior documented by the
examples. Before opening a change, run the complete validation suite and add a
regression test for behavior that has changed or been corrected.

## License

Simply is distributed under the [MIT License](LICENSE). You may use, copy,
modify, merge, publish, distribute, sublicense, and sell the software, subject
to the conditions in the license notice. The software is provided without
warranty; see [LICENSE](LICENSE) for the complete terms.
