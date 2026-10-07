# SimplyLang Errors

Simply diagnostics carry a typed identity, a category, a source span, a message,
and (when available) source and import context. The CLI renders the complete
diagnostic identity before the category and source location.

## Diagnostic code format

Canonical codes start with `E` and use dot-separated, meaning-bearing path
segments, for example `E.semantic.index.matrix.invalid`. Each segment narrows
the condition. The tree currently groups errors by compiler/runtime
responsibility, then by semantic concern, then by the specific condition.

Depth is not fixed: `E.cli.usage` is shallower than
`E.semantic.index.matrix.invalid`. `DiagnosticCode::parent_code`,
`ancestors`, and `is_descendant_of` derive ancestry from the canonical path;
callers do not need to parse the printed code. The Rust enum is the typed
identity, and one registry in `src/error.rs` associates it with its canonical
path, category, compatibility code, explanation, and suggestion. CLI rendering,
caught error fields, and semantic categories use that registry.

Leaf paths are stable once published. To add a diagnostic, choose the closest
existing conceptual parent and add a new terminal segment. If a branch becomes
too broad, add a meaningful intermediate segment for new diagnostics; do not
move or rename existing leaves, or reuse a retired identity. Gaps and uneven
depth are intentional.

Previously published numeric spellings remain accepted as compatibility
aliases in `catch error as CODE:`. New code should use canonical paths, such as
`catch error as E.runtime.numeric.division-by-zero:`. Runtime error values
expose the canonical code in `error.code`.

## Current diagnostic inventory

| Canonical code | Legacy alias | Parent | Meaning | Origin |
|---|---|---|---|---|
| `E.lex.character.invalid` | `E0101` | `E.lex.character` | Invalid source character | Lexer |
| `E.lex.string.unterminated` | `E0102` | `E.lex.string` | Unterminated string literal | Lexer |
| `E.lex.number.invalid` | `E0106` | `E.lex.number` | Invalid or out-of-range numeric literal | Lexer |
| `E.syntax.token.unexpected` | `E0103` | `E.syntax.token` | Unexpected token or delimiter | Parser |
| `E.syntax.expression.missing` | `E0104` | `E.syntax.expression` | Required expression missing | Parser |
| `E.semantic.name.undefined` | `E0001` | `E.semantic.name` | Name is not defined or visible | Semantic analysis |
| `E.semantic.type.mismatch` | `E0003` | `E.semantic.type` | Value does not satisfy required type | Semantic analysis |
| `E.semantic.ref.invalid` | `E0020` | `E.semantic.ref` | `ref` is outside its allowed function-call lifetime or violates read-only rules | Parser and semantic analysis |
| `E.semantic.binding.reassignment` | `E0105` | `E.semantic.binding` | Invalid reassignment | Semantic analysis |
| `E.semantic.declaration.duplicate` | `E0017` | `E.semantic.declaration` | Duplicate declaration | Semantic analysis |
| `E.semantic.call.invalid` | `E0002` | `E.semantic.call` | Invalid function or constructor call | Semantic analysis |
| `E.semantic.control.return-invalid` | `E0006` | `E.semantic.control` | Return is invalid in its context | Semantic analysis |
| `E.semantic.control.loop-transfer-invalid` | `E0007` | `E.semantic.control` | Break/continue is outside a loop | Semantic analysis |
| `E.semantic.control.return-missing` | `E0005` | `E.semantic.control` | Function may finish without its promised return | Semantic analysis |
| `E.semantic.conversion.literal-invalid` | `E0018` | `E.semantic.conversion` | Numeric text cannot be converted during checking | Semantic analysis |
| `E.semantic.pattern.destructure-invalid` | `E0011` | `E.semantic.pattern` | Destructuring shape does not fit the value | Semantic analysis |
| `E.semantic.collection.shape-invalid` | `E0012` | `E.semantic.collection` | Collection shape is invalid for an operation | Semantic analysis |
| `E.semantic.collection.operation-invalid` | `E0010` | `E.semantic.collection` | Collection operation is not permitted | Semantic analysis |
| `E.semantic.field.missing` | `E0013` | `E.semantic.field` | Requested field does not exist | Semantic analysis |
| `E.semantic.index.invalid` | `E0014` | `E.semantic.index` | Index operation is invalid | Semantic analysis |
| `E.semantic.index.tuple.invalid` | `E0015` | `E.semantic.index.tuple` | Tuple index is invalid | Semantic analysis |
| `E.semantic.index.matrix.invalid` | `E0016` | `E.semantic.index.matrix` | Matrix index or shape is invalid | Semantic analysis |
| `E.semantic.catch.code-unknown` | `E0019` | `E.semantic.catch` | Catch filter uses an unknown code | Semantic analysis |
| `E.runtime.collection.operation` | `E0201` | `E.runtime.collection` | Collection operation failed at runtime | Runtime collections |
| `E.runtime.numeric.division-by-zero` | `E0202` | `E.runtime.numeric` | Division by zero | Runtime operations |
| `E.runtime.numeric.arithmetic-invalid` | `E0203` | `E.runtime.numeric` | Arithmetic result is invalid or out of range | Runtime operations |
| `E.runtime.module.import` | `E0204` | `E.runtime.module` | Module or file import failed | Module loader |
| `E.runtime.message.dispatch` | `E0205` | `E.runtime.message` | Message dispatch failed | Runtime objects |
| `E.runtime.operation.failed` | `E0206` | `E.runtime.operation` | Runtime operation failed without a narrower identity | Evaluator and runtime helpers |
| `E.runtime.argument.invalid` | `E0211` | `E.runtime.argument` | Runtime call arguments are not accepted | Runtime calls and built-ins |
| `E.runtime.assertion.failed` | `E0212` | `E.runtime.assertion` | An assertion condition is false | `assert` built-in |
| `E.runtime.io.operation-failed` | `E0213` | `E.runtime.io` | A runtime input/output operation failed | File APIs, prompts, and test runner |
| `E.runtime.limit.exceeded` | `E0214` | `E.runtime.limit` | A runtime resource limit was exceeded | Call depth and matrix allocation guards |
| `E.runtime.enum.variant-unknown` | `E0215` | `E.runtime.enum` | Requested enum variant is not declared | Runtime enum construction |
| `E.runtime.name.undefined` | `E0216` | `E.runtime.name` | A runtime name is not declared or in scope | Evaluator |
| `E.runtime.control.invalid` | `E0217` | `E.runtime.control` | Runtime control flow is invalid in its context | Evaluator |
| `E.runtime.conversion.failed` | `E0207` | `E.runtime.conversion` | Runtime numeric conversion failed | Built-ins |
| `E.runtime.type.mismatch` | `E0208` | `E.runtime.type` | Runtime value has the wrong type | Evaluator |
| `E.runtime.declaration.conflict` | `E0209` | `E.runtime.declaration` | Runtime declaration conflicts with a binding | Evaluator |
| `E.runtime.binding.immutable` | `E0210` | `E.runtime.binding` | Runtime mutation violates binding mutability | Evaluator |
| `E.cli.usage` | `E0301` | `E.cli` | Invalid command-line usage | CLI |

