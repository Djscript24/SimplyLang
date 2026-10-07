# SimplyLang Grammar

The grammar below is an implementation-oriented summary, not a complete parser generator grammar. Whitespace and comments may occur between tokens; newlines terminate ordinary statements, except that a reassignment may put its value on the following line.

```ebnf
program        = { newline | statement newline } ;
statement      = say | assignment | reassignment | destructure
               | destructure_assignment | call | function
               | conditional | for_loop | while_loop | return
               | try_statement | throw
               | break | continue | import | export | collection_op
               | enum | struct | message | expression ;
flow           = "flow" name "from" expression ":" newline
                 { flow_step newline } "end" ;
flow_step      = pipeline_step | "chunk" integer | "parallel" integer
               | "checkpoint" expression ;
pipeline_step  = "where" expression | "derive" expression
               | "take" integer | "skip" integer | "step_by" integer
               | "take_while" expression
               | "drop_while" expression
               | "distinct"
               | "partition" name ":" newline
                 { partition_rule newline } "end"
               | "sum" | "count" | "average" | "min" | "max" | "any" | "all"
               | "write_csv" "(" expression ")" ;
partition_rule = expression "->" name | "otherwise" "->" name ;
assignment     = [ "mut" ] name [ "as" type ] "is" expression ;
reassignment   = name "->" [ newline ] expression ;
destructure    = [ "mut" ] ( tuple_target | sequence_target ) "is" expression ;
destructure_assignment = ( tuple_target | sequence_target )
                         "->" [ newline ] expression ;
tuple_target   = "(" destructure_target { "," destructure_target } ")" ;
sequence_target = "[" [ destructure_target { "," destructure_target }
                      [ "," rest_target ] | rest_target ] "]" ;
destructure_target = name | "_" | tuple_target | sequence_target ;
rest_target    = "..." name ;
say            = "Say" expression | "Sayln" expression ;
function       = "fn" name "(" [ parameters ] ")" [ "gives" type ] ":"
                 newline { statement newline } "end" ;
call           = name "(" [ call_arguments ] ")" ;
message_call   = expression "::" name [ "(" [ expressions ] ")" ] ;
enum           = "enum" name ":" newline
                 { name [ "as" type ] newline } "end" ;
struct         = "type" name ":" newline
                 { struct_field } "end" ;
struct_field   = name "as" type newline ;
message        = "on" name "receive" name [ "(" [ message_parameters ] ")" ] ":"
                 newline { statement newline } "end" ;
conditional    = "if" expression ":" newline { statement newline }
                 conditional_tail ;
conditional_tail = "end"
                 | "else" ":" newline { statement newline } "end"
                 | "else" "if" expression ":" newline
                   { statement newline } conditional_tail ;
for_loop       = "for" [ "mut" ] name "in" expression ":" newline { statement newline } "end" ;
while_loop     = "while" expression ":" newline { statement newline } "end" ;
try_statement = "try" ":" newline { statement newline }
                { "catch" [ name ] [ "as" diagnostic_code ] ":"
                  newline { statement newline } }
                [ "finally" ":" newline { statement newline } ] "end" ;
throw         = "throw" expression ;
return         = "return" expression ;
import         = "open" string ( "as" name
               | "exposing" imported_name { "," imported_name } ) ;
imported_name  = name [ "as" name ] ;
diagnostic_code = name { "." name { "-" name } } ;
export         = "export" name { "," name } ;
collection_op  = name ( "add" | "remove" ) expression ;
parameters     = parameter { "," parameter } ;
parameter      = [ "mut" | "ref" ] name [ "as" type ] ;
message_parameters = message_parameter { "," message_parameter } ;
message_parameter = [ "mut" ] name [ "as" type ] ;
call_arguments = call_argument { "," call_argument } ;
call_argument  = [ "ref" ] expression ;
expressions    = expression { "," expression } ;
type           = "String" | "Int" | "Float" | "Bool" | "Hash"
               | "Matrix" | name | "Array" "[" type "]" | "List" "[" type "]"
               | "Vector" "[" type [ "," dimension ] "]"
               | "Matrix" "[" type [ "," dimension "," dimension ] "]"
               | "Tuple" "[" type { "," type } "]"
               | "(" type "," type { "," type } ")" ;
dimension      = integer | "?" ;
match          = "match" expression ":" newline
                 { pattern [ "if" expression ] ":" newline
                   { statement newline } }
                 "end" ;
pattern        = or_pattern ;
or_pattern     = pattern_atom { "|" pattern_atom } ;
pattern_atom   = alias_pattern | literal | range_pattern | "_" | name | struct_pattern
               | enum_pattern | tuple_pattern | sequence_pattern
               | hash_pattern ;
alias_pattern  = name "@" pattern_atom ;
range_pattern  = closed_range | lower_bounded_range | upper_bounded_range ;
closed_range   = int_literal ".." int_literal ;
lower_bounded_range = int_literal ".." ;
upper_bounded_range = ".." int_literal ;
int_literal    = integer | "-" integer ;
literal        = integer | float | string | "true" | "false"
               | "-" ( integer | float ) ;
enum_pattern   = name "::" name [ "(" pattern ")" ] ;
struct_pattern = name "(" [ pattern { "," pattern }
               | field_pattern { "," field_pattern } ] ")" ;
field_pattern  = name ":" pattern ;
tuple_pattern  = "(" [ pattern { "," pattern } ] ")" ;
sequence_pattern = "[" [ pattern { "," pattern } [ "," rest_pattern ]
                      | rest_pattern ] "]" ;
rest_pattern   = "..." name ;
hash_pattern   = "{" [ hash_entry { "," hash_entry } [ "," ] ] "}" ;
hash_entry     = ( string | name ) ":" pattern ;
```

