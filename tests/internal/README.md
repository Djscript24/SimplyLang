# Internal unit tests

These suites exercise private parser, evaluator, diagnostic, and runtime
invariants that have no stable public Rust API. Their source now lives under
`tests/internal/`, but they remain unit tests: the `#[cfg(test)]` path
registrations in the corresponding `src/` modules attach each suite to the
module it tests. This preserves private access without making implementation
details public.

Language behavior, CLI behavior, and regression tests belong in the top level
of `tests/` and exercise SimplyLang through its observable source/CLI behavior
wherever practical.

| Source module | Internal unit suite |
|---|---|
| `src/error.rs` | `error.rs` |
| `src/evaluator.rs` | `evaluator.rs` |
| `src/formatter.rs` | `formatter.rs` |
| `src/lexer.rs` | `lexer.rs` |
| `src/parser.rs` | `parser.rs` |
| `src/runtime/arena.rs` | `runtime_arena.rs` |
| `src/runtime/files.rs` | `runtime_files.rs` |
| `src/runtime/operations.rs` | `runtime_operations.rs` |
| `src/runtime/scope.rs` | `runtime_scope.rs` |
| `src/runtime/storage.rs` | `runtime_storage.rs` |
| `src/runtime/value.rs` | `runtime_value.rs` |
| `src/types.rs` | `types.rs` |

The evaluator's parallel-worker test uses a test-only thread identifier hook
because actual worker concurrency is not a language-visible property.
