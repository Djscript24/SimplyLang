# SimplyLang Types

The interpreter supports `Unit`, `String`, `Int`, `Float`, `Bool`, `Range`, `CsvStream`, `Array[T]`, `List[T]`, `Vector[T]`, `Tuple[T1, T2, ...]`, `Hash`, `Tree`, `Matrix`, `Matrix[T]`, and function types internally.

An unannotated binding receives the inferred type of its initial expression. Bindings are immutable by default; prefix a declaration with `mut` to permit reassignment and collection mutation. A reassignment must remain compatible with the binding's type. Explicit annotations are checked when the value is defined, reassigned, inserted into a collection, or passed to a typed function parameter.

`Int` and `Float` are both numeric. Arithmetic involving either float produces `Float`; otherwise it produces `Int`. Floating-point arithmetic rejects non-finite results (`NaN`, positive infinity, and negative infinity) as runtime arithmetic errors. Division or remainder by zero is always an error. `Unknown` is reserved for genuinely unavailable type information. It is not type-compatible with an unrelated concrete type at the top level; semantic checks defer only when an expression's actual type is unknown, leaving that check to the runtime. Nested unknown element information is deferred to runtime checks. Empty collections retain unknown element types because there is no value from which to infer one.

Arrays and lists are homogeneous. Tuple elements may have different types and tuple indexing with a known integer index is checked statically. Hash and tree keys are strings; inferred hash/tree literals retain a common value type for indexed reads and collection checks when their values agree. A heterogeneous map retains an unknown value type. Tree indexed writes are intentionally rejected, while Hash indexed writes require a mutable binding. Matrix dimensions and numeric contents are validated by the runtime operations.

`Vector[T]` is a statically element-typed view over an `Array[T]`, `List[T]`,
or homogeneous tuple; `Vector[T, N]` additionally fixes its length. It does
not introduce a separate runtime container. `Matrix[T]` specifies the cell
type for a matrix literal (`matrix [[...], ...]`) or rectangular nested
arrays/lists. `Matrix[T, R, C]` additionally fixes row and column counts.
Omit dimensions or use `?` for a dimension checked at runtime, for example
`Matrix[Float, ?, 3]`. `Matrix` without a type argument remains available for
dynamically typed code. Element types and known literal dimensions are checked
statically; dynamic values are checked at runtime. Operation dimension
compatibility is checked statically whenever both dimensions are known.

`range(...)` values have runtime type `Range`; `csv_rows(...)` values have
runtime type `CsvStream`. These are not aliases for arrays or lists. A Flow can
consume them as streaming sources; the checker's Flow analysis understands
their yielded item types.

Function parameters and declared return types are optional. A function without an explicit return annotation returns `Unit` when it reaches the end. A typed function must return a compatible value on every possible branch.

Matrix multiplication validates rectangular numeric matrices and returns a `Matrix` whose cells are `Float`, including when both inputs contain only integers. Empty matrices and incompatible dimensions are runtime errors.
`matvec(matrix, vector)` returns a Float array and requires the vector length
to match the matrix column count. `trace(matrix)` returns a `Float` for square
matrices. `rank(matrix)` returns an `Int`; its elimination uses a
scale-relative tolerance of `max(rows, columns) × epsilon × max_abs(matrix)`.
`determinant(matrix)` returns a `Float`; `inverse(matrix)` returns a
`Matrix` of `Float` cells. Both require a square matrix, and inverse rejects
singular matrices with a runtime arithmetic error. They use partial pivoting
and O(n³) time.
`lu(matrix)` returns `(L, U, P)` with `P × matrix = L × U`, using partial
pivoting; it accepts rectangular matrices. `qr(matrix)` returns `(Q, R)` with
square orthogonal `Q` and `matrix = Q × R`, including for rectangular input.
`cholesky(matrix)` returns lower-triangular `L` where `matrix = L × transpose(L)`;
the input must be symmetric positive definite.
`solve(matrix, right_hand_side)` returns a Float vector for a square,
non-singular system with one numeric right-hand-side value per row. A matrix
right-hand side returns a matrix with one solution column per input column.
Both forms use partial pivoting and do not construct an inverse.
`least_squares(matrix, right_hand_side)` returns a Float array for an
overdetermined system with full column rank. It uses Householder QR rather than
forming the less stable normal equations.
`cross(left, right)` returns a Float array and requires both vectors to contain
exactly three numeric elements.

Numerical built-ins accept `Int` and `Float` inputs. `dot` preserves `Int`
results when both vectors contain integers and uses checked integer arithmetic;
mixed or floating-point products use `Float`. Scalar transcendental functions,
vector norms/distances/normalization, and statistical functions return `Float`.
`sign` returns `Int`. Non-finite floating-point results are runtime errors.
Population variance and covariance divide by the number of observations.
Percentiles use linear interpolation on sorted values and accept inclusive
percent values from 0 to 100.

Vector functions accept non-empty arrays, lists, or tuples of numeric values.
Element-wise paired operations require equal lengths. Matrix functions accept
non-empty rectangular rows of numeric arrays or lists and reject ragged rows,
non-numeric cells, or incompatible dimensions. Matrix multiplication uses the
standard O(m × n × k) algorithm and returns floating-point cells; matrix
transpose and element-wise operations are O(rows × columns). Determinant and
inverse use partial pivoting in O(n³) time; inverse returns floating-point
cells. `solve` also uses partial pivoting in O(n³) time. LU, QR, and Cholesky
decompositions use O(n³) arithmetic for square matrices. `rank` uses
row-echelon elimination in O(m × n × min(m, n)) time; `matvec` is O(m × n).
`least_squares` uses Householder QR in O(m × n²) time for m rows and n columns.
Mean, variance,
standard deviation, covariance, and correlation are calculated in one pass;
median and percentile sort a copy of their input.
