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
the fixture is intentionally not a standalone program. The module example's
entry point is `examples/17-modules/main.si`; its files under `modules/` are
imported fixtures, not standalone examples. Declarative Flow demos live under
`examples/10-flow/`; benchmark programs and large input fixtures live under
`examples/99-bench/`.

An `open "path.si" as value` import resolves relative paths from the file that
contains the statement; absolute paths are accepted. Nested imports use the
nested module's own directory. Canonical paths are used for module identity,
cycle detection, and parsed-program reuse. A module must `return` one value;
that value is bound to the alias. A module may additionally declare named value
exports with `export name, other`, which callers select using
`open "path.si" exposing name, other`. Only selected, explicitly exported
values are bound in the importer; an imported name may be locally renamed
with `as`, and all other module names remain private.

`simply check` uses the shared semantic analyzer and recursively checks imported
modules without executing them. `simply run` and the REPL intentionally do not
run a static preflight: they retain runtime type and operation checks. Runtime
imports execute in isolated module evaluators on every import, including
repeated imports; the parsed program is cached, but module state is not shared.
`fmt`, `bench`, `explain`, and `explain-flow` parse and semantically analyze the
current source using that same analyzer; they do not execute imports during
their analysis. `bench` subsequently executes the source for its runtime
measurement, so it may perform the program's normal side effects.

Function calls are limited to 16 nested invocations to prevent stack exhaustion.
Parallel flows accept at most 64 workers, a chunk size of 1,000,000 values, and
100,000 chunks; results are merged in source order.

Rust integration tests in `tests/*.rs` are handled by Cargo and are not
SimplyLang test files. They are grouped by user-facing language domain, with
shared CLI helpers in `tests/common/`. Nested files, non-`.si` files, examples,
and imported module fixtures are not discovered by `simply test`.

Run a project test suite from the project root:

```bash
simply test
```

The command exits `0` when every discovered test passes and non-zero when any test fails. A malformed test reports its diagnostic, does not stop discovery of later tests, and contributes to the final failure count.
