# SimplyLang Semantics

The front end lexes source into tokens, parses tokens into an AST, and performs semantic analysis for `check`. Normal file execution evaluates the parsed program with runtime validation; use `check` when static diagnostics are required before execution.

The evaluator maintains indexed stacks of value scopes and runtime types. Bindings are immutable unless declared with `mut`; reassignment, indexed writes, and list add/remove require a mutable binding. Definitions are local to the current scope; reassignment updates the nearest existing mutable binding. The reassignment value may start on the same line as `->` or on the following line, which is useful for longer expressions:

```simply
mut name is 10
name ->
    name + 10
```

Writing `name + 10` alone only evaluates a calculation and does not change `name`. Function parameters and loop variables are immutable by default and may be prefixed with `mut`. `if` and `for` scopes are discarded after their bodies; `while` uses the surrounding scope. Collection values use copy-on-write storage, so aliases detach when one mutable binding mutates a collection.

`and` and `or` evaluate the right operand only when needed. Pipeline sources may be arrays, lists, lazy ranges, or CSV streams. `where` requires a boolean result and selects one subset; it preserves source order and does not mutate the source. `derive` transforms each item; transformed values become the input to later operations, and source order is preserved without mutating the source. `take N` passes at most N items at its position in the pipeline, with zero producing an empty result; it stops consuming the source as soon as the limit is reached. Thus, `where` before `take` counts matching items, while `where` after `take` filters the limited prefix. `skip N` discards the first N items that reach its position; `where` before `skip` therefore counts matches, while `where` after it filters the remaining suffix. `take_while condition` keeps the matching prefix at its position and stops consuming the source at the first false condition; unlike `where`, it never examines later items after a failure. `drop_while condition` discards items while the condition is true, then passes the first false item and all later items without testing them again. Both conditions must return `Bool`. `distinct` removes duplicates at its position using the language's equality semantics and preserves the first occurrence order; it compares transformed values after preceding `derive` steps and before subsequent operations. `take`, `skip`, `take_while`, `drop_while`, and `distinct` cannot be combined with `parallel` or `checkpoint`. `partition` is available in both Pipeline expressions and Flow declarations. It requires at least two distinct category names, classifies each item into at most one category, evaluates rules top-to-bottom, and uses the first matching rule. Unmatched items without `otherwise` are omitted. Repeated category names merge into one bucket. It preserves input order within each category and is terminal. `any` and `all` are Boolean terminals: `any` returns true at the first true item, while `all` returns false at the first false item; for empty input they return false and true respectively. Numeric aggregation and `write_csv(path)` are also terminal operations. `sum` returns zero and `count` returns zero for empty input; `average`, `min`, and `max` require at least one numeric item. `average` returns `Float`; the other numeric aggregates preserve the item type. A pipeline temporarily binds its current value as `item` and restores any outer `item` afterward. Flow execution controls `chunk`, `parallel`, and `checkpoint` configure work scheduling or resumable output; they do not change the data operations. A checkpoint stores the completed input position and committed output byte length and checksum for `write_csv`; changed committed output is rejected, while uncommitted output is truncated before continuing. Input identity is checked by path, size, and modification time, not a full content checksum. Checkpointing only supports `write_csv`; aggregate and partition state is not persisted.

During `check`, a literal `false` left operand of `and` and a literal `true`
left operand of `or` make the right operand statically unreachable, so it is
not analyzed. For non-literal left operands, both operands are checked even
though runtime evaluation may short-circuit.

Named Struct match patterns validate every selected field against the declared
field type; omitted fields are unconstrained. Their coverage analysis expands
selected fields into declaration order with wildcards for omitted fields, so
exhaustiveness checks stay consistent with positional Struct patterns.

`step_by N` is an ordered pipeline step that keeps the first item reaching it
and then every Nth item; N must be positive. The items are counted after all
preceding pipeline steps, and `step_by` cannot be combined with `parallel` or
`checkpoint`.

`enumerate(sequence)` eagerly returns an Array of `(Int, value)` tuples with
zero-based indexes. It accepts arrays, lists, tuples, ranges, and strings;
string items are Unicode scalar values. Enumerating a heterogeneous tuple
returns an unknown inferred item type while preserving the runtime values.

`zip(left, right)` eagerly returns an Array of tuples pairing corresponding
items from two arrays, lists, tuples, ranges, or strings. It stops when either
sequence ends; string items are Unicode scalar values. The returned values are
copies, and neither input is modified.

`length(value)` is the collection/string size function; `count(value)` is an
alias retained for compatibility. In pipeline position, terminal `count`
instead counts items that survive all preceding steps. Likewise,
`any(collection)` and `all(collection)` operate on an already-built Boolean
collection, while their pipeline terminals reduce the values reaching them and
can stop consuming a lazy source early. `total(collection)` sums an existing
numeric collection or range; pipeline `sum` also supports transformed and CSV
streams.
`mean(collection)` is the statistical function for an existing numeric
collection or range; pipeline `average` aggregates the values reaching that
terminal, including values from filters, derivations, and CSV streams.

An `open "path.si" as alias` import resolves relative paths from the importing
file and accepts absolute paths. Canonical paths determine module identity and
cycle detection; nested imports resolve from their own module file. Each module
must return one value, which becomes the alias. Modules may declare named
exports with `export name, other`; `open "path.si" exposing name, other`
binds only those explicitly exported values into the importing scope. Exported
names can be renamed with `as`, for example `exposing name as local_name`.
Structs and enums can also be exported and imported by name. Imported type
aliases retain the source declaration's nominal identity, and exported struct
messages retain their defining module's captured bindings. Exported functions
capture the module bindings they depend on. Other declarations remain private,
and named imports are checked for missing or duplicate local names.

At runtime, parsed programs are cached, but each import evaluates the module
again in an isolated evaluator; repeated imports do not share module state.
`check` recursively analyzes imported modules without executing them. Cycles are
rejected with the canonical import chain. Runtime checks handle file access,
bounds, division by zero, integer overflow, matrix shape, and dynamic
collection operations.

The formatter normalizes indentation for colon-delimited blocks, preserves strings and comments, and always emits deterministic newline output.
