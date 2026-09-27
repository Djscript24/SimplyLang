# SimplyLang Language

This document describes the language implemented by the current interpreter.
Statements are newline-oriented and blocks close with `end`.

## Source Files

Simply source files use the `.si` extension. `#` starts a comment outside a string. Strings are double-quoted and support `\\n`, `\\t`, `\\r`, `\\"`, and `\\\\` escapes.

## Statements

- `name is expression` defines an immutable binding; prefix with `mut` for a mutable binding (`mut name as Type is expression`).
- `name -> expression` reassigns an existing mutable binding.
- `Say expression` prints a value without ending the line.
- `Sayln expression` prints a value followed by a newline.
- `fn name(parameters) gives Type: ... end` defines a function. Functions may be
  nested inside functions or control-flow blocks and resolve visible lexical bindings.
  A nested function can be returned, stored in a binding, and called later as a
  closure; captured bindings are immutable snapshots.
- `type Name: ... end` declares a nominal struct with ordered, typed fields.
  Construct instances positionally with `Name(value, ...)`.
- `on Name receive message(parameters): ... end` defines behavior for that
  struct. `instance :: message(arguments)` dispatches it, passing declared
  fields into the message scope. Reassigning a field binding within a message
  updates the persistent instance and is checked against the field's declared
  type.
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
variants with an optional single payload pattern, and positional Struct
patterns. For example, `(left, (right, _))` matches nested tuples,
`Result::Ok((left, right))` destructures a tuple payload, and
`Person(name, Address(city))` matches nested Structs. Struct fields are matched
in declaration order, using nominal type identity and exact field count;
named-field patterns are not supported. Sequence patterns use brackets, such
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
enum identity, variant, and payload.

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
- `Ask("prompt")` writes a prompt and reads one line as a `String`.
  `Ask("prompt", Int)`, `Ask("prompt", Float)`, `Ask("prompt", String)`, and
  `Ask("prompt", Bool)` parse the line into the requested primitive type.
  String input preserves leading and trailing spaces while removing its line
  ending; typed input ignores surrounding whitespace. Invalid typed input and
  end-of-file are runtime errors.
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
