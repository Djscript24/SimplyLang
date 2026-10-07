# SimplyLang Language

This document describes the language implemented by the current interpreter.
Statements are newline-oriented and blocks close with `end`.
The language-level value, object, equality, capture, and lifetime rules are
specified in [SimplyLang Memory Model v1](memory-model.md).

## Source Files

Simply source files use the `.si` extension. `#` starts a comment outside a string. Strings are double-quoted and support `\\n`, `\\t`, `\\r`, `\\"`, and `\\\\` escapes.

## Statements

- `name is expression` defines an immutable binding; prefix with `mut` for a mutable binding (`mut name as Type is expression`).
- `name -> expression` reassigns an existing mutable binding.
- `Say expression` prints a value without ending the line.
- `Sayln expression` prints a value followed by a newline.
- `fn name(parameters) gives Type: ... end` defines a function. Functions may be
  nested inside functions or control-flow blocks and resolve visible lexical bindings.
  Top-level function declarations in an executable source file are available
  throughout that file, including before their textual declaration. Imported
  modules execute in isolation and in source order; their functions capture
  module bindings when declared and must be declared before they are used.
  Nested function declarations are also created in execution order.
  A nested function can be returned, stored in a binding, and called later as a
  closure; captured bindings are immutable snapshots taken when the function is
  created, and `check` rejects rebinding or collection mutation through them.
  Nested closures do not share a live environment: a returned closure cannot
  rely on a later-declared sibling, so sibling mutual recursion is not supported.
- `type Name: ... end` declares a nominal struct with ordered, typed fields.
  Construct instances positionally with `Name(value, ...)`.
- Numeric vectors and matrices can be annotated as `Vector[Float]`,
  `Vector[Float, 3]`, `Matrix[Float]`, or `Matrix[Float, 3, 3]`. A `?` leaves
  an individual dimension dynamic, as in `Matrix[Float, ?, 3]`. Vectors use
  existing arrays, lists, or homogeneous tuples; matrices accept the
  `matrix [[...], ...]` literal or rectangular nested arrays/lists. The
  checker verifies known dimensions—including compatible matrix products and
  vector operations—and leaves unknown dimensions to runtime validation.
- `on Name receive message(parameters): ... end` defines behavior for that
  struct. `instance :: message(arguments)` dispatches it, passing declared
  fields into the message scope. Reassigning a field binding within a message
  updates the persistent instance and is checked against the field's declared
  type.
- `return expression`, `if`, `for`, `while`, `break`, and `continue` provide control flow.
- `try: ... catch error: ... finally: ... end` handles runtime errors. The `catch` binding
  receives a Hash with `message`, `code`, `category`, `line`, and `column` fields. Multiple
  `catch` clauses can filter by diagnostic code, for example
  `catch error as E.runtime.numeric.division-by-zero:`.
  `throw expression` raises a user-defined runtime error, and `finally` always runs,
  including when the error is not caught. A `finally` control statement or error takes precedence.
- `open "path.si" as name` loads a source module relative to the importing file
  and binds its returned value to `name`. Modules can declare named exports
  with `export name, other`; `open "path.si" exposing name, other` imports
  those values directly. Rename an imported value with
  `open "path.si" exposing name as local_name`. Exported structs and enums
  can be imported the same way; struct messages and nominal type identity are
  retained across aliases.
- Lists support `name add expression` and `name remove expression`.
- Arrays, lists, tuples, hashes, and matrices support the forms shown in the examples.

Conditions must be `Bool`. `if` and `for` create local scopes; `while` does not.
Functions and imports execute with isolated local state. Imported modules
return one value; explicitly selected named exports are also available, while
unselected module declarations stay private.
Relative paths use the importing module's directory and absolute paths are
accepted. Canonical paths detect cycles and identify parsed-program cache
entries. `check` recursively analyzes modules without executing them, whereas
runtime imports execute each import occurrence in an isolated evaluator and
reuse only parsed programs. A function returned by a module retains
creation-time snapshots of the module bindings it references; each import
execution creates its own function values and captures. Iterating a hash visits its values in deterministic key order.

## Expressions