The canonical registry currently contains 41 diagnostic identities. Common
argument, assertion, file-I/O, resource-limit, unknown-name, invalid-control,
and unknown-enum-variant failures have dedicated runtime codes.
`E.runtime.operation.failed` remains a fallback for runtime failures without a
more specific stable identity, including malformed CSV records, invalid
checkpoint recovery state, evaluator invariants, and user-thrown enum values
when they are not caught. Checkpoint file-system failures use
`E.runtime.io.operation-failed`; invalid checkpoint paths use
`E.runtime.argument.invalid`; parallel evaluation preserves the diagnostic from
the operation that failed. The fallback's detailed message retains the
operation context. Give recurring, distinct conditions a dedicated child
identity rather than broadening that fallback.

## Handling errors

`catch error as CODE:` filters built-in errors by diagnostic identity. Both a
canonical code and a previous numeric alias are recognized; `check` rejects
unknown codes. An unfiltered `catch error:` handles any runtime error.

User-defined errors are enum values. This lets code use the language's type
identity and pattern matching instead of stringly typed error names:

```simply
enum FileError:
    Missing as String
    Unreadable as String
end

try:
    throw FileError::Missing("config.si")
catch error:
    match error:
        FileError::Missing(path):
            Sayln "missing: " + path
        FileError::Unreadable(reason):
            Sayln "cannot read: " + reason
        _:
            Sayln "another runtime error"
    end
end
```

`throw` accepts enum values only. A caught user-defined enum is bound directly
to the name after `catch`; built-in runtime failures bind a structured Hash
with `message`, `code`, `category`, `line`, and `column` fields. Uncaught
user-defined errors are rendered as runtime diagnostics under
`E.runtime.operation.failed`. `finally` runs after the `try`/`catch` path,
including when an error propagates.

Static errors are reported by `check`; errors that depend on evaluated values
are reported at runtime. Parse and check failures occur before runtime
handlers can execute. The REPL reports an error and continues with subsequent
input.

All `SimplyError` values use the same renderer, including lexer, parser,
semantic, runtime, command-line, REPL, and project-test errors. Newlines and
terminal control characters are escaped. Source markers use Unicode grapheme
widths and terminal display cells, with tabs expanded at fixed four-cell
stops. Long source lines are clipped around the error only when terminal width
is available.

Errors originating in imported modules retain that module's filename and
source excerpt. Nested import failures include import context, and the same
source context is retained when an imported module returns a function that
fails later when called.
