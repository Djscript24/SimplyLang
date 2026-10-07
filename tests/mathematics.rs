mod common;
use common::*;

#[test]
fn runs_scalar_math_with_defined_float_results() {
    let (success, stdout) = run_source_stdout(
        "Sayln sqrt(9)\n\
         Sayln pow(2, 3)\n\
         Sayln log10(100)\n\
         Sayln floor(2.9)\n\
         Sayln ceil(-2.1)\n\
         Sayln sign(-4.5)\n\
         Sayln abs(-7)\n\
         Sayln round(1.25, 1)\n\
         Sayln clamp(5, 0, 4)\n",
    );
    assert!(success, "{stdout}");
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        ["3", "8", "2", "2", "-2", "-1", "7", "1.3", "4",]
    );
}

#[test]
fn rejects_invalid_math_domains_and_non_finite_results() {
    for (source, expected) in [
        ("Sayln sqrt(-1)\n", "non-negative"),
        ("Sayln log(0)\n", "positive"),
        ("Sayln pow(-2, 0.5)\n", "not finite"),
        ("Sayln exp(1000)\n", "not finite"),
    ] {
        let (success, error) = run_source(source);
        assert!(!success, "{source}");
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn runs_vector_operations_and_preserves_integer_dot_products() {
    let (success, stdout) = run_source_stdout(
        "a is [1, 2, 3]\n\
         b is [4, 5, 6]\n\
         Sayln vector_add(a, b)\n\
         Sayln vector_subtract(b, a)\n\
         Sayln vector_scale(a, 2)\n\
         Sayln dot(a, b)\n\
         Sayln norm([3, 4])\n\
         Sayln distance([1, 2], [4, 6])\n\
         Sayln normalize([3, 4])\n",
    );
    assert!(success, "{stdout}");
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        [
            "[5, 7, 9]",
            "[3, 3, 3]",
            "[2, 4, 6]",
            "32",
            "5",
            "5",
            "[0.6, 0.8]",
        ]
    );
}

#[test]
fn computes_three_dimensional_cross_products() {
    let (success, stdout) = run_source_stdout(
        "Sayln cross([1, 2, 3], [4, 5, 6])\n\
         Sayln cross([1, 0, 0], [0, 1, 0])\n",
    );
    assert!(success, "{stdout}");
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        ["[-3, 6, -3]", "[0, 0, 1]"]
    );

    let (success, error) = run_source("Sayln cross([1, 2], [3, 4])\n");
    assert!(!success);
    assert!(error.contains("exactly three elements"), "{error}");
}

#[test]
fn handles_zero_unit_negative_and_float_vectors() {
    let (success, stdout) = run_source_stdout(
        "Sayln norm([0, 0])\n\
         Sayln dot([1, 0], [1, 0])\n\
         Sayln vector_add([-1, 2], [1, -2])\n\
         Sayln dot([0.5, 1.5], [2.0, 2.0])\n\
         Sayln distance([0.0, 0.0], [3.0, 4.0])\n",
    );
    assert!(success, "{stdout}");
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        ["0", "1", "[0, 0]", "4", "5"]
    );
}

#[test]
fn rejects_invalid_vector_dimensions_and_values() {
    for (source, expected) in [
        ("Sayln dot([], [])\n", "vector must not be empty"),
        ("Sayln dot([1], [1, 2])\n", "dimensions do not match"),
        (
            "Sayln dot([9223372036854775807], [2])\n",
            "integer arithmetic error",
        ),
        (
            "Sayln norm([1, \"two\"])\n",
            "expected a finite numeric value",
        ),
        ("Sayln normalize([0, 0])\n", "zero vector"),
    ] {
        let (success, error) = run_source(source);
        assert!(!success, "{source}");
        assert!(error.contains(expected), "{error}");
    }

    let (success, error) = run_source("Sayln dot([1], [1, 2])\n");
    assert!(!success);
    assert!(
        error.contains("error[E.runtime.collection.operation]"),
        "{error}"
    );

    let (success, error) = run_source("Sayln norm([1, \"two\"])\n");
    assert!(!success);
    assert!(error.contains("error[E.runtime.type.mismatch]"), "{error}");
}

