# Performance Notes

Simply is currently an interpreter. The optimization work is intentionally split between low-risk allocation improvements and semantic correctness fixes.

## Current optimizations

- Function definitions are stored behind `Arc` during invocation, and their AST bodies use shared slices so registration does not clone every statement.
- Runtime bindings use a scope-owned generational arena with unique, non-cloneable binding tokens. Scope frames own those tokens; removing a binding or popping its frame consumes the token and releases the slot. The arena rejects stale/cross-arena handles, reuses released slots with a new generation, and retires a slot rather than allowing generation overflow to make an old handle valid again. Closures capture creation-time `Value` snapshots rather than references to lexical slots.
- Mutable Arrays, Lists, and Hashes live in evaluator-owned generational arenas. Values carry weak, generational handles; cloning a value aliases the same collection node, and writes update it in place. Arena slots are intentionally retained until evaluator teardown, so unreachable nodes are not reclaimed during evaluation. Tuples and matrices keep immutable value-like storage.
- `ref` collection parameters pass those same small handles; they do not clone collection contents. The parameter declaration carries the borrow contract, so calls need no duplicate marker or runtime marker validation.
- Parsed imports are cached as shared `Arc<Program>` values. Imported programs are still evaluated in isolated module evaluators, so evaluation side effects are not cached away.
- Pipeline values are moved into the temporary `item` binding and recovered without cloning each element.
- `where`/`derive` pipelines ending in `count` or `sum` are fused across multiple stages without materializing intermediate vectors.
- Integer `start .. end` and `range(start, end[, step])` values are lazy. `for` loops and fused `count`/`sum` pipelines consume them directly, avoiding a million-element allocation for large numeric workloads.
- Range pipelines with pure scalar `where`/`derive` expressions and an aggregate terminal evaluate those expressions directly against each item, without creating and removing a temporary `item` scope for every stage. Other expressions keep the general streaming evaluator.
- Non-terminal `where`/`derive` pipelines are evaluated in one streaming pass, so each stage does not allocate a separate intermediate collection.
- `take N` short-circuits collection, range, and CSV sources as soon as N items reach that step; filters before `take` count only matching items. `take 0` does not consume the source.
- `skip N` is applied during the same streaming pass and does not materialize a prefix; items skipped after a `where` are counted only after the filter.
- `step_by N` samples in the same streaming pass without materializing skipped items; it counts only items that reach its position after preceding steps.
- `enumerate(sequence)` materializes an array of index/value tuples; use it for reusable indexed data, while direct `for` iteration remains preferable when only one pass is needed.
- `zip(left, right)` iterates both input sequences together and stops at the shorter one, so a long range is not fully consumed when paired with a shorter collection.
- `take_while condition` stops a lazy range or CSV stream at the first item that fails the condition, without scanning the remaining source.
- `drop_while condition` evaluates its condition only on the initial matching prefix, then passes the remaining stream through without further condition checks.
- `distinct` retains previously observed values while streaming; it uses language equality, so arbitrary values work consistently but membership checks grow with the number of unique values.
- Boolean `any` and `all` terminals stop consuming collections, ranges, or CSV rows once the result is determined.
- `csv_rows(path) -> where/derive -> write_csv(path)` keeps one input row and one output row in memory at a time. CSV fields support quoted commas, escaped quotes, and newlines inside quoted fields.
- Numeric formulas can use `to_float` and `csv_row` while preserving streaming memory usage; aggregation and cleanup are separate passes over the source.
- `average`, `min`, and `max` are streaming terminals for ranges, collections, and CSV sources. They retain only aggregate state, not all rows.
- Flow `chunk N` defines work batch size. When combined with `checkpoint`, each completed batch is a commit boundary. `checkpoint "path"` records processed input positions and committed output byte lengths and checksums for resumable `write_csv` output; changed committed output is rejected and incomplete output after the last committed boundary is truncated on resume. The input identity uses path, size, and modification time rather than a full content checksum. Aggregate, partition, and in-memory state are intentionally not checkpointed.
- Flow `parallel N` launches scoped standard-library workers for pure scalar
  `where`/`derive` transforms over primitive collection or range values when the
  terminal is a deterministic aggregate. Chunks are merged in source order.
  Output, closures, calls, nested collections, CSV rows, and I/O are rejected
  with `parallel` rather than silently falling back because evaluator scopes
  and runtime collections use single-threaded shared-storage handles.