`ref` is allowed only on function parameters and corresponding function-call arguments,
for example `fn first(ref values as List[Int]) ...` and `first(ref numbers)`.
It applies to collection parameters, including collection expressions such as
`ref profile["scores"]`. A `ref` parameter is a read-only, temporary view of
the collection handle; it does not copy collection contents and cannot escape
the call. Assignment and reassignment do not accept a `ref` marker.

Match patterns are parsed in match-arm context, separately from tuple expressions.
They recursively match tuple elements, enum payloads, and Struct fields.
Positional Struct patterns use declaration order, exact arity, and nominal
type identity. Named-field patterns use `Person(name: value, age: _)`; they
may select any subset of unique declared fields and are checked by field name
and type. A Struct pattern cannot mix positional and named fields. Tuple arity
and nominal enum identity are type-checked; bindings are local to the selected arm.
Sequence patterns use bracket syntax and recursively apply ordinary patterns
to each element. Fixed-length patterns match only that exact length; a trailing
rest binding, such as `[head, ...tail]`, matches any sequence at least as long
as its prefix and binds the suffix using the source collection type. Rest
bindings use an identifier, appear at most once, and must be last. An empty
prefix (`[...tail]`) matches every length. Array and List values preserve their
collection kind in the suffix. CSV streams are matched lazily by reading only
the prefix needed to decide the pattern; their rest binding remains a lazy
stream positioned at the suffix.
Hash patterns use `{key: pattern}` with string-literal keys or bare field-name
keys. Every listed key must exist and match its nested pattern; additional keys
are allowed. `{}` matches every Hash. Hash keys are strings in the current
runtime representation, and nested values are checked dynamically because
Hash does not carry key/value type parameters.
An alias pattern binds the complete value only after its nested pattern
succeeds: `whole @ Result::Ok(value)` binds both the enum value and its
payload. Alias binds one pattern atom; use parentheses to alias a grouped OR
pattern, as in `whole @ (1 | 2)`. Aliases compose recursively with nested
patterns and do not alter their coverage.
Exhaustiveness and unreachable-pattern checks recursively analyze constructors
and their fields. Wildcard and identifier patterns are irrefutable; nested
constructor patterns are irrefutable when their subpatterns are.
Guards are ordinary Bool expressions evaluated only after a pattern succeeds,
with its bindings in scope; a false guard proceeds to the next arm. Guarded
patterns do not count toward exhaustiveness, and the checker does not reason
symbolically about guards. Destructuring declarations reuse the pattern AST but
accept only identifiers, `_`, tuples, sequences, and a final identifier rest
binding; nested combinations are supported. They evaluate the value once,
validate and collect every binding before adding any names to the scope.
Sequence destructuring supports Array, List, Range, and lazy CSV streams. CSV
suffix bindings remain lazy. Literal, range, OR, alias, enum, Struct, and Hash
patterns are not valid destructuring targets; unsupported targets are rejected
with a semantic diagnostic. Destructuring assignment uses `->` with existing
identifier targets; all targets must resolve to mutable bindings and updates
are validated before any mutation is committed. Identifier reassignment also
uses `->`. Multiple enum payloads are not supported.
OR-pattern alternatives are tried left-to-right and must bind matching names
with compatible types. Unguarded alternatives contribute coverage as a union.
Literal patterns compare Int, Float, String, and Bool values exactly, without
numeric coercion. Bool has finite coverage (`true` and `false` are exhaustive);
Float and String are open domains and require a wildcard or identifier. Int
literal and range patterns use inclusive intervals, can compose recursively,
and can prove exhaustiveness when their union covers the entire Int domain.
Ranges require at least one Int-literal bound; for example, `-10..10`,
`10..`, and `..10`. A closed range with a lower bound greater than its upper
bound is invalid.