#[test]
fn runs_matrix_shape_transformations_and_products() {
    let (success, stdout) = run_source_stdout(
        "a is [[1, 2], [3, 4]]\n\
         b is [[5, 6], [7, 8]]\n\
         Sayln shape(a)\n\
         Sayln transpose(a)\n\
         Sayln matrix_add(a, b)\n\
         Sayln matrix_subtract(b, a)\n\
         Sayln matrix_scale(a, 2)\n\
         Sayln multiply(a, b)\n\
         Sayln multiply(a, identity(2))\n",
    );
    assert!(success, "{stdout}");
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        [
            "[2, 2]",
            "[[1, 3], [2, 4]]",
            "[[6, 8], [10, 12]]",
            "[[4, 4], [4, 4]]",
            "[[2, 4], [6, 8]]",
            "[[19, 22], [43, 50]]",
            "[[1, 2], [3, 4]]",
        ]
    );
}

#[test]
fn computes_matrix_determinants_and_inverses_with_pivoting() {
    let (success, stdout) = run_source_stdout(
        "a is [[2, 0], [0, 4]]\n\
         Sayln determinant(a)\n\
         Sayln inverse(a)\n\
         Sayln multiply(a, inverse(a))\n\
         Sayln determinant([[0, 1], [1, 0]])\n\
         Sayln inverse([[0, 1], [1, 0]])\n\
         Sayln determinant(identity(3))\n",
    );
    assert!(success, "{stdout}");
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        [
            "8",
            "[[0.5, 0], [0, 0.25]]",
            "[[1, 0], [0, 1]]",
            "-1",
            "[[0, 1], [1, 0]]",
            "1",
        ]
    );
}

#[test]
fn computes_matrix_trace_rank_and_matrix_vector_products() {
    let (success, stdout) = run_source_stdout(
        "Sayln trace([[1, 2], [3, 4]])\n\
         Sayln rank([[1, 2], [3, 4]])\n\
         Sayln rank([[1, 2], [2, 4], [3, 6]])\n\
         Sayln rank([[1, 0, 1], [0, 1, 1]])\n\
         Sayln matvec([[1, 2], [3, 4]], [1, 2])\n",
    );
    assert!(success, "{stdout}");
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        ["5", "2", "1", "2", "[5, 11]"]
    );

    let (success, error) = run_source("Sayln trace([[1, 2, 3], [4, 5, 6]])\n");
    assert!(!success);
    assert!(error.contains("trace requires a square matrix"), "{error}");

    let (success, error) = run_source("Sayln matvec([[1, 2], [3, 4]], [1])\n");
    assert!(!success);
    assert!(error.contains("dimensions do not match"), "{error}");
}

#[test]
fn solves_linear_systems_without_inverting_the_matrix() {
    let (success, stdout) = run_source_stdout(
        "Sayln solve([[3, 2], [1, 2]], [5, 5])\n\
         Sayln solve([[0, 2], [1, 3]], [4, 5])\n\
         Sayln solve([[2]], [6])\n",
    );
    assert!(success, "{stdout}");
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        ["[0, 2.5]", "[-1, 2]", "[3]"]
    );
}

#[test]
fn solves_multiple_right_hand_sides_in_one_call() {
    let (success, stdout) = run_source_stdout(
        "A is [[0.0, 2.0], [1.0, 3.0]]\n\
         B is [[4.0, 2.0], [5.0, 1.0]]\n\
         Sayln solve(A, B)\n\
         Sayln solve(A, [4.0, 5.0])\n",
    );
    assert!(success, "{stdout}");
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        ["[[-1, -2], [2, 1]]", "[-1, 2]"]
    );
}

#[test]
fn factors_matrices_with_lu_qr_and_cholesky() {
    let (success, stdout) = run_source_stdout(
        "(L, U, P) is lu([[0.0, 2.0], [1.0, 3.0]])\n\
         Sayln multiply(L, U)\n\
         Sayln multiply(P, [[0.0, 2.0], [1.0, 3.0]])\n\
         (Q, R) is qr([[1.0, 2.0], [3.0, 4.0]])\n\
         Sayln round(multiply(Q, R)[0][0], 4)\n\
         Sayln round(multiply(Q, R)[0][1], 4)\n\
         Sayln round(multiply(Q, R)[1][0], 4)\n\
         Sayln round(multiply(Q, R)[1][1], 4)\n\
         C is cholesky([[4.0, 2.0], [2.0, 3.0]])\n\
         Sayln round(C[0][0], 4)\n\
         Sayln round(C[1][0], 4)\n\
         Sayln round(C[1][1], 4)\n\
         reconstructed is multiply(C, transpose(C))\n\
         Sayln round(reconstructed[0][0], 4)\n\
         Sayln round(reconstructed[0][1], 4)\n\
         Sayln round(reconstructed[1][0], 4)\n\
         Sayln round(reconstructed[1][1], 4)\n",
    );
    assert!(success, "{stdout}");
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        [
            "[[1, 3], [0, 2]]",
            "[[1, 3], [0, 2]]",
            "1",
            "2",
            "3",
            "4",
            "2",
            "1",
            "1.4142",
            "4",
            "2",
            "2",
            "3",
        ]
    );
}

