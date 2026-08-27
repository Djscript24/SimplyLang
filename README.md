# Simply

**Simply** is a small programming language written in Rust.

The idea behind it is pretty straightforward: programming should be easy to read without hiding what's actually happening.

```si
name is "Simply"
age is 18

age -> age + 1

Say "Hello, " + name
Say age

The syntax is intentionally simple. There are no semicolons everywhere, declarations use is, reassignment uses ->, and blocks end with end.

Simply is currently an interpreted language. It already has a lexer, parser, AST, semantic checker, evaluator, formatter, CLI, tests, and a collection of examples.

> Simply is still a work in progress. Things may change as the language evolves.




---

What Simply can do

A few things are already working:

Simple variable declarations with is

Type inference and optional type annotations

Type-safe reassignment with ->

Integers, floats, booleans, strings, and Unit

Arrays, lists, tuples, hashes, trees, and matrices

if / else

for and while

break and continue

Functions

Static checking with check

Collection pipelines with filter, map, sum, and count

Built-in functions such as range, length, contains, and type_of

Loading values from another source file with open

Explicit message dispatch with send

Token, AST, and formatting inspection

Source-aware error messages


There's still a lot to build, but the core language is already usable.


---

Try it

From source

You'll need:

Rust

Cargo


Clone the repository and run an example:

cargo run -- examples/99-smoke/smoke.si

You can also install the CLI locally:

cargo install --path .

After that:

simply examples/99-smoke/smoke.si

That's it.

Prebuilt releases

Prebuilt binaries are available on the GitHub Releases page when a release is published.

After installing Simply, you should be able to run:

simply program.si

The CLI accepts .si source files.


---

CLI

The CLI is intentionally small.

simply <file.si>
simply check <file.si>
simply --tokens <file.si>
simply --ast <file.si>
simply --format <file.si>
simply --help

Run a program

simply examples/01-basics/values.si

Check a program

check runs the lexer, parser, and semantic analyzer without actually executing the program.

simply check examples/99-smoke/smoke.si

You can also use:

simply --check examples/99-smoke/smoke.si

Inspect tokens or AST

Useful when working on the language itself:

simply --tokens examples/01-basics/values.si
simply --ast examples/01-basics/values.si

Format source

simply --format examples/04-control-flow/while.si

Formatting currently prints the result instead of modifying the file.


---

The Language

Variables

Variables use is:

name is "Ada"
age is 18
active is true

Simply figures out the type from the value.

You can also write the type explicitly:

age as Int is 18
scores as List[Int] is list [10, 20, 30]

To change a variable, use ->:

age is 18
age -> age + 1

A variable keeps its type, so this isn't allowed:

age is 18
age -> "hello"


---

Conditions

Blocks end with end.

score is 75

if score >= 60:
    Say "passed"
else:
    Say "try again"
end

Conditions must evaluate to Bool.


---

Loops

for

for name in list ["Ada", "Lin"]:
    Say name
end

while

count is 0

while count < 3:
    Say count
    count -> count + 1
end

Both break and continue are supported.


---

Functions

Functions use fn:

fn add(left as Int, right as Int) gives Int:
    return left + right
end

total is add(10, 20)

Say total

Parameters and return types can be left untyped when you don't need explicit annotations.

Functions have their own local scope and support return.


---

Collections

Simply currently has several collection types.

Lists and arrays

cities is array ["Jakarta", "Citra", "Lima"]
queue is list ["first", "second"]

queue add "third"
queue remove "first"

Say cities[1]
Say queue

Tuples

point is (10, "Ada")

(x, y) is point

Say x
Say point[1]

Hashes

settings is hash:
    retries is 3
    mode is "quiet"
end

Say settings["mode"]

Trees and matrices are also supported.

For examples, see examples/06-collections.


---

Pipelines

Pipelines are a way to process collections step by step.

numbers is list [10, 20, 30]

total is pipeline:
    numbers
    filter item >= 20
    map item * 2
    sum
end

Say total

Current pipeline stages are:

filter

map

sum

count



---

Loading another source file

open can load a value from another Simply source file:

open "imported-values.si" as imported

Say imported[1]

The other file can return a value:

return list [10, 20, 30]

Paths are resolved relative to the file doing the open.

This is currently more like loading a source value than a full module/import system.


---

Message Dispatch

Simply also has a small message-dispatch mechanism.

A function can receive a value:

fn greet(self):
    return "Hello " + self["name"]
end

And a value can contain the data:

person is hash:
    name is "Ada"
end

Then:

Say send(person, "greet")

There are no classes, constructors, inheritance, or hidden method machinery here. The behavior is deliberately explicit.


---

Built-ins

Some of the currently available built-ins:

Function	What it does

range(start, end)	Creates a range of integers
length(value)	Gets the size of a collection or string
count(value)	Counts a value/collection
contains(collection, value)	Checks whether something exists
type_of(value)	Returns the runtime type name
print(value)	Prints a value


You can find working examples in examples/08-standard-library.


---

Errors and check

One of the things I'm trying to keep important in Simply is getting useful errors instead of just crashing somewhere deep inside the interpreter.

For example:

simply check program.si

can catch things such as:

Unknown variables or functions

Type mismatches

Invalid reassignment

Wrong function arguments

Invalid return types

Invalid break / continue

Invalid conditions

Collection and indexing problems

Pipeline mistakes


When something goes wrong, Simply tries to point to the actual location:

Error: Semantic error at program.si:2:1: error[E0003]: type mismatch

  2 | age -> "hello"
    | ^

Some problems can only be known at runtime, so those are still handled by the evaluator.


---

Under the Hood

The interpreter is intentionally split into a few straightforward stages:

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

The main pieces live in src/:

File	Purpose

lexer.rs	Turns source code into tokens
parser.rs	Turns tokens into an AST
ast.rs	Defines the language structure
semantic.rs	Checks names, scopes, and types
evaluator.rs	Executes programs
error.rs	Error reporting
formatter.rs	Source formatting
main.rs	CLI


The implementation is written in Rust.


---

Examples

If you want to see what Simply looks like without reading the whole README, start here:

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

The 99-smoke example is a good starting point if you just want to see the language running.

Run one with:

cargo run -- examples/01-basics/values.si


---

Development

If you're working on Simply itself:

cargo test

Format the code:

cargo fmt

Check formatting without changing anything:

cargo fmt -- --check

Before pushing changes, I usually recommend:

cargo fmt -- --check
cargo test

When changing the language, try to keep these three things together:

1. The implementation


2. The tests


3. The examples



That makes it much easier to tell whether a language feature actually works end-to-end.


---

Project Status

Simply is not finished yet.

It's currently an interpreter with static semantic checking and a growing standard library. There is no bytecode VM, native compiler backend, package manager, or class/inheritance system at the moment.

Those may come later, but the current priority is keeping the core language small, understandable, and consistent.


---

Repository Structure

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
└── README.md


---

Contributing

Simply is still evolving, so feedback and improvements are welcome.

If you change language behavior, please try to update the relevant:

implementation

tests

examples

documentation


Small changes are fine too. Even finding a confusing error message or an awkward piece of syntax is useful feedback.


---

License

Simply is licensed under the MIT License.

Copyright (c) 2026 Simply Contributors.