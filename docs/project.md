# SimplyLang Project Contract

SimplyLang does not require a project manifest yet. A project may use this layout:

```text
project/
├── src/
│   └── main.si
├── tests/
└── README.md
```

Application source belongs in `src/`, with `src/main.si` as the conventional entry point. Standalone SimplyLang tests belong directly in `tests/` and must use the `.si` extension. `simply test` discovers only direct `tests/*.si` files, sorts them by path, runs each through the normal evaluator, and continues after failures.

The CLI is available through Cargo during development, for example
`cargo run -- run src/main.si` or `cargo run -- test`. Build a release binary
with `cargo build --release`, or install it locally with `cargo install --path
.`; after that, replace `cargo run --` with `simply`.

The repository examples are organized by feature. The standard-library directory
contains runnable examples for built-ins and a separate imported-value fixture;
the fixture is intentionally not a standalone program. Declarative Flow demos
live under `examples/10-flow/`; benchmark programs and large input fixtures
live under `examples/99-bench/`.

Rust integration tests in `tests/*.rs` are handled by Cargo and are not
SimplyLang test files. They are grouped by user-facing language domain, with
shared CLI helpers in `tests/common/`. Nested files, non-`.si` files, examples,
and imported module fixtures are not discovered by `simply test`.

Run a project test suite from the project root:

```bash
simply test
```

The command exits `0` when every discovered test passes and non-zero when any test fails. A malformed test reports its diagnostic, does not stop discovery of later tests, and contributes to the final failure count.