#[test]
fn decompositions_preserve_rectangular_matrix_shapes() {
    let (success, stdout) = run_source_stdout(
        "(L, U, P) is lu([[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]])\n\
         Sayln multiply(P, [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]])\n\
         Sayln multiply(L, U)\n\
         (Q, R) is qr([[1.0, 0.0], [0.0, 1.0], [1.0, 1.0]])\n\
         reconstructed is multiply(Q, R)\n\
         Sayln shape(Q)[0]\n\
         Sayln shape(R)[0]\n\
         Sayln shape(R)[1]\n\
         Sayln round(reconstructed[2][0], 4)\n\
         Sayln round(reconstructed[2][1], 4)\n",
    );
    assert!(success, "{stdout}");
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        [
            "[[4, 5, 6], [1, 2, 3]]",
            "[[4, 5, 6], [1, 2, 3]]",
            "3",
            "3",
            "2",
            "1",
            "1",
        ]
    );
}

#[test]
fn cholesky_rejects_non_positive_definite_or_nonsymmetric_inputs() {
    for source in [
        "Sayln cholesky([[1.0, 2.0], [2.0, 1.0]])\n",
        "Sayln cholesky([[1.0, 2.0], [0.0, 1.0]])\n",
    ] {
        let (success, error) = run_source(source);
        assert!(!success, "{source}");
        assert!(
            error.contains("symmetric positive-definite matrix"),
            "{error}"
        );
        assert!(
            error.contains("error[E.runtime.numeric.arithmetic-invalid]"),
            "{error}"
        );
    }
}

#[test]
fn computes_least_squares_with_householder_qr() {
    let (success, stdout) = run_source_stdout(
        "fit_exact is least_squares([[1, 0], [0, 1], [1, 1]], [2, 3, 5])\n\
         Sayln round(fit_exact[0], 4)\n\
         Sayln round(fit_exact[1], 4)\n\
         fit is least_squares([[1, 0], [1, 1], [1, 2]], [1, 2, 2])\n\
         Sayln round(fit[0], 4)\n\
         Sayln round(fit[1], 4)\n",
    );
    assert!(success, "{stdout}");
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        ["2", "3", "1.1667", "0.5"]
    );
}

#[test]
fn rejects_least_squares_systems_without_a_unique_solution() {
    let (success, error) = run_source("Sayln least_squares([[1, 0], [0, 1]], [1, 2, 3])\n");
    assert!(!success);
    assert!(
        error.contains("one right-hand-side value per matrix row"),
        "{error}"
    );
    assert!(
        error.contains("error[E.runtime.collection.operation]"),
        "{error}"
    );

    let (success, error) = run_source("Sayln least_squares([[1, 1], [2, 2], [3, 3]], [1, 2, 3])\n");
    assert!(!success);
    assert!(error.contains("full column rank"), "{error}");
    assert!(
        error.contains("error[E.runtime.numeric.arithmetic-invalid]"),
        "{error}"
    );

    let (success, error) = run_source("Sayln least_squares([[1, 2, 3], [4, 5, 6]], [1, 2])\n");
    assert!(!success);
    assert!(
        error.contains("at least as many rows as columns"),
        "{error}"
    );
}

#[test]
fn rejects_invalid_linear_systems() {
    let (success, error) = run_source("Sayln solve([[1, 2, 3], [4, 5, 6]], [1, 2])\n");
    assert!(!success);
    assert!(error.contains("solve requires a square matrix"), "{error}");
    assert!(
        error.contains("error[E.runtime.collection.operation]"),
        "{error}"
    );

    let (success, error) = run_source("Sayln solve([[1, 2], [3, 4]], [1])\n");
    assert!(!success);
    assert!(
        error.contains("one right-hand-side value per matrix row"),
        "{error}"
    );

    let (success, error) = run_source("Sayln solve([[1, 2], [2, 4]], [3, 6])\n");
    assert!(!success);
    assert!(error.contains("no unique solution"), "{error}");
    assert!(
        error.contains("error[E.runtime.numeric.arithmetic-invalid]"),
        "{error}"
    );
}