Struct fields require type annotations and preserve declaration order.
`Person("Budi", 17)` constructs a nominal `Person` instance positionally.
Messages are declared with `on Person receive greet:` and invoked with
`person :: greet`; explicit message parameters use the ordinary function
parameter grammar.

`ref` marks a read-only Array, List, or Hash parameter and must also appear
before the matching call argument, for example `inspect(ref values)`. A ref
parameter requires an explicit collection type. It cannot be mutated, stored,
returned, or used while declaring a nested function.

Expressions also include array/list literals, tuples, named Hash blocks, matrix literals, indexing with `[]`, Hash field access with `.`, function calls, and pipeline blocks. Dot access is not struct field access.

Message dispatch is an expression suffix on a postfix receiver. It is not
chainable without explicit parentheses:

```simply
receiver :: message
receiver :: message(argument, other_expression)
(left + right) :: combine
(person :: address) :: city
```

`::` is tokenized separately from `:`. A message name is an identifier.
Struct receivers resolve by nominal type and expose their declared fields in
the message scope. Reassigning a field binding updates the persistent instance
and must match the field's declared type. Other receivers retain the original
function-dispatch behavior, with the receiver passed as the first argument.
When the target function is statically known, `check` validates its argument
count and types; an unknown target on a statically known receiver is diagnosed
before execution.
Dispatch evaluates to the message's return value, or `Unit` when it returns
nothing. `send()` is not a public function.

### Modules and imports

`open "path.si" as name` loads a Simply source module and binds its returned
value to `name`. An imported module must return an expression. Modules may also
declare named value exports with `export add, PI`; importers select them with
`open "math.si" exposing add, PI`, which binds each selected name directly.
An imported name may be renamed with `as`, as in
`open "math.si" exposing add as add_numbers`.
Struct and enum declarations can also be selected from a module's exports.
Only explicitly exported declarations are available this way. Exported
functions retain access to values they use from their defining module. Legacy
return-value imports remain supported.

Relative paths are resolved from the file containing the `open` statement, not
the process working directory. Absolute paths are accepted. Paths are
canonicalized for cycle detection and parsed-program reuse, so equivalent
relative paths and symlinks to the same file identify the same source module.
Nested imports resolve relative to their own importing module. A cycle is
rejected with the canonical import chain.

At runtime, each `open` evaluates the module in an isolated evaluator and
requires it to return a value. The parsed program is cached, but module
statements execute on every import; module state is not shared between import
occurrences. `check` recursively parses and semantically analyzes imported
modules without executing them. It can validate the returned module value and
calls through returned function values, but does not export module-local
declarations into the importer.

Enum variants use `EnumName::Variant` for unit values and
`EnumName::Variant(payload)` for one-payload values. Match patterns include
identifiers, `_`, tuples, positional Struct patterns, and recursively nested
enum patterns. A trailing expression in an arm is the match-expression result;
an arm without one evaluates to `Unit`. Enum matches must cover every variant
unless an irrefutable fallback pattern is present.
Struct and enum payloads retain their nominal type; struct payloads preserve
the shared instance and its state. Enum values do not support message
dispatch.

