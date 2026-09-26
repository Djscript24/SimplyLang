mod common;
use common::*;

#[test]
fn runs_scalar_math_with_defined_float_results() {
    let (success, stdout) = run_source_stdout(
        "Say sqrt(9)\n\
         Say pow(2, 3)\n\
         Say log10(100)\n\
         Say floor(2.9)\n\
         Say ceil(-2.1)\n\
         Say sign(-4.5)\n\
         Say abs(-7)\n\
         Say round(1.25, 1)\n\
         Say clamp(5, 0, 4)\n",
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
        ("Say sqrt(-1)\n", "non-negative"),
        ("Say log(0)\n", "positive"),
        ("Say pow(-2, 0.5)\n", "not finite"),
        ("Say exp(1000)\n", "not finite"),
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
         Say vector_add(a, b)\n\
         Say vector_subtract(b, a)\n\
         Say vector_scale(a, 2)\n\
         Say dot(a, b)\n\
         Say norm([3, 4])\n\
         Say distance([1, 2], [4, 6])\n\
         Say normalize([3, 4])\n",
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
        "Say norm([0, 0])\n\
         Say dot([1, 0], [1, 0])\n\
         Say vector_add([-1, 2], [1, -2])\n\
         Say dot([0.5, 1.5], [2.0, 2.0])\n\
         Say distance([0.0, 0.0], [3.0, 4.0])\n",
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
        ("Say dot([], [])\n", "vector must not be empty"),
        ("Say dot([1], [1, 2])\n", "dimensions do not match"),
        (
            "Say dot([9223372036854775807], [2])\n",
            "integer arithmetic error",
        ),
        (
            "Say norm([1, \"two\"])\n",
            "expected a finite numeric value",
        ),
        ("Say normalize([0, 0])\n", "zero vector"),
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
         Say shape(a)\n\
         Say transpose(a)\n\
         Say matrix_add(a, b)\n\
         Say matrix_subtract(b, a)\n\
         Say matrix_scale(a, 2)\n\
         Say multiply(a, b)\n\
         Say multiply(a, identity(2))\n",
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
        "Say multiply([[3]], [[4]])\n\
         Say multiply([[1, 2, 3], [4, 5, 6]], [[1], [0], [-1]])\n\
         Say matrix_add([[1.5, -2.0]], [[0.5, 2.0]])\n\
         Say multiply([[0, 0], [0, 0]], identity(2))\n",
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
        ("Say shape([])\n", "matrix must not be empty"),
        ("Say shape([[1, 2], [3]])\n", "equal widths"),
        (
            "Say multiply([[1, 2]], [[1, 2]])\n",
            "dimensions do not match",
        ),
        (
            "Say matrix_add([[1, \"two\"]], [[1, 2]])\n",
            "finite numeric value",
        ),
        ("Say identity(0)\n", "positive integer"),
        ("Say identity(1000000000)\n", "too large to allocate"),
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
         Say mean(values)\n\
         Say median(values)\n\
         Say variance(values)\n\
         Say stddev(values)\n\
         Say percentile(values, 25)\n\
         Say covariance([1, 2, 3], [2, 4, 6])\n\
         Say correlation([1, 2, 3], [2, 4, 6])\n\
         Say mean(range(1, 4))\n",
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
        ("Say mean([])\n", "at least one"),
        ("Say percentile([1, 2], 101)\n", "between 0 and 100"),
        ("Say covariance([1, 2], [1])\n", "lengths do not match"),
        ("Say correlation([1, 1], [2, 3])\n", "non-constant"),
        ("Say mean([1, \"two\"])\n", "finite numeric value"),
    ] {
        let (success, error) = run_source(source);
        assert!(!success, "{source}");
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn checks_math_argument_types_before_execution() {
    for (source, expected) in [
        ("Say sqrt(\"nine\")\n", "expected a number"),
        ("Say vector_add([1], [\"two\"])\n", "expected a number"),
        (
            "Say multiply([1, 2], [3, 4])\n",
            "matrix rows must be numeric sequences",
        ),
        ("Say vector_scale(2, 3)\n", "expected a numeric sequence"),
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
