# SimplyLang Errors

Errors are returned as structured diagnostics rather than panics. Each diagnostic has a category, stable code, source span, message, and rendered source context when source is available.

Categories are `Lex`, `Parse`, `Semantic`, `Runtime`, and `Command`. Codes are defined centrally by `DiagnosticCode` in `src/error.rs`; the code determines the category and is stable across rendering. Examples include:

- `E0101` invalid character
- `E0102` unterminated string
- `E0103` unexpected token
- `E0106` invalid numeric literal
- `E0001` undefined variable
- `E0003` type mismatch
- `E0202` division by zero
- `E0204` import failure
- `E0018` invalid numeric text known during `check`
- `E0207` invalid numeric text discovered while running the program
- `E0208` runtime type mismatch
- `E0209` runtime declaration conflict
- `E0210` reassignment or mutation of an immutable binding

Command-line usage errors are reported as `Command error` (`E0301`). When a
source file is passed without a command, the diagnostic suggests the normal
form, such as `simply run program.si`; use `simply check program.si` when only
syntax and semantics should be validated.

All `SimplyError` values use the same renderer, including lexer, parser, semantic, runtime, command-line, REPL, and project-test errors. The layout keeps the stable code and detailed message, then adds a plain-language explanation and a practical next step. Newlines and terminal control characters in paths, source excerpts, and messages are escaped so they cannot break the layout or inject terminal formatting. Source markers use Unicode grapheme widths and terminal display cells, with tabs expanded at fixed four-cell stops. On a terminal, long source lines are clipped around the error to the current terminal width; if width detection is unavailable, the full source line is rendered without terminal-dependent clipping. Resizing affects only the excerpt width, never the reported source location. Static errors are reported by `check`; normal execution reports structured runtime errors for conditions that depend on evaluated values. Use `check` explicitly when static validation must happen before execution.

The command exits non-zero for invalid source, failed checks, and runtime failures. The REPL prints diagnostics and continues reading subsequent input.

Text file read/write failures and out-of-range string indexes or substrings are
reported as runtime diagnostics. File diagnostics include the requested path
and the operating-system error; text reads reject content that is not valid
UTF-8.