- Loop values are moved directly into the loop binding.
- Matrix addition, multiplication, and transpose validate shape once and read numeric cells directly without allocating a temporary converted matrix.
- Matrix determinant and inverse use partial pivoting in O(n³) time; both validate their square workspace against the one-million-cell matrix limit before allocation.
- `solve(A, b)` uses partial-pivoted elimination in O(n³) time and solves the system directly without computing `inverse(A)`.
- `least_squares(A, b)` uses Householder QR in O(m × n²) time, checks its workspace against the matrix-cell limit, and rejects rank-deficient or underdetermined inputs.
- `trace(A)` is O(n) for an n × n matrix; `rank(A)` uses O(m × n × min(m, n)) elimination and a scale-relative tolerance; `matvec(A, x)` is O(m × n).
- `cross(a, b)` computes the 3D vector cross product in constant time and rejects vectors that are not exactly length three.
- Matrix-producing operations reject results larger than one million cells before allocating, preventing a single operation from requesting unbounded memory.
- Explicit vector and matrix math reuses the runtime's existing collection values. Vector dot products, norms, distances, and one-pass statistics are O(n); matrix multiplication is the straightforward O(m × n × k) algorithm. Median and percentile sort a working copy in O(n log n).
- Lexer and parser vectors reserve an estimated capacity to reduce reallocations.
- Runtime value scopes, evaluator type scopes, and semantic-analysis scopes maintain nearest-binding indexes, avoiding a scan through every nested frame on lookup.
- Runtime and type scope stacks reuse popped hash maps across function calls and nested blocks, reducing allocation churn without changing shadowing semantics.
- Function calls avoid creating a separate function/capture scope when a capture-free named function already resolves to the same function value in the caller; parameter bindings remain isolated in their own scope. The release `function-dispatch` workload improved from roughly 14 ms to a roughly 10 ms median across three runs.

## Important semantic safeguards

- Duplicate function declarations return diagnostics instead of panicking.
- Nested collection and tuple types treat `Unknown` recursively as a wildcard.
- Typed functions must return on every possible branch. A return inside a loop alone does not satisfy this requirement.

## Deliberately deferred work

Large `Value` reads and indexed results still produce owned snapshots of collection contents. Collection handles themselves are cheap aliases. Matrix results still use nested value rows; a flat numeric matrix representation should be introduced only together with a stable matrix type contract.

Lexical slot resolution, broader lazy pipeline fusion, byte-oriented lexing, and `HashMap`-backed hashes should be benchmarked before changing their current deterministic and readable behavior.

## Suggested benchmark workloads

Use `cargo run -- bench <file.si>` to measure the front-end and runtime phases. The command runs ten iterations, reports average lex, parse, semantic, runtime, and total time, and suppresses program output during the runtime phase. Runtime state is reset for each iteration.

Benchmark these separately in release mode before further changes:

- a pipeline with one million integers and multiple where/derive stages;
- `examples/99-bench/large-pipeline.si` is the reproducible one-million-element workload for that case;
- `examples/99-bench/runtime-pipeline.si` for a repeatable 10,000-element fused pipeline;
- `examples/99-bench/function-dispatch.si` for repeated function calls;
- `examples/99-bench/function-body-registration.si` for function registration with a larger body;
- `examples/99-bench/closure-capture.si` for selective closure capture with many visible bindings;
- `examples/99-bench/hash-lookup.si` for deterministic hash lookup;
- `examples/99-bench/nested-read.si` for nested list/hash indexing;
- `examples/99-bench/matrix-workload.si` for repeated matrix operations;
- deeply nested scopes with repeated identifier lookup;
- repeated imports of the same module;
- large matrix multiplication and transpose;
- lexing and parsing a generated source file.