Built-in calls use the same call syntax as user functions. A user-defined
function with the same name takes precedence over a built-in. `Ask("prompt")`
reads a line as a String; `Ask("prompt", Int)`, `Ask("prompt", Float)`,
`Ask("prompt", String)`, and `Ask("prompt", Bool)` parse input as the selected
primitive type. Invalid typed input is a runtime error. The standard library
includes collection operations such as `range`, `length`, `count`, `contains`,
`any`, `all`, `join`, `total`, `is_empty`, and `reverse`, plus string
operations such as `trim`, `split`, `replace`, `starts_with`, and `ends_with`.
`length(collection)` returns a collection's size; the pipeline terminal `count`
counts items after pipeline transforms.
Strings support scalar-value indexing, `substring(text, start, length)`,
`characters(text)` for one-pass scalar materialization, and the character
predicates `is_ascii_alpha`, `is_ascii_digit`, and `is_whitespace`.
`read_file(path)` and `write_file(path, content)` provide UTF-8 text file I/O
without changing the separate meaning of module imports. Pipeline terminals
include `sum`, `count`, `average`, `min`, `max`, and `write_csv(path)`.
Here `count` counts pipeline items. `mean(collection)` is the
statistical function for an existing collection; `average` aggregates values
reaching a pipeline terminal.
`csv_rows(path)` creates a lazy CSV pipeline source; `to_float(text)`,
`to_int(text)`, `abs`, `round`, and `clamp` support numeric formulas; and
`csv_row(...)` constructs an output row.

Scalar math functions include `sqrt`, `pow`, `exp`, `log`, `log10`, `sin`,
`cos`, `tan`, `floor`, `ceil`, and `sign`. Vector built-ins include
`vector_add`, `vector_subtract`, `vector_scale`, `dot`, `cross`, `norm`,
`distance`, and `normalize`. Matrix built-ins include `shape`, `trace`, `rank`,
`matvec`, `transpose`, `matrix_add`,
`matrix_subtract`, `matrix_scale`, `multiply`, `identity`, `determinant`,
`inverse`, `solve`, and `least_squares`. Statistical
built-ins include `mean`, `median`, `variance`, `stddev`, `percentile`,
`covariance`, and `correlation`. `multiply(...)` and `transpose(...)` are
function-call forms of words that also participate in existing matrix syntax.

String positions count Unicode scalar values, not bytes or grapheme clusters;
combining marks therefore occupy separate positions.

Text file paths are interpreted relative to the process working directory.
File failures and out-of-range string access are reported as runtime errors.
The compiler-foundations example demonstrates character-by-character source
processing; these APIs are a foundation for future self-hosted compiler
development, not a claim that Simply is self-hosted.

The bounded declarative flow syntax is lowered to the same pipeline semantics:

```simply
flow total from values:
    where item >= 2
    derive item * 2
    sum
end
```

`where` selects items and `derive` transforms each item. The terminal may be
an aggregation, `write_csv("path")`, or `partition item: ... end`.
Flow and Pipeline share the same data-operation terminals; Flow additionally
exposes workflow execution controls.
Flows may additionally use `chunk N` to select a positive batch size and
`checkpoint "path"` to persist item progress before a `write_csv` terminal. A
`chunk` is only meaningful alongside `parallel` or `checkpoint`: it defines
the work batch size, and checkpointed output commits at the end of each such
batch. If `chunk` is used without `checkpoint`, no progress file is created.
A checkpoint stores the completed input position, committed output byte length,
and a checksum of that committed output prefix. On resume, modified committed
output is rejected and any later uncommitted output is truncated before
remaining items are processed. The input is validated using its path, size, and
modification time; it is not content-checksummed.
Checkpointing is intentionally limited to `write_csv`; aggregate, partition,
and in-memory results are not persisted.
`parallel N` is an explicit, validated worker request for Flow execution.
Pure scalar `where`/`derive` transforms run in scoped workers and merge in
source order before a deterministic aggregate. Parallel execution rejects
non-scalar input, unsupported expressions, output terminals, and checkpoint
combinations instead of silently falling back to sequential execution.
Repeated category names in a partition intentionally append to the same
category, preserving source order.
