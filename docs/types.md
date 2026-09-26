# SimplyLang Types

The interpreter supports `Unit`, `String`, `Int`, `Float`, `Bool`, `Array[T]`, `List[T]`, `Tuple[T1, T2, ...]`, `Hash`, `Tree`, `Matrix`, and function types internally.

An unannotated binding receives the inferred type of its initial expression. Bindings are immutable by default; prefix a declaration with `mut` to permit reassignment and collection mutation. A reassignment must remain compatible with the binding's type. Explicit annotations are checked when the value is defined, reassigned, inserted into a collection, or passed to a typed function parameter.

`Int` and `Float` are both numeric. Arithmetic involving either float produces `Float`; otherwise it produces `Int`. Floating-point arithmetic rejects non-finite results (`NaN`, positive infinity, and negative infinity) as runtime arithmetic errors. Division or remainder by zero is always an error. `Unknown` is used internally for values whose shape cannot be established statically, such as imported values and dynamic fields. Compatibility treats nested `Unknown` values as wildcards for collection, tuple, and function types.

Arrays and lists are homogeneous. Tuple elements may have different types and tuple indexing with a known integer index is checked statically. Hash and tree keys are strings. Matrix dimensions and numeric contents are validated by the runtime operations.

Function parameters and declared return types are optional. A function without an explicit return annotation returns `Unit` when it reaches the end. A typed function must return a compatible value on every possible branch.

Matrix multiplication validates rectangular numeric matrices and returns a `Matrix` whose cells are `Float`, including when both inputs contain only integers. Empty matrices and incompatible dimensions are runtime errors.
