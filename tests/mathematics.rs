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
         Sayln mean(range(1, 4))\n",
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
        ("Sayln vector_add([1], [\"two\"])\n", "expected a number"),
        (
            "Sayln multiply([1, 2], [3, 4])\n",
            "matrix rows must be numeric sequences",
        ),
        ("Sayln vector_scale(2, 3)\n", "expected a numeric sequence"),
    ] {
        let (success, _, error) = check_source(source);
        assert!(!success, "{source}");
        assert!(error.contains(expected), "{error}");
    }
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