#[test]
fn rejects_non_square_and_singular_matrix_determinant_operations() {
    let (success, error) = run_source("Sayln determinant([[1, 2, 3], [4, 5, 6]])\n");
    assert!(!success);
    assert!(
        error.contains("determinant requires a square matrix"),
        "{error}"
    );
    assert!(
        error.contains("error[E.runtime.collection.operation]"),
        "{error}"
    );

    let (success, error) = run_source("Sayln inverse([[1, 2, 3], [4, 5, 6]])\n");
    assert!(!success);
    assert!(
        error.contains("inverse requires a square matrix"),
        "{error}"
    );
    assert!(
        error.contains("error[E.runtime.collection.operation]"),
        "{error}"
    );

    let (success, error) = run_source("Sayln inverse([[1, 2], [2, 4]])\n");
    assert!(!success);
    assert!(error.contains("matrix is singular"), "{error}");
    assert!(
        error.contains("error[E.runtime.numeric.arithmetic-invalid]"),
        "{error}"
    );
}

#[test]
fn handles_single_cell_rectangular_float_and_zero_matrices() {
    let (success, stdout) = run_source_stdout(
        "Sayln multiply([[3]], [[4]])\n\
         Sayln multiply([[1, 2, 3], [4, 5, 6]], [[1], [0], [-1]])\n\
         Sayln matrix_add([[1.5, -2.0]], [[0.5, 2.0]])\n\
         Sayln multiply([[0, 0], [0, 0]], identity(2))\n",
    );
    assert!(success, "{stdout}");
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        ["[[12]]", "[[-2], [-2]]", "[[2, 0]]", "[[0, 0], [0, 0]]"]
    );
}

#[test]
fn rejects_invalid_matrix_shapes_and_dimensions() {
    for (source, expected) in [
        ("Sayln shape([])\n", "matrix must not be empty"),
        ("Sayln shape([[1, 2], [3]])\n", "equal widths"),
        (
            "Sayln multiply([[1, 2]], [[1, 2]])\n",
            "dimensions do not match",
        ),
        (
            "Sayln matrix_add([[1, \"two\"]], [[1, 2]])\n",
            "finite numeric value",
        ),
        ("Sayln identity(0)\n", "positive integer"),
        ("Sayln identity(1000000000)\n", "too large to allocate"),
    ] {
        let (success, error) = run_source(source);
        assert!(!success, "{source}");
        assert!(error.contains(expected), "{error}");
    }

    let (success, error) = run_source("Sayln identity(1000000000)\n");
    assert!(!success);
    assert!(
        error.contains("identity matrix is too large to allocate"),
        "{error}"
    );
    assert!(error.contains("error[E.runtime.limit.exceeded]"), "{error}");

    let (success, error) = run_source("Sayln multiply([[1, 2]], [[1, 2]])\n");
    assert!(!success);
    assert!(
        error.contains("error[E.runtime.collection.operation]"),
        "{error}"
    );
}

#[test]
fn computes_population_statistics_and_linear_percentiles() {
    let (success, stdout) = run_source_stdout(
        "values is [1, 2, 3, 4]\n\
         Sayln mean(values)\n\
         Sayln median(values)\n\
         Sayln variance(values)\n\
         Sayln stddev(values)\n\
         Sayln percentile(values, 25)\n\
         Sayln covariance([1, 2, 3], [2, 4, 6])\n\
         Sayln correlation([1, 2, 3], [2, 4, 6])\n\
         Sayln mean(range(1, 4))\n\
         mean_value is mean(values)\n\
         average_value is pipeline:\n\
             values\n\
             average\n\
         end\n\
         Sayln mean_value\n\
         Sayln average_value\n",
    );
    assert!(success, "{stdout}");
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        [
            "2.5",
            "2.5",
            "1.25",
            "1.118033988749895",
            "1.75",
            "1.3333333333333333",
            "1",
            "2",
            "2.5",
            "2.5",
        ]
    );
}

