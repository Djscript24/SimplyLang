# Performance Notes

Simply is currently an interpreter. The optimization work is intentionally split between low-risk allocation improvements and semantic correctness fixes.

## Current optimizations

- Function definitions are stored behind `Arc` during invocation, and their AST bodies use shared slices so registration does not clone every statement.
- Arrays, lists, tuples, matrices, hashes, and trees use reference-counted copy-on-write storage. Reading or passing a collection is cheap; mutation detaches only when another binding still shares it.
- Parsed imports are cached as shared `Arc<Program>` values. Imported programs are still evaluated in isolated module evaluators, so evaluation side effects are not cached away.
- Pipeline values are moved into the temporary `item` binding and recovered without cloning each element.
- `where`/`derive` pipelines ending in `count` or `sum` are fused across multiple stages without materializing intermediate vectors.
- Integer `range(start, end)` values are lazy. `for` loops and fused `count`/`sum` pipelines consume them directly, avoiding a million-element allocation for large numeric workloads.
- Non-terminal `where`/`derive` pipelines are evaluated in one streaming pass, so each stage does not allocate a separate intermediate collection.
- `csv_rows(path) -> where/derive -> write_csv(path)` keeps one input row and one output row in memory at a time. CSV fields support quoted commas, escaped quotes, and newlines inside quoted fields.
- Numeric formulas can use `to_float` and `csv_row` while preserving streaming memory usage; aggregation and cleanup are separate passes over the source.
- `average`, `min`, and `max` are streaming terminals for ranges, collections, and CSV sources. They retain only aggregate state, not all rows.
- Flow `chunk N` defines work batch size. When combined with `checkpoint`, each completed batch is a commit boundary. `checkpoint "path"` records processed input positions and committed output byte lengths and checksums for resumable `write_csv` output; changed committed output is rejected and incomplete output after the last committed boundary is truncated on resume. The input identity uses path, size, and modification time rather than a full content checksum. Aggregate, partition, and in-memory state are intentionally not checkpointed.
- Flow `parallel N` launches scoped standard-library workers for pure scalar
  `where`/`derive` transforms over primitive collection or range values when the
  terminal is a deterministic aggregate. Chunks are merged in source order.
  Output, closures, calls, nested collections, CSV rows, and I/O are rejected
  with `parallel` rather than silently falling back because evaluator scopes
  and runtime collections are `Rc`-backed.
- Loop values are moved directly into the loop binding.
- Matrix addition, multiplication, and transpose validate shape once and read numeric cells directly without allocating a temporary converted matrix.
- Explicit vector and matrix math reuses the runtime's existing collection values. Vector dot products, norms, distances, and one-pass statistics are O(n); matrix multiplication is the straightforward O(m × n × k) algorithm. Median and percentile sort a working copy in O(n log n).
- Lexer and parser vectors reserve an estimated capacity to reduce reallocations.
- Runtime value scopes, evaluator type scopes, and semantic-analysis scopes maintain nearest-binding indexes, avoiding a scan through every nested frame on lookup.
- Runtime and type scope stacks reuse popped hash maps across function calls and nested blocks, reducing allocation churn without changing shadowing semantics.

## Important semantic safeguards

- Duplicate function declarations return diagnostics instead of panicking.
- Nested collection and tuple types treat `Unknown` recursively as a wildcard.
- Typed functions must return on every possible branch. A return inside a loop alone does not satisfy this requirement.

## Deliberately deferred work

Large `Value` reads and indexed results still produce owned values. Replacing this with references requires an explicit aliasing policy for mutable lists, hashes, and trees. Matrix results still use nested value rows; a flat numeric matrix representation should be introduced only together with a stable matrix type contract.

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