Literals are integers, floating-point numbers, booleans, and strings. Expressions include identifiers, function calls, message dispatch, unary operators, binary operators, indexing, field access, collections, and pipelines. `receiver :: message` dispatches a named behavior; optional message arguments use ordinary expressions. For example, `person :: rename("Ada")`. Parentheses make a compound expression an explicit receiver, as in `(first + last) :: format`. Message dispatch returns the behavior's value and can be used anywhere an expression is accepted. Chained dispatch is not supported; parenthesize nested dispatch explicitly. For non-struct receivers, dispatch retains the existing named-function behavior with the receiver as its first argument. Struct receivers resolve messages by nominal receiver type before any global function of the same name. `send()` is not a public language function.

Enums are nominal tagged values with unit or single-payload variants:

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
```

Unit variants use `Result::None`; payload variants require one value of their
declared type. Patterns recursively support identifiers, `_`, tuples, and enum
variants with an optional single payload pattern, and positional or named
Struct patterns. For example, `(left, (right, _))` matches nested tuples,
`Result::Ok((left, right))` destructures a tuple payload, and
`Person(name, Address(city))` matches nested Structs positionally. Named fields
can be selected in any order or as a subset, as in
`Person(age: 17, name: person_name)`; fields are checked by name and type.
Positional fields use declaration order and require exact arity. Both forms
preserve nominal type identity and cannot be mixed in one pattern. Sequence patterns use brackets, such
as `[1, 2]`, `[]`, or `[[1, 2], [3, 4]]`; fixed-length patterns match Array and
List values only when the length is exact. A trailing rest binding, such as
`[head, ...tail]` or `[first, second, ...rest]`, matches any length at least as
large as its prefix and binds the remaining suffix. The rest binding is an
identifier, may appear only once, and must be last; `[...rest]` matches
sequences of any length. Array and List suffixes preserve their collection
type, integer ranges preserve their lazy range representation, and CSV stream
suffixes remain lazy. Elements may use nested literal, range, tuple, enum,
Struct, wildcard, binding, or OR-patterns. Binding names exist only within
their selected arm, and a failed nested pattern exposes no partial bindings. A trailing
expression gives the arm and match expression its value; an arm without one
returns `Unit`. Enum equality uses the existing equality operators and compares
nominal declaration, variant, and payload. Struct equality compares instance
identity rather than fields; collection equality is structural, with nested
struct values compared by identity. Function equality compares callable
identity.

Patterns are checked against the scrutinee type, including tuple arity,
nominal enum identity, and Struct identity/arity. Exhaustiveness and
unreachable-arm checks recursively account for enum variants, tuple elements,
and Struct fields. Wildcards, identifiers, and recursively irrefutable
constructor patterns cover their expected type.
Pattern validation rejects a shape or nested literal/range only when existing
static type information proves it incompatible. Unknown values—including
nested Hash values, because Hash has no generic value type—remain dynamically
checked rather than being assigned an invented static type. Alias patterns
share the nested pattern's type compatibility; OR alternatives and guards are
validated by their ordinary recursive pattern and Bool-expression rules.
An arm may add `if expression` after its pattern. Guards are evaluated only
after the pattern matches, in a temporary scope containing its bindings; a
false guard discards that scope and continues to the next arm. The expression
must have type `Bool`, with no truthiness conversion. Guarded patterns do not
contribute to exhaustiveness, and the checker does not reason symbolically
about guard expressions. Runtime errors from a guard propagate normally.
OR-patterns use `pattern | pattern`, are attempted left-to-right, and require
all alternatives to bind the same names with compatible types. Unguarded
alternatives contribute their union of coverage; a guarded OR-pattern does
not.

A `return expression` in a match arm body terminates the arm and supplies the
match-expression result rather than returning from the enclosing function.
`break` and `continue` are not valid directly in match arms, even when the
match occurs inside a loop; a loop nested inside an arm may use its own loop
control statements.

Literal patterns support exact Int, Float, String, and Bool values and compose
recursively with tuples, enum payloads, Struct fields, and OR-patterns. Numeric
types do not coerce, and strings match by exact equality. Bool is finite, so
`true` and `false` together prove exhaustiveness; Float and String remain open
domains and require a wildcard or identifier for exhaustive matching.
Guards on literal patterns have the same scope and conservative coverage
behavior as other guards.

Int range patterns use inclusive integer bounds: `0..10` matches both endpoints,
`10..` has no upper bound, and `..10` has no lower bound. At least one bound is
required; bounds must be Int literals, and a closed range whose lower bound
exceeds its upper bound is invalid. Int literals are singleton intervals for
usefulness analysis. Overlapping or adjacent ranges combine for exhaustiveness,
but gaps remain uncovered. Ranges compose with nested patterns and
OR-patterns, and guarded ranges do not contribute to exhaustiveness.
Sequence patterns recursively participate in usefulness analysis. Fixed-length
patterns cover only their exact lengths. A rest pattern covers every length at
least as large as its prefix; for example, `[]` plus `[head, ...tail]` covers
all sequence lengths. Prefix element patterns still need to cover the element
domain, and guarded patterns do not contribute to exhaustiveness.

Hash patterns use `{key: pattern}`; keys may be string literals or bare field
names, which are equivalent string keys. Listed keys are required, extra keys
are ignored, and nested value patterns use the regular pattern semantics. For
example, `{"name": person, age: 18..}` binds the `name` value when both keys
exist and the age is at least 18. `{}` matches every Hash and can complete
exhaustiveness; keyed patterns alone cannot, because arbitrary Hash values may
omit those keys. Hash values have dynamic nested types, so nested
structural checks are validated at runtime. Duplicate keys are rejected.

Alias patterns bind the entire value matched by a nested pattern. For example,
`whole @ Result::Ok(value)` binds both the complete enum value to `whole` and
its payload to `value`; `adult @ 18..` binds the original Int after the range
matches. If the nested pattern fails, no alias or nested bindings are exposed.
Aliases compose recursively with tuples, enums, Structs, Hashes, and sequence
patterns. Alias binds one pattern atom; parenthesize an OR pattern when the
alias should cover its union, as in `whole @ (1 | 2)`. Aliases retain the
nested pattern's usefulness and exhaustiveness behavior.

### Destructuring declarations

Destructuring declarations bind values from tuples and sequences outside
`match`:

```simply
pair is (10, 20)
(first, second) is pair
(head, (left, right)) is (1, (2, 3))
values is [4, 5, 6]
[first_value, ...remaining] is values
[...all_values] is values
(_, ignored) is pair
```

Targets may combine identifiers, `_`, nested tuples, and nested sequences.
Fixed-length sequence targets require an exact length; a final `...name` rest
target accepts the remaining suffix, including an empty suffix. Array and List
suffixes preserve their collection type, Range suffixes remain lazy ranges,
and CSV stream suffixes remain lazy streams. Destructuring evaluates its
right-hand value once and installs bindings only after the complete target has
matched. Duplicate names are rejected, and `_` creates no binding.

Destructuring does not support assertion-style Literal, Range, OR, or Alias
patterns, nor Enum, Struct, or Hash targets. Those patterns remain available in
`match`.

Destructuring assignment updates existing mutable variables rather than
creating bindings:

```simply
mut name is "Ada"
mut age is 36
(name, age) -> ("Grace", 37)
[name, ...others] -> ["Lin", "Ada", "Grace"]
(_, age) -> ("ignored", 38)
```

Every named target must already exist, be mutable, and have a compatible type.
Wildcard targets are ignored. The RHS is evaluated once; the complete target
shape and all targets are checked before any value is changed. Duplicate
targets are rejected. A failed shape or CSV prefix read leaves targets
unchanged. CSV rest values remain lazy descriptors; errors that occur only when
a later consumer reads the suffix are reported by that consumer. Plain
identifier reassignment continues to use `name -> value`.
Payloads can contain nominal Struct values and preserve their shared identity,
so changes made through messages remain visible after the value is extracted.
Enum values do not support message dispatch.

Struct fields are declared with a type and are available by name inside a
message body. Construction uses declaration order:

```simply
type Person:
    name as String
    age as Int
