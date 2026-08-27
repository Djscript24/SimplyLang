# Simply

Simply is a small, readable programming language built around direct syntax:
`is` creates values, `->` changes them, and `Say` prints them.

```si
name is "Simply"
age is 10
age -> age + 1

Say "Hello, " + name
Say age
```

## Quick Start

Requirements: Rust and Cargo.

Run a source file:

```bash
cargo run -- examples/99-smoke/smoke.si
```

Inspect tokens, the parsed AST, or normalized formatting:

```bash
cargo run -- --tokens examples/01-basics/values.si
cargo run -- --ast examples/01-basics/values.si
cargo run -- --format examples/04-control-flow/while.si
```

Check a source file without executing it:

```bash
cargo run -- check examples/99-smoke/smoke.si
```

Run the full test suite:

```bash
cargo test
```

## Language Basics

### Values and Expressions

Simply supports strings, integers, floats, booleans, arithmetic, comparison,
equality, and short-circuit boolean operators.

An unannotated binding infers its type from its initial value. Reassignment
must keep that type; use `as` when an explicit type is clearer.

```si
active is true
score is 20 + 5

Say score * 2
Say score >= 25
Say not active
Say false and missing_value
```

Operators: `+`, `-`, `*`, `/`, `%`, `>`, `>=`, `<`, `<=`, `==`, `!=`, `and`,
`or`, and `not`.

### Conditions and Loops

Blocks end explicitly with `end`.

```si
if score >= 25 and active:
	Say "Ready"
else:
	Say "Keep going"
end
```

Simply also provides `for`, `while`, `break`, and `continue`.

### Functions

Functions can have typed parameters and return values.

```si
fn add(left as Int, right as Int) gives Int:
	return left + right
end

total is add(10, 20)
Say total
```

### Collections

Available collections are arrays, lists, tuples, hashes, trees, and matrices.
Collection elements can be typed with `Array[String]`, `List[Int]`, or similar
annotations.

```si
names is list ["Ada", "Lin"]
point is (3, 4)
Say names[0]
Say point[1]
```

Named values use a block:

```si
settings is hash:
	retries is 3
	mode is "quiet"
end

Say settings["mode"]
```

### Pipelines

Pipelines transform collections through `filter`, `map`, `sum`, and `count`.
Each step reads the current value as `item`.

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

## Source Values

`open` evaluates another Simply file in isolation and binds its returned value
to an alias. The path is relative to the importing file, and the value may be
any Simply value.

`builtins.si` demonstrates this with the sibling file
`imported-values.si`:

```si
open "imported-values.si" as imported
Say imported[1]
```

The source file must return a value:

```si
return list [10, 20, 30]
```

Run the importing file, not the returned-value source by itself:

```bash
cargo run -- examples/08-standard-library/builtins.si
```

## Message Objects

Simply models objects as values that receive messages. A hash stores the data,
a function defines the behavior, and `send` makes the interaction explicit.
The receiver becomes the first function argument.

```si
fn greet(self):
	return "Hello " + self["name"]
end

person is hash:
	name is "Ada"
end

Say send(person, "greet")
```

This model does not require classes, constructors, dot-method syntax, or hidden
inheritance. See `examples/09-quality/message-objects.si` for a runnable example.

## Standard Library

Built-in functions include:

- `range(start, end)`
- `length(value)` and `count(value)`
- `contains(collection, value)`
- `type_of(value)`
- `print(value)`

## Errors

Lexing, parsing, and runtime errors report their category, source filename,
line, column, source line, and a caret pointing to the available source span.
Type errors include both the expected and actual value types.

## Examples

Examples are grouped by topic:

| Directory | Focus |
| --- | --- |
| `01-basics` | Primitive values and output |
| `02-variables` | Assignment, reassignment, and types |
| `03-operators` | Arithmetic and boolean logic |
| `04-control-flow` | Conditions and loops |
| `05-functions` | Function declarations and calls |
| `06-collections` | Arrays, lists, tuples, hashes, trees, and matrices |
| `07-pipelines` | Collection transformations |
| `08-standard-library` | Built-ins and source-value imports |
| `09-quality` | Scope, short-circuiting, and message objects |
| `99-smoke` | Compact end-to-end example |
