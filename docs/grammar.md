# SimplyLang Grammar

The grammar below is an implementation-oriented summary, not a complete parser generator grammar. Whitespace and comments may occur between tokens; newlines terminate ordinary statements, except that a reassignment may put its value on the following line.

```ebnf
program        = { newline | statement newline } ;
statement      = say | assignment | reassignment | call | function
               | conditional | for_loop | while_loop | return
               | try_statement | throw
               | break | continue | import | collection_op
               | destructure | expression ;
flow           = "flow" name "from" expression ":" newline
                 { flow_step newline } "end" ;
flow_step      = pipeline_step | "chunk" integer | "parallel" integer
               | "checkpoint" expression ;
pipeline_step  = "where" expression | "derive" expression
               | "partition" name ":" newline
                 { partition_rule newline } "end"
               | "sum" | "count" | "average" | "min" | "max"
               | "write_csv" "(" expression ")" ;
partition_rule = expression "->" name | "otherwise" "->" name ;
assignment     = [ "mut" ] name [ "as" type ] "is" expression ;
reassignment   = name "->" [ newline ] expression ;
say            = "Say" expression ;
function       = "fn" name "(" [ parameters ] ")" [ "gives" type ] ":"
                 newline { statement newline } "end" ;
conditional    = "if" expression ":" newline { statement newline }
                 [ "else" ":" newline { statement newline } ] "end" ;
for_loop       = "for" [ "mut" ] name "in" expression ":" newline { statement newline } "end" ;
while_loop     = "while" expression ":" newline { statement newline } "end" ;
try_statement = "try" ":" newline { statement newline }
                { "catch" [ name ] [ "as" diagnostic_code ] ":"
                  newline { statement newline } }
                [ "finally" ":" newline { statement newline } ] "end" ;
throw         = "throw" expression ;
return         = "return" expression ;
import         = "open" string "as" name ;
collection_op  = name ( "add" | "remove" ) expression ;
parameters     = [ "mut" ] name [ "as" type ] { "," [ "mut" ] name [ "as" type ] } ;
type           = "String" | "Int" | "Float" | "Bool" | "Hash" | "Tree"
               | "Matrix" | "Array" "[" type "]" | "List" "[" type "]"
               | "Tuple" "[" type { "," type } "]" ;
```

Expressions also include array/list literals, tuples, named hash/tree blocks, matrix literals, indexing with `[]`, field access with `.`, function calls, and pipeline blocks.

Built-in calls use the same call syntax as user functions. The standard library includes collection operations such as `range`, `length`, `count`, `contains`, `any`, `all`, `join`, `total`, `is_empty`, and `reverse`, plus string operations such as `trim`, `split`, `replace`, `starts_with`, and `ends_with`. Pipeline terminals include `sum`, `count`, `average`, `min`, `max`, and `write_csv(path)`. `csv_rows(path)` creates a lazy CSV pipeline source; `to_float(text)`, `to_int(text)`, `abs`, `round`, and `clamp` support numeric formulas; and `csv_row(...)` constructs an output row.

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
