use std::{path::Path, process::Command};

fn run_fixture(command: &str, fixture: &str) -> std::process::Output {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    Command::new(env!("CARGO_BIN_EXE_simply"))
        .arg(command)
        .arg(root.join("tests/fixtures/nominal-identity").join(fixture))
        .current_dir(root)
        .output()
        .expect("failed to run Simply fixture")
}

#[test]
fn same_named_structs_from_distinct_modules_are_incompatible() {
    let output = run_fixture("check", "check-struct-identity.si");
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{diagnostic}");
    assert!(
        diagnostic.contains("expected Item, found Item"),
        "{diagnostic}"
    );
    assert!(!diagnostic.contains("memory://"), "{diagnostic}");
}

#[test]
fn same_named_enums_from_distinct_modules_are_incompatible() {
    let output = run_fixture("check", "check-enum-identity.si");
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{diagnostic}");
    assert!(
        diagnostic.contains("expected State, found State"),
        "{diagnostic}"
    );
    assert!(!diagnostic.contains("memory://"), "{diagnostic}");
}

#[test]
fn module_returned_values_retain_identity_for_struct_and_enum_patterns() {
    for (fixture, expected_pattern_error, expected_output) in [
        (
            "struct-pattern.si",
            "struct pattern `Item` cannot match value of type Item",
            "foreign\n",
        ),
        (
            "enum-pattern.si",
            "pattern `State::Ready` cannot match value of type State",
            "foreign\n",
        ),
    ] {
        let check = run_fixture("check", fixture);
        let diagnostic = String::from_utf8_lossy(&check.stderr);
        assert!(!check.status.success(), "{diagnostic}");
        assert!(diagnostic.contains(expected_pattern_error), "{diagnostic}");

        let run = run_fixture("run", fixture);
        assert!(
            run.status.success(),
            "{}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&run.stdout), expected_output);
    }
}

#[test]
fn message_dispatch_does_not_cross_dispatch_same_named_structs() {
    let output = run_fixture("run", "message-dispatch.si");
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{diagnostic}");
    assert!(
        diagnostic.contains("message `kind` is not understood by `Item`"),
        "{diagnostic}"
    );
    assert!(!diagnostic.contains("memory://"), "{diagnostic}");
}

#[test]
fn equal_payloads_from_distinct_modules_are_not_equal_values() {
    let output = run_fixture("run", "module-value-equality.si");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "false\nfalse\n");
}

#[test]
fn values_from_different_aliases_of_one_module_remain_equal() {
    let output = run_fixture("run", "alias-value-equality.si");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "true\ntrue\n");
}

#[test]
fn values_declared_in_one_module_remain_compatible() {
    let output = run_fixture("run", "same-module.si");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Item\nState\ntrue\ntrue\n"
    );
}
