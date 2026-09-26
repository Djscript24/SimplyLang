# SimplyLang Language

This document describes the language implemented by the current interpreter.
Statements are newline-oriented and blocks close with `end`.

## Source Files

Simply source files use the `.si` extension. `#` starts a comment outside a string. Strings are double-quoted and support `\\n`, `\\t`, `\\r`, `\\"`, and `\\\\` escapes.

## Statements

- `name is expression` defines an immutable binding; prefix with `mut` for a mutable binding (`mut name as Type is expression`).
- `name -> expression` reassigns an existing mutable binding.
- `Say expression` prints a value.
- `fn name(parameters) gives Type: ... end` defines a function. Functions may be
  nested inside functions or control-flow blocks and resolve visible lexical bindings.
  A nested function can be returned, stored in a binding, and called later as a
  closure; captured bindings are immutable snapshots.
- `return expression`, `if`, `for`, `while`, `break`, and `continue` provide control flow.
- `try: ... catch error: ... finally: ... end` handles runtime errors. The `catch` binding
  receives a tree with `message`, `code`, `category`, `line`, and `column` fields. Multiple
  `catch` clauses can filter by diagnostic code, for example `catch error as E0202:`.
  `throw expression` raises a user-defined runtime error, and `finally` always runs,
  including when the error is not caught. A `finally` control statement or error takes precedence.
- `open "path.si" as name` loads a source value relative to the importing file.
- Lists support `name add expression` and `name remove expression`.
- Arrays, lists, tuples, hashes, trees, and matrices support the forms shown in the examples.

Conditions must be `Bool`. `if` and `for` create local scopes; `while` does not. Functions and imports execute with isolated local state. Iterating a hash or tree visits its values in deterministic key order.

## Expressions

Literals are integers, floating-point numbers, booleans, and strings. Expressions include identifiers, function calls, unary operators, binary operators, indexing, field access, collections, and pipelines.

Operators, from lower to higher precedence, are `or`, `and`, equality, comparisons, `+ - multiply`, and `* / %`. Unary `not`, unary `-`, and `transpose` bind tightly. `and` and `or` short-circuit.

## Built-ins

The built-ins are:

- `range(start, end)` for lazy integer sequences. They support indexing,
  `length`, `for`, and pipelines like arrays without allocating every element
  up front; operations that return a collection materialize the result.
- `length(value)` and `count(value)` for collection or string sizes.
- `contains(collection, value)` for membership and string substrings.
- `text[index]` returns one Unicode scalar value as a `String`; indexes are
  zero-based and out-of-range access is a runtime error. Combining marks are
  separate scalar values. `substring(text, start, length)` uses the same
  scalar-value indexing, requires non-negative integer bounds, and reports
  out-of-range slices as runtime errors.
- `characters(text)` materializes an `Array[String]` with one Unicode scalar
  per element. Use this for repeated character-by-character traversal; direct
  string indexing locates a scalar from the beginning of the text.
- `is_ascii_alpha(character)` and `is_ascii_digit(character)` inspect one ASCII
  lexer character; `is_whitespace(character)` recognizes Unicode whitespace.
  Each requires a string containing exactly one Unicode scalar value.
- `read_file(path)` reads UTF-8 text and `write_file(path, content)` writes or
  replaces a UTF-8 text file, returning `Unit`. Paths are interpreted relative
  to the process working directory. File access and invalid UTF-8 failures are
  reported as runtime diagnostics; `open` retains its separate module-import
  behavior.