end

on Person receive greet:
    return "Hello " + name
end

person is Person("Budi", 17)
Sayln person :: greet
```

Message fields are local bindings that can be reassigned using `->`; the
assignment updates the receiver's persistent state and is checked against the
declared field type. Struct values copied between variables alias the same
instance. Messages can return values with the normal `return` statement, and
dispatch evaluates to that value (or `Unit` when no value is returned).
There is no direct struct field access syntax or automatic getter/setter
generation; state is read and changed through messages.

Operators, from lower to higher precedence, are `or`, `and`, equality, comparisons, `+ - multiply`, and `* / %`. Unary `not`, unary `-`, and `transpose` bind tightly. `and` and `or` short-circuit.

## Collections

Arrays and lists are ordered, zero-based sequences. Both support indexed reads
and writes through mutable bindings; only lists support `add` and `remove`.
Tuples are fixed-length, potentially heterogeneous ordered values and cannot
be mutated. Their indexes are zero-based integers.

Hashes are string-keyed maps backed by sorted keys, so iteration and display
order are deterministic. Indexed writes require a mutable binding. Indexing
and dot access read a value, and
`contains(map, value)` searches map values, not keys. Both can contain nested
collections.

Matrices are collections of rows. Matrix coordinates use a two-integer tuple,
`matrix[row, column]`, with zero-based, non-negative indexes; rows may be arrays
or lists. `shape(matrix)` and matrix arithmetic require non-empty, rectangular
numeric rows, and reject zero-sized dimensions. Matrix values do not support
`length` or `contains`.

`range(start, end)` represents the half-open integer sequence from `start`
through `end - 1`, with unit step. `range(start, end, step)` uses the supplied
non-zero integer step and excludes `end`; a positive step progresses upward
and a negative step downward. If the step points away from the end, the range
is empty. Ranges support indexing without negative indexes. `length` reports a
Simply `Int`; if a range's length cannot fit in that type, it reports a runtime
diagnostic instead of returning a saturated, inaccurate length. `contains`
tests integer membership without materializing the range.
Displaying a range with at most 100 values uses the familiar list form;
larger ranges display their bounds as `Range(start..end)` without iterating
through the represented values.

## Built-ins

The built-ins are:

- `range(start, end[, step])` for lazy integer sequences. They support indexing,
  `length`, `for`, and pipelines like arrays without allocating every element
  up front; operations that return a collection materialize the result.
- `length(value)` for collection or string sizes. In a pipeline, the `count`
  terminal counts items reaching the end of the transformed source.
- `assert(condition[, message])` requires a Bool condition and optionally a
  String message. It returns `Unit` when true; when false, it raises a runtime
  diagnostic with the message or `assertion failed`. The optional message is
  evaluated only when the condition is false.
- `contains(collection, value)` for membership and string substrings.
- `has_key(map, key)` checks whether a Hash contains a string key;
  unlike `contains(map, value)`, it searches keys rather than values.
- `get(map, key, default)` returns a map value or the default when the string
  key is absent. The default is evaluated only on a miss and must match the
  map's inferred value type when that type is known.
- `without_key(map, key)` returns a copy of a Hash without the named
  string key. The input is unchanged, and removing a missing key is a no-op.
- `select_keys(map, keys)` returns a copy containing only keys from an
  Array, List, or Tuple of strings. Missing and repeated requested keys are
  ignored; output order follows the map's deterministic key order.
- `keys(map)`, `values(map)`, and `entries(map)` return arrays for a Hash,
  all in deterministic key order. Keys are strings, values retain their
  inferred element type, and entries are `(key, value)` tuples.
- `text[index]` returns one Unicode scalar value as a `String`; indexes are
  zero-based and out-of-range access is a runtime error. Combining marks are
  separate scalar values. `substring(text, start, length)` uses the same
  scalar-value indexing, requires non-negative integer bounds, and reports
  out-of-range slices as runtime errors.
- `characters(text)` materializes an `Array[String]` with one Unicode scalar
  per element. Use this for repeated character-by-character traversal; direct
  string indexing locates a scalar from the beginning of the text.
- `enumerate(sequence)` returns an array of `(index, value)` tuples for an
  Array, List, Tuple, Range, or String. Indexes start at zero; strings are
  traversed by Unicode scalar value. Unlike `zip`, it takes one sequence and
  generates the index values.
- `zip(left, right)` returns an array of `(left_value, right_value)` tuples for
  two Arrays, Lists, Tuples, Ranges, or Strings. Pairing stops at the shorter
  input; string elements are Unicode scalar values. Unlike `enumerate`, it
  pairs two caller-provided sequences rather than generating indexes.
- `is_ascii_alpha(character)` and `is_ascii_digit(character)` inspect one ASCII
  lexer character; `is_whitespace(character)` recognizes Unicode whitespace.
  Each requires a string containing exactly one Unicode scalar value.
- `read_file(path)` reads UTF-8 text and `write_file(path, content)` writes or
  replaces a UTF-8 text file, returning `Unit`. Paths are interpreted relative
  to the process working directory. File access and invalid UTF-8 failures are
  reported as runtime diagnostics; `open` retains its separate module-import
  behavior.
- `parse_json(text)` converts JSON null, booleans, numbers, strings, arrays, and
  objects to `Unit`, primitive values, Arrays, and Hashes. `to_json(value)` and
  `to_json_pretty(value)` serialize those values (and Lists, Tuples, Matrices,
  and Hashes) compactly or with indentation; values with no JSON representation,
  such as Ranges and functions, produce a runtime diagnostic.
- `read_json(path)` reads and parses a UTF-8 JSON file. `write_json(path, value)`
  and `write_json_pretty(path, value)` atomically write compact or indented JSON
  respectively, returning `Unit`; file and conversion failures produce runtime
  diagnostics.
- `read_json_lines(path)` parses each non-empty line of a UTF-8 JSON Lines file
  into an Array. `write_json_lines(path, sequence)` writes an Array, List, or
  Tuple as one compact JSON value per line, with a trailing newline for each
  value. `append_json_line(path, value)` adds one compact JSON value, inserting
  a line break first if the existing file has no final newline. Blank input
  lines are ignored; invalid lines report their line number. Reading materializes
  the records in memory; use `csv_rows` and a pipeline for lazy CSV processing.
- `Ask("prompt")` writes a prompt and reads one line as a `String`.
  `Ask("prompt", Int)`, `Ask("prompt", Float)`, `Ask("prompt", String)`, and
  `Ask("prompt", Bool)` parse the line into the requested primitive type.
  String input preserves leading and trailing spaces while removing its line
  ending; typed input ignores surrounding whitespace. Invalid typed input and
  end-of-file are runtime errors.
- `any(collection)` and `all(collection)` for boolean collections. The
  function forms inspect an existing collection; the pipeline/Flow terminal
  forms can reduce filtered or derived items and short-circuit lazy sources.
- `join(collection, separator)` for string collections.
- `total(collection)` for numeric arrays, lists, tuples, and ranges. It sums an
  existing collection without pipeline transformations; the pipeline `sum`
  terminal also accepts transformed values and CSV streams.
- `mean(collection)` computes the mean of an existing numeric collection; the
  pipeline `average` terminal computes the mean of values reaching that stage.
- `trim`, `split`, `replace`, `starts_with`, and `ends_with` for strings.
  `replace` matches literal text; use `regex_replace` when the pattern should
  use regular-expression syntax.
- `regex_find_all(text, pattern)` returns all non-overlapping matches as an
  `Array[String]`; `regex_replace(text, pattern, replacement)` replaces every
  match. Patterns use Rust's Unicode-aware regular-expression syntax. Invalid
  patterns produce runtime diagnostics.
- `score_rules(rules)` evaluates an Array, List, or Tuple of
  `(label, condition, weight)` tuples. Each label is a String, condition a Bool,
  and weight an Int. It returns a Hash with the sum of weights for true
  conditions under `score`, plus ordered `matched` and `unmatched` label Arrays.
  Conditions are ordinary expressions evaluated eagerly before the function
  receives the sequence, not deferred callbacks. Total-score overflow and
  malformed rule tuples are runtime diagnostics.
- `is_empty(value)` for collections and strings.
- `reverse(sequence)` for arrays, lists, tuples, and ranges. Reversing a range
  materializes and returns an Array.
- `take N` in a pipeline or Flow to keep at most N items and stop reading the
  source once the limit is reached. N must be a non-negative integer literal;
  zero produces an empty result. `take` may precede aggregates and `write_csv`,
  but cannot be combined with `parallel` or `checkpoint`.
- `skip N` in a pipeline or Flow to discard the first N items reaching that
  step. N must be a non-negative integer literal. Its position relative to
  `where`, `take`, and `derive` determines which items are counted or discarded;
  like `take`, it cannot be combined with `parallel` or `checkpoint`.
- `step_by N` keeps the first item reaching the step, then every Nth item after
  it. N must be a positive integer literal; its position relative to `where`,
  `take`, and `derive` determines which items are counted. It cannot be combined
  with `parallel` or `checkpoint`.
- `take_while condition` keeps the matching prefix at that point in a pipeline
  or Flow and stops reading as soon as the condition is false. The condition
  must return `Bool`; unlike `where`, later items are not examined after the
  first failure.
- `drop_while condition` discards the matching prefix, then passes the first
  non-matching item and all later items without testing them again. The
  condition must return `Bool`.
- `distinct` removes duplicate values at its position in a pipeline or Flow,
  using the language's equality semantics and preserving the first occurrence
  order. Values are compared after preceding `derive` steps and before following
  steps. Because it depends on ordered input and retains seen values, it cannot
  be combined with `parallel` or `checkpoint`.
- `any` and `all` are Boolean pipeline and Flow terminals. They require Boolean
  items, short-circuit when the result is determined, and return `false` and
  `true` respectively for empty input. These terminals are distinct from the
  `any(collection)` and `all(collection)` built-in functions.
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
- `vector_add`, `vector_subtract`, `vector_scale`, `dot`, `cross`, `norm`,
  `distance`, and `normalize` operate on numeric arrays, lists, or tuples. Vectors must be
  non-empty; paired vector operations require equal lengths. `dot` preserves
  integer results for integer inputs, while norm, distance, and normalization
  return `Float` values. `cross` requires two three-dimensional vectors and
  returns a Float vector. A zero vector cannot be normalized.
- `shape`, `trace`, `rank`, `matvec`, `transpose`, `matrix_add`, `matrix_subtract`, `matrix_scale`,
  `multiply`, `identity`, `determinant`, `inverse`, `lu`, `qr`, `cholesky`,
  `solve`, and `least_squares` provide explicit matrix operations over rectangular,
  non-empty numeric row collections. `shape` returns `(rows, columns)`;
  `trace` requires a square matrix and returns its diagonal sum. `rank` returns
  the numerical rank using a scale-relative floating-point tolerance.
  `matvec(A, x)` multiplies a matrix by a vector whose length matches A's
  column count and returns one Float per row.
  multiplication requires the left column count to equal the right row count
  and returns `Float` cells. Ragged, empty, nonnumeric, and incompatible
  matrices produce runtime errors. `determinant` and `inverse` require square
  matrices; `inverse` also rejects singular matrices. `solve(A, b)` solves
  square, non-singular `A` systems with one numeric right-hand-side value per
  row, returning a Float vector; if `b` is a matrix, its rows represent the
  right-hand sides and `solve(A, B)` returns one solution column per column of
  `B`. `lu(A)` returns `(L, U, P)` with `P × A = L × U` using partial pivoting.
  `qr(A)` returns `(Q, R)` with orthogonal square `Q` and `A = Q × R`.
  `cholesky(A)` returns lower-triangular `L` such that `A = L × transpose(L)`;
  it requires a symmetric positive-definite square matrix. Use `matrix_scale`
  for scalar-matrix multiplication. `least_squares(A, b)` fits an
  overdetermined system using Householder QR; it requires at least as many
  rows as columns and full column rank.
- `mean`, `median`, `variance`, `stddev`, `percentile`, `covariance`, and
  `correlation` operate on numeric arrays, lists, tuples, and ranges.
  Variance and covariance use population conventions (divide by N).
  `percentile(values, p)` accepts `p` from 0 through 100 and uses linear
  interpolation between sorted observations. Statistics reject empty inputs;
  correlation also rejects a constant sequence. `mean(values)` is the function
  for an existing collection; `average` is a pipeline terminal for transformed
  or streamed values.
- Pipeline terminals `average`, `min`, and `max` aggregate numeric streams in a
  single pass.
- `type_of(value)` for runtime inspection. Use `Say` or `Sayln` for output.

Argument counts and supported value types are checked before execution when statically knowable. Dynamic values remain runtime-validated.

Floating-point results must remain finite; overflow to `NaN` or infinity is reported as a runtime arithmetic error. Matrix multiplication, matrix-vector multiplication, cross products, trace, determinant, inverse, decompositions, `solve`, and `least_squares` produce floating-point results.

Vector dot/add/subtract and matrix element-wise operations are linear in the
number of elements. Norm, distance, and normalization are O(n); matrix
transpose and element-wise operations are O(rows × columns); matrix
multiplication is O(rows × shared_dimension × columns); determinant, inverse,
`solve`, and the matrix decompositions use O(n³) arithmetic for square
matrices; `rank` uses O(rows × columns × min(rows, columns)); `matvec` uses
O(rows × columns); `least_squares` uses O(rows × columns²) arithmetic. Median and percentile sort their input and are
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