#[test]
fn rejects_invalid_statistical_inputs() {
    for (source, expected) in [
        ("Sayln mean([])\n", "at least one"),
        ("Sayln percentile([1, 2], 101)\n", "between 0 and 100"),
        ("Sayln covariance([1, 2], [1])\n", "lengths do not match"),
        ("Sayln correlation([1, 1], [2, 3])\n", "non-constant"),
        ("Sayln mean([1, \"two\"])\n", "finite numeric value"),
    ] {
        let (success, error) = run_source(source);
        assert!(!success, "{source}");
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn checks_math_argument_types_before_execution() {
    for (source, expected) in [
        ("Sayln sqrt(\"nine\")\n", "expected a number"),
        (
            "Sayln determinant([1, 2])\n",
            "matrix rows must be numeric sequences",
        ),
        ("Sayln inverse(\"matrix\")\n", "expected a matrix"),
        (
            "Sayln solve([[1, 0], [0, 1]], \"vector\")\n",
            "expected a numeric sequence",
        ),
        (
            "Sayln least_squares([[1, 0], [0, 1]], \"vector\")\n",
            "expected a numeric sequence",
        ),
        ("Sayln vector_add([1], [\"two\"])\n", "expected a number"),
        (
            "Sayln multiply([1, 2], [3, 4])\n",
            "matrix rows must be numeric sequences",
        ),
        ("Sayln vector_scale(2, 3)\n", "expected a numeric sequence"),
        (
            "Sayln cross([1], \"vector\")\n",
            "expected a numeric sequence",
        ),
        (
            "Sayln matvec([[1]], \"vector\")\n",
            "expected a numeric sequence",
        ),
    ] {
        let (success, _, error) = check_source(source);
        assert!(!success, "{source}");
        assert!(error.contains(expected), "{error}");
    }

    let (success, _, error) = check_source(
        "fn determinant_result() gives Float:\n\
             return determinant([[1, 2], [3, 4]])\n\
         end\n\
         fn inverse_result() gives Matrix:\n\
             return inverse([[1, 0], [0, 1]])\n\
         end\n",
    );
    assert!(success, "{error}");

    let (success, _, error) = check_source(
        "fn trace_result() gives Float:\n\
             return trace([[1, 0], [0, 1]])\n\
         end\n\
         fn rank_result() gives Int:\n\
             return rank([[1, 0], [0, 1]])\n\
         end\n\
         fn matvec_result() gives Array[Float]:\n\
             return matvec([[1, 0], [0, 1]], [1, 2])\n\
         end\n\
         fn cross_result() gives Array[Float]:\n\
             return cross([1, 0, 0], [0, 1, 0])\n\
         end\n",
    );
    assert!(success, "{error}");

    let (success, _, error) = check_source(
        "fn fit(ref values as Array[Float]) gives Array[Float]:\n\
             return least_squares([[1, 0], [0, 1], [1, 1]], values)\n\
         end\n",
    );
    assert!(success, "{error}");
}

#[test]
fn checks_vector_and_matrix_element_types_statically() {
    let source = "mat as Matrix[Float] is [[1.0, 2.0], [3.0, 4.0]]\n\
         vec as Vector[Float] is [5.0, 6.0]\n\
         fn transform(m as Matrix[Float], ref v as Vector[Float]) gives Array[Float]:\n\
             return matvec(m, v)\n\
         end\n\
         Sayln transform(mat, ref vec)\n\
         Sayln mat[0, 1]\n\
         Sayln mat + mat\n\
         Sayln vec[1]\n";
    let (checked, _, check_error) = check_source(source);
    assert!(checked, "{check_error}");
    let (success, stdout) = run_source_stdout(source);
    assert!(success, "{}", run_source(source).1);
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        ["[17, 39]", "2", "[[2, 4], [6, 8]]", "6"]
    );

    for (source, expected) in [
        (
            "mat as Matrix[Float] is [[1]]\n",
            "expected Matrix[Float], found Vector[Vector[Int, 1], 1]",
        ),
        (
            "vector as Vector[String] is [1, 2]\n",
            "expected Vector[String], found Vector[Int, 2]",
        ),
        (
            "fn invalid(ref values as Vector[String]) gives Float:\n\
                 return norm(values)\n\
             end\n",
            "expected a number, found String",
        ),
    ] {
        let (success, _, error) = check_source(source);
        assert!(!success, "{source}");
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn checks_decomposition_and_multi_rhs_solve_dimensions_statically() {
    let valid = "A as Matrix[Float, 2, 2] is [[3.0, 2.0], [1.0, 2.0]]\n\
         B as Matrix[Float, 2, 3] is [[5.0, 2.0, 1.0], [5.0, 1.0, 4.0]]\n\
         X as Matrix[Float, 2, 3] is solve(A, B)\n\
         (L, U, P) is lu(A)\n\
         (Q, R) is qr(A)\n\
         C as Matrix[Float, 2, 2] is cholesky([[4.0, 2.0], [2.0, 3.0]])\n";
    let (success, _, error) = check_source(valid);
    assert!(success, "{error}");

    let (success, _, error) = check_source(
        "A as Matrix[Float, 2, 2] is [[1.0, 0.0], [0.0, 1.0]]\n\
         B as Matrix[Float, 3, 2] is [[1.0, 0.0], [0.0, 1.0], [1.0, 1.0]]\n\
         solve(A, B)\n",
    );
    assert!(!success);
    assert!(
        error.contains("`solve` requires the right-hand-side row count to match the matrix"),
        "{error}"
    );
}

#[test]
fn checks_vector_and_matrix_dimensions_statically() {
    let (success, _, error) = check_source("vec as Vector[Float, 3] is [1.0, 2.0]\n");
    assert!(!success);
    assert!(error.contains("expected Vector[Float, 3]"), "{error}");

    let (success, _, error) =
        check_source("mat as Matrix[Float, 2, 2] is [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]\n");
    assert!(!success);
    assert!(error.contains("expected Matrix[Float, 2, 2]"), "{error}");

    for (source, expected) in [
        (
            "mat as Matrix[Float, 2, 3] is [[1.0, 2.0], [3.0, 4.0]]\n",
            "expected Matrix[Float, 2, 3]",
        ),
        (
            "mat as Matrix[Float, 2, 2] is [[1.0, 2.0], [3.0]]\n",
            "equal widths",
        ),
        (
            "a as Matrix[Float, 2, 3] is [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]\n\
         b as Matrix[Float, 4, 2] is [[1.0, 2.0], [3.0, 4.0], [5.0, 6.0], [7.0, 8.0]]\n\
         Sayln multiply(a, b)\n",
            "multiplication dimensions do not match",
        ),
        (
            "a as Matrix[Float, 2, 3] is [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]\n\
         x as Vector[Float, 2] is [1.0, 2.0]\n\
         Sayln matvec(a, x)\n",
            "matching vector dimensions",
        ),
        (
            "Sayln trace([[1, 2, 3], [4, 5, 6]])\n",
            "requires a square matrix",
        ),
    ] {
        let (success, _, error) = check_source(source);
        assert!(!success, "{source}");
        assert!(error.contains(expected), "{error}");
    }

    let source = "a as Matrix[Float, ?, 3] is [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]\n\
         v as Vector[Float, ?] is [1.0, 2.0, 3.0]\n\
         Sayln matvec(a, v)\n";
    let (success, _, error) = check_source(source);
    assert!(success, "{error}");

    let source = "dynamic as Vector[Float] is [1.0, 2.0]\n\
         fn fixed(v as Vector[Float, 3]) gives Float:\n\
             return norm(v)\n\
         end\n\
         Sayln fixed(dynamic)\n";
    let (success, error) = run_source(source);
    assert!(!success);
    assert!(error.contains("expected Vector[Float, 3]"), "{error}");
}

#[test]
fn runs_vector_matrix_statistics_and_ml_examples() {
    for path in [
        "examples/12-mathematics/vector.si",
        "examples/12-mathematics/matrix.si",
        "examples/12-mathematics/statistics.si",
        "examples/12-mathematics/gradient-step.si",
    ] {
        run_example(path);
    }
}

#[test]
fn checks_and_runs_matrix_workload_example() {
    let source = include_str!("../examples/99-bench/matrix-workload.si");
    let (valid, _, error) = check_source(source);
    assert!(valid, "matrix-workload example failed checking: {error}");

    let output = run_example("examples/99-bench/matrix-workload.si");
    let result = output
        .trim()
        .parse::<f64>()
        .expect("matrix-workload output should be numeric");
    assert!(result.is_finite(), "matrix-workload result was not finite");
}