- `any(collection)` and `all(collection)` for boolean collections.
- `join(collection, separator)` for string collections.
- `total(collection)` for numeric arrays, lists, and tuples.
- `trim`, `split`, `replace`, `starts_with`, and `ends_with` for strings.
- `is_empty(value)` for collections and strings.
- `reverse(sequence)` for arrays, lists, and tuples.
- `csv_rows(path)` for lazy CSV input. Use it as a pipeline source and finish
  with `write_csv(path)` to select/derive rows and write output incrementally.
  Both Pipeline expressions and Flow declarations support `partition` as a
  terminal classification operation.
  Flows can add `chunk N` and `checkpoint "path"` before a `write_csv(path)`
  terminal to checkpoint long-running output and resume after an interruption.
  `chunk N` sets the work batch size and must be
  combined with `parallel` or `checkpoint`.
  `parallel N` runs pure scalar `where`/`derive` expressions in up to `N`
  isolated standard-library worker threads and merges chunks in source order
  before aggregate terminals (`sum`, `count`, `average`, `min`, and `max`).
  `parallel` rejects unsupported expressions, non-scalar inputs, non-aggregate
  terminals, `write_csv`, and checkpoint combinations rather than silently
  switching to sequential execution.
  Literals, `item`, scalar operators, and `not`/unary `-` are the parallel-safe
  subset. Closures, calls, collections (including CSV rows), and I/O are not
  supported with `parallel`.
  Aggregate terminals do not resume from a position yet because their
  aggregate state is not persisted. A checkpoint records the completed input
  position and committed output prefix checksum; resume rejects changed
  committed output and truncates uncommitted output before continuing. Input
  identity is checked using path, size, and modification time, not a full
  content checksum. Checkpointing is only supported for `write_csv`; it cannot
  resume aggregate, partition, or in-memory results.
- `to_float(text)` converts a numeric string (including decimals and exponent
  notation, such as `"3.5"` or `"1e2"`) to a finite Float.
- `to_int(text)` converts a whole-number string such as `"42"` to an Int;
  decimal strings such as `"4.2"` are not accepted. Invalid literal strings are
  reported by `check`, while values only known at runtime are checked then.
- `csv_row(...)` constructs a mixed-type output row for CSV rewriting.
- `abs(value)`, `round(value, decimals)`, and
  `clamp(value, minimum, maximum)` support numeric cleanup without materializing
  a large dataset.
- `sqrt`, `pow`, `exp`, `log`, `log10`, `sin`, `cos`, `tan`, `floor`, `ceil`,
  and `sign` provide scalar mathematical operations. Scalar transcendental
  functions return finite `Float` values; `sqrt` rejects negative inputs and
  logarithms reject non-positive inputs.
- `vector_add`, `vector_subtract`, `vector_scale`, `dot`, `norm`, `distance`, and
  `normalize` operate on numeric arrays, lists, or tuples. Vectors must be
  non-empty; paired vector operations require equal lengths. `dot` preserves
  integer results for integer inputs, while norm, distance, and normalization
  return `Float` values. A zero vector cannot be normalized.
- `shape`, `transpose`, `matrix_add`, `matrix_subtract`, `matrix_scale`,
  `multiply`, and `identity` provide explicit matrix operations over rectangular,
  non-empty numeric row collections. `shape` returns `(rows, columns)`;
  multiplication requires the left column count to equal the right row count
  and returns `Float` cells. Ragged, empty, nonnumeric, and incompatible
  matrices produce runtime errors. Use `matrix_scale` for scalar-matrix
  multiplication.
- `mean`, `median`, `variance`, `stddev`, `percentile`, `covariance`, and
  `correlation` operate on numeric arrays, lists, tuples, and ranges.
  Variance and covariance use population conventions (divide by N).
  `percentile(values, p)` accepts `p` from 0 through 100 and uses linear
  interpolation between sorted observations. Statistics reject empty inputs;
  correlation also rejects a constant sequence.
- Pipeline terminals `average`, `min`, and `max` aggregate numeric streams in a
  single pass.
- `type_of(value)` and `print(value)` for inspection and output.
- `send(receiver, "message", ...)` for explicit message dispatch.

Argument counts and supported value types are checked before execution when statically knowable. Dynamic values remain runtime-validated.

Floating-point results must remain finite; overflow to `NaN` or infinity is reported as a runtime arithmetic error. Matrix multiplication always produces floating-point cells.

Vector dot/add/subtract and matrix element-wise operations are linear in the
number of elements. Norm, distance, and normalization are O(n); matrix
transpose and element-wise operations are O(rows × columns); matrix
multiplication is O(m × n × k). Median and percentile sort their input and are
O(n log n); mean, variance, standard deviation, covariance, and correlation
use one-pass updates and are O(n). These operations use existing Simply
collections rather than a separate tensor representation. They provide a
foundation for future mathematical and self-hosting work, not a high-level
machine-learning library.

String indexing, slicing, character inspection, and text file I/O provide the
foundation for future self-hosted compiler development. The
`examples/11-compiler-foundations/mini-lexer.si` example scans a small Simply
source file and creates token-like values; it is a demonstration, not a
complete lexer or a self-hosted compiler.
