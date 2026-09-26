# SimplyLang Semantics

The front end lexes source into tokens, parses tokens into an AST, and performs semantic analysis for `check`. Normal file execution evaluates the parsed program with runtime validation; use `check` when static diagnostics are required before execution.

The evaluator maintains indexed stacks of value scopes and runtime types. Bindings are immutable unless declared with `mut`; reassignment, indexed writes, and list add/remove require a mutable binding. Definitions are local to the current scope; reassignment updates the nearest existing mutable binding. The reassignment value may start on the same line as `->` or on the following line, which is useful for longer expressions:

```simply
mut name is 10
name ->
    name + 10
```

Writing `name + 10` alone only evaluates a calculation and does not change `name`. Function parameters and loop variables are immutable by default and may be prefixed with `mut`. `if` and `for` scopes are discarded after their bodies; `while` uses the surrounding scope. Collection values use copy-on-write storage, so aliases detach when one mutable binding mutates a collection.

`and` and `or` evaluate the right operand only when needed. Pipeline sources may be arrays, lists, lazy ranges, or CSV streams. `where` requires a boolean result and selects one subset; it preserves source order and does not mutate the source. `derive` transforms each item; transformed values become the input to later operations, and source order is preserved without mutating the source. `partition` is available in both Pipeline expressions and Flow declarations. It requires at least two distinct category names, classifies each item into at most one category, evaluates rules top-to-bottom, and uses the first matching rule. Unmatched items without `otherwise` are omitted. Repeated category names merge into one bucket. It preserves input order within each category and is terminal. Aggregation and `write_csv(path)` are also terminal operations. `sum` returns zero and `count` returns zero for empty input; `average`, `min`, and `max` require at least one numeric item. `average` returns `Float`; the other numeric aggregates preserve the item type. A pipeline temporarily binds its current value as `item` and restores any outer `item` afterward. Flow execution controls `chunk`, `parallel`, and `checkpoint` configure work scheduling or resumable output; they do not change the data operations. A checkpoint stores the completed input position and committed output byte length and checksum for `write_csv`; changed committed output is rejected, while uncommitted output is truncated before continuing. Input identity is checked by path, size, and modification time, not a full content checksum. Checkpointing only supports `write_csv`; aggregate and partition state is not persisted.

Imports resolve relative paths from the importing file, reuse parsed programs, execute in an isolated evaluator, and must produce a value with `return`. Cyclic imports are rejected. Runtime checks handle file access, bounds, division by zero, integer overflow, matrix shape, and dynamic collection operations.

The formatter normalizes indentation for colon-delimited blocks, preserves strings and comments, and always emits deterministic newline output.
