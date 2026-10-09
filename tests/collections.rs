mod common;
use common::*;

#[test]
fn enforces_float_and_matrix_runtime_contracts() {
    let (success, float_error) = run_source("Sayln 1e308 * 1e308\n");
    assert!(!success);
    assert!(float_error.contains("floating-point result is not finite"));

    let (success, matrix_output) = run_source_stdout(
        "left is matrix [[1, 2], [3, 4]]\nright is matrix [[5, 6], [7, 8]]\nSayln left multiply right\n",
    );
    assert!(success);
    assert!(matrix_output.contains("19"));
    assert!(matrix_output.contains("50"));
}

#[test]
fn collection_aliases_observe_mutation_through_mutable_bindings() {
    let (success, stdout) = run_source_stdout(
        "values is list [1]\nmut copy is values\ncopy add 2\nSayln values\nSayln copy\n",
    );
    assert!(success);
    assert_eq!(stdout.lines().collect::<Vec<_>>(), ["[1, 2]", "[1, 2]"]);

    let (success, stdout) = run_source_stdout(
        "profile is hash:\n    name is \"Ada\"\nend\nmut copy is profile\ncopy[\"role\"] -> \"builder\"\nSayln profile\nSayln copy\n",
    );
    assert!(success);
    assert!(stdout.lines().all(|line| line.contains("role")));
}

#[test]
fn ref_markers_are_rejected_outside_function_parameter_declarations() {
    for source in [
        "alias -> ref values\n",
        "enum Result:\n    Some as List[Int]\nend\nResult::Some(ref values)\n",
        "on List receive inspect(ref value as List[Int]):\n    return 1\nend\n",
        "values :: inspect(ref other)\n",
        "fn first(ref values as List[Int]) gives Int:\n    return values[0]\nend\nitems is list [1]\nfirst(ref items)\n",
    ] {
        let (valid, _, error) = check_source(source);
        assert!(!valid, "non-function use accepted a `ref` marker");
        assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");
    }
}

#[test]
fn nested_collection_mutation_updates_all_aliases() {
    let source = r#"record is hash:
    details is hash:
        scores is list [10, 20]
    end
end
mut copy is record
copy["details"]["scores"][0] -> 99
Sayln record["details"]["scores"][0]
Sayln copy["details"]["scores"][0]
"#;
    let (success, output) = run_source_stdout(source);
    assert!(success, "nested collection mutation failed: {output}");
    assert_eq!(output, "99\n99\n");
}

#[test]
fn ref_collection_parameters_are_read_only_and_non_escaping() {
    let source = r#"fn first(ref values as List[Int]) gives Int:
    return values[0]
end
items is list [42]
Sayln first(items)
profile is hash:
    scores is list [17, 23]
end
Sayln first(profile["scores"])
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "read-only ref call or nested view failed: {output}"
    );
    assert_eq!(output, "42\n17\n");

    let (valid, _, error) =
        check_source("fn first(value as Int) gives Int:\n    return value\nend\nfirst(ref 1)\n");
    assert!(!valid, "a redundant call-site ref marker was accepted");
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");

    let (valid, _, error) =
        check_source("fn first(values as List[Int]) gives Int:\n    return values[0]\nend\n");
    assert!(!valid, "a collection parameter without `ref` was accepted");
    assert!(error.contains("must be declared `ref`"), "{error}");

    let (valid, _, error) = check_source(
        "fn first(values) gives Int:\n    return 0\nend\nitems is list [1]\nfirst(items)\n",
    );
    assert!(!valid, "an untyped parameter accepted a collection value");
    assert!(error.contains("requires a `ref` parameter"), "{error}");

    let (valid, _, error) =
        check_source("fn update(ref values as List[Int]) gives Unit:\n    values[0] -> 7\nend\n");
    assert!(!valid, "a ref parameter was mutable");
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");
    assert!(error.contains("read-only and cannot be mutated"), "{error}");

    for source in [
        "fn escape(ref values as List[Int]) gives List[Int]:\n    return values\nend\n",
        "fn escape(ref values as List[Int]) gives Int:\n    copy is values\n    return 0\nend\n",
        "fn escape(ref values as List[Int]) gives Int:\n    fn nested() gives Int:\n        return values[0]\n    end\n    return 0\nend\n",
        "fn escape(ref values as List[List[Int]]) gives List[Int]:\n    for item in values:\n        return item\n    end\n    return [0]\nend\n",
    ] {
        let (valid, _, error) = check_source(source);
        assert!(!valid, "a ref parameter escaped its call");
        assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");
        assert!(
            error.contains("ref parameter") || error.contains("ref` parameter"),
            "{error}"
        );
    }

    let (valid, _, error) = check_source(
        "mut stash is list []\nfn leak(ref values as List[Int]) gives Unit:\n    stash add values\nend\n",
    );
    assert!(!valid, "a ref parameter escaped through list add");
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");
    assert!(error.contains("ref` parameter"), "{error}");
}

#[test]
fn ref_parameters_allow_nested_functions_that_do_not_capture_them() {
    let source = r#"fn first_plus_one(ref values as List[Int]) gives Int:
    fn increment(value as Int) gives Int:
        return value + 1
    end
    return increment(values[0])
end
items is list [41]
Sayln first_plus_one(items)
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "non-capturing nested function was rejected: {output}"
    );
    assert_eq!(output, "42\n");
}

#[test]
fn ref_parameters_can_be_forwarded_between_functions() {
    let source = r#"fn first(ref values as Vector[Int]) gives Int:
    return values[0]
end
fn forward(ref input as Vector[Int]) gives Int:
    return first(input)
end
numbers as Vector[Int] is [42, 99]
Sayln forward(numbers)
"#;
    let (success, output) = run_source_stdout(source);
    assert!(success, "forwarding a ref parameter failed: {output}");
    assert_eq!(output, "42\n");
}

#[test]
fn mutable_ref_borrows_exclusively_and_forwards_without_copying() {
    let source = r#"fn update(mut ref values as List[Int]):
    values[1] -> 99
end
fn forward(mut ref items as List[Int]):
    update(items)
end
mut numbers is list [10, 20, 30]
forward(numbers)
Sayln numbers
"#;
    let (success, output) = run_source_stdout(source);
    assert!(success, "exclusive ref forwarding failed: {output}");
    assert_eq!(output, "[10, 99, 30]\n");

    let source = r#"fn update(mut ref values as List[Int]):
    values[0] -> 7
end
mut left is list [1]
mut right is list [2]
update(left)
update(right)
Sayln left
Sayln right
"#;
    let (success, output) = run_source_stdout(source);
    assert!(success, "independent exclusive borrows failed: {output}");
    assert_eq!(output, "[7]\n[7]\n");

    let source = r#"fn update_both(mut ref left as List[Int], mut ref right as List[Int]):
    left[0] -> 7
    right[0] -> 8
end
mut left is list [1]
mut right is list [2]
update_both(left, right)
Sayln left
Sayln right
"#;
    let (success, output) = run_source_stdout(source);
    assert!(success, "independent simultaneous borrows failed: {output}");
    assert_eq!(output, "[7]\n[8]\n");
}

#[test]
fn mutable_ref_requires_a_mutable_owner_and_rejects_conflicting_aliases() {
    let (valid, _, error) = check_source(
        "fn update(mut ref values as List[Int]):\n    values[0] -> 1\nend\nvalues is list [0]\nupdate(values)\n",
    );
    assert!(!valid, "immutable owner was accepted for a mutable ref");
    assert!(error.contains("requires a `mut` owner"), "{error}");

    let (valid, _, error) = check_source(
        "fn swap(mut ref values as List[Int]):\n    mut other is list [2]\n    (values, other) -> (other, values)\nend\nmut values is list [1]\nswap(values)\n",
    );
    assert!(
        !valid,
        "mutable ref parameter was reassigned by destructuring"
    );
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");

    let (success, error) = run_source(
        "fn update(mut ref left as List[Int], mut ref right as List[Int]):\n    left[0] -> 9\nend\nmut values is list [1]\nmut alias is values\nupdate(values, alias)\n",
    );
    assert!(!success, "two mutable aliases were accepted");
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");

    let (success, error) = run_source(
        "fn conflict(ref read as List[Int], mut ref write as List[Int]):\n    write[0] -> 9\nend\nmut values is list [1]\nconflict(values, values)\n",
    );
    assert!(
        !success,
        "overlapping shared and exclusive borrows were accepted"
    );
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");
}

#[test]
fn shared_ref_blocks_mutation_through_another_alias_until_the_call_ends() {
    let source = r#"mut values is list [1]
mut alias is values
fn mutate_global():
    alias[0] -> 9
end
fn inspect(ref borrowed as List[Int]):
    mutate_global()
end
inspect(values)
"#;
    let (success, error) = run_source(source);
    assert!(!success, "mutation through a shared alias was allowed");
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");

    let source = r#"mut values is list [1]
mut alias is values
fn inspect(ref borrowed as List[Int]) gives Int:
    return borrowed[0]
end
inspect(values)
alias[0] -> 2
Sayln values
"#;
    let (success, output) = run_source_stdout(source);
    assert!(success, "borrow was not released after its call: {output}");
    assert_eq!(output, "[2]\n");
}

#[test]
fn shared_ref_borrows_can_overlap_and_release_after_error() {
    let source = r#"fn combine(ref left as List[Int], ref right as List[Int]) gives Int:
    return left[0] + right[0]
end
values is list [20]
Sayln combine(values, values)
"#;
    let (success, output) = run_source_stdout(source);
    assert!(success, "overlapping shared borrows failed: {output}");
    assert_eq!(output, "40\n");

    let source = r#"mut values is list [20]
mut alias is values
fn combine(ref left as List[Int], ref right as List[Int]) gives Int:
    return left[0] + right[0]
end
fn forward(ref borrowed as List[Int]) gives Int:
    return combine(borrowed, alias)
end
Sayln forward(values)
"#;
    let (success, output) = run_source_stdout(source);
    assert!(success, "nested shared alias borrow failed: {output}");
    assert_eq!(output, "40\n");

    let source = r#"enum Fault:
    Failed
end
mut values is list [1]
mut alias is values
fn fail(ref borrowed as List[Int]):
    throw Fault::Failed
end
try:
    fail(values)
catch error:
    alias[0] -> 2
end
Sayln values
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "borrow was not released after a thrown error: {output}"
    );
    assert_eq!(output, "[2]\n");
}

#[test]
fn mutable_ref_can_be_inferred_from_a_message_receiver() {
    let source = r#"fn update(mut ref values as List[Int]):
    values[0] -> 8
end
mut numbers is list [1]
numbers :: update
Sayln numbers
"#;
    let (success, output) = run_source_stdout(source);
    assert!(success, "mutable ref message receiver failed: {output}");
    assert_eq!(output, "[8]\n");
}

#[test]
fn message_receivers_enforce_borrow_modes_aliasing_forwarding_and_cleanup() {
    let source = r#"fn first(ref values as List[Int]) gives Int:
    return values[0]
end
fn update(mut ref values as List[Int]):
    values[0] -> 9
end
mut numbers is list [1]
mut alias is numbers
fn read_while_exclusive(mut ref values as List[Int]):
    return alias :: first
end
read_while_exclusive(numbers)
"#;
    let (success, error) = run_source(source);
    assert!(
        !success,
        "read-only message ignored an aliased exclusive borrow"
    );
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");

    let source = r#"fn update(mut ref values as List[Int]):
    values[0] -> 9
end
mut numbers is list [1]
mut alias is numbers
fn mutate_while_shared(ref values as List[Int]):
    alias :: update
end
mutate_while_shared(numbers)
"#;
    let (success, error) = run_source(source);
    assert!(
        !success,
        "mutating message ignored an aliased shared borrow"
    );
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");

    let source = r#"fn first(ref values as List[Int]) gives Int:
    return values[0]
end
fn forward(ref values as List[Int]) gives Int:
    return values :: first
end
numbers is list [42]
Sayln numbers :: forward
"#;
    let (success, output) = run_source_stdout(source);
    assert!(success, "shared message forwarding failed: {output}");
    assert_eq!(output, "42\n");

    let source = r#"enum Fault:
    Failed
end
fn fail(ref values as List[Int]):
    throw Fault::Failed
end
mut numbers is list [1]
try:
    numbers :: fail
catch error:
    numbers[0] -> 2
end
Sayln numbers
"#;
    let (success, output) = run_source_stdout(source);
    assert!(success, "message error left a borrow active: {output}");
    assert_eq!(output, "[2]\n");
}

#[test]
fn mutable_ref_updates_array_and_hash_storage_in_place() {
    let source = r#"fn update_array(mut ref values as Array[Int]):
    values[0] -> 5
end
fn update_hash(mut ref values as Hash):
    values["score"] -> 8
end
mut numbers is [1, 2]
mut profile is hash:
    score is 3
end
update_array(numbers)
update_hash(profile)
Sayln numbers
Sayln profile
"#;
    let (success, output) = run_source_stdout(source);
    assert!(success, "mutable array/hash borrowing failed: {output}");
    assert_eq!(output, "[5, 2]\n{score: 8}\n");
}

#[test]
fn local_reference_bindings_hold_scoped_shared_and_exclusive_borrows() {
    let source = r#"mut values is list [1]
fn mutate_through_shared():
    alias is ref values
    values[0] -> 9
end
mutate_through_shared()
"#;
    let (success, error) = run_source(source);
    assert!(!success, "owner mutation bypassed a local shared borrow");
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");

    let source = r#"mut values is list [1]
fn mutate_through_exclusive():
    mut alias is ref values
    alias[0] -> 9
end
mutate_through_exclusive()
Sayln values
"#;
    let (success, output) = run_source_stdout(source);
    assert!(success, "local exclusive borrow failed: {output}");
    assert_eq!(output, "[9]\n");

    let source = r#"mut values is list [1]
fn borrow_until_scope_exit():
    alias is ref values
end
borrow_until_scope_exit()
values[0] -> 2
Sayln values
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "local borrow remained active after scope exit: {output}"
    );
    assert_eq!(output, "[2]\n");
}

#[test]
fn borrowed_collection_bindings_cannot_be_reassigned_until_the_borrow_ends() {
    let source = r#"mut values is list [1]
fn reassign_owner():
    alias is ref values
    values -> list [2]
end
reassign_owner()
"#;
    let (success, error) = run_source(source);
    assert!(!success, "reassigning a borrowed collection was accepted");
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");
    assert!(error.contains("cannot reassign `values` while its collection is borrowed"));

    let source = r#"mut values is list [1]
mut replacement is list [2]
fn reassign_owner():
    alias is ref values
    values -> replacement
end
reassign_owner()
"#;
    let (success, error) = run_source(source);
    assert!(
        !success,
        "reassignment to a different collection bypassed the borrow"
    );
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");

    let source = r#"mut values is list [1]
mut other is list [3]
fn reassign_owner():
    alias is ref values
    (values, other) -> (list [2], list [4])
end
reassign_owner()
"#;
    let (success, error) = run_source(source);
    assert!(!success, "destructuring reassignment bypassed the borrow");
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");
}

#[test]
fn local_exclusive_borrows_reject_overlapping_aliases() {
    let source = r#"mut values is list [1]
fn conflicting():
    mut first is ref values
    mut second is ref values
end
conflicting()
"#;
    let (success, error) = run_source(source);
    assert!(
        !success,
        "overlapping exclusive local borrows were accepted"
    );
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");
    assert!(error.contains("conflicting borrow of collection"));
}

#[test]
fn local_element_borrows_read_and_write_array_list_and_hash_slots() {
    let source = r#"mut values is list [1, 2]
fn update_slot(index as Int):
    mut slot is ref values[index]
    slot -> 9
    Sayln slot
end
update_slot(1)
Sayln values
mut profile is hash:
    name is "Ada"
end
fn rename():
    mut name is ref profile["name"]
    name -> "Grace"
end
rename()
Sayln profile["name"]
"#;
    let (success, output) = run_source_stdout(source);
    assert!(success, "local element borrowing failed: {output}");
    assert_eq!(output, "9\n[1, 9]\nGrace\n");

    let source = r#"values as Array[Int] is [4, 5]
slot is ref values[1]
Sayln slot
"#;
    let (success, output) = run_source_stdout(source);
    assert!(success, "shared element read failed: {output}");
    assert_eq!(output, "5\n");

    let source = r#"mut values as Array[Int] is [4, 5]
mut first is ref values[0]
first -> 8
Sayln values
"#;
    let (success, output) = run_source_stdout(source);
    assert!(success, "Array element mutation failed: {output}");
    assert_eq!(output, "[8, 5]\n");

    let source = r#"values is list [6, 7]
fn read_both():
    first is ref values[0]
    second is ref values[1]
    Sayln first
    Sayln second
end
read_both()
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "compatible shared element borrows failed: {output}"
    );
    assert_eq!(output, "6\n7\n");

    let source = r#"mut values is list [1]
fn update():
    mut slot is ref values[0]
    slot -> 8
end
update()
values[0] -> 9
Sayln values
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "element borrow remained active after scope exit: {output}"
    );
    assert_eq!(output, "[9]\n");

    let source = r#"enum Fault:
    Failed
end
mut values is list [1]
fn fail():
    mut slot is ref values[0]
    throw Fault::Failed
end
try:
    fail()
catch error:
    values[0] -> 2
end
Sayln values
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "element borrow remained active after error: {output}"
    );
    assert_eq!(output, "[2]\n");
}

#[test]
fn local_element_borrows_follow_nested_collection_paths() {
    let source = r#"mut record is hash:
    details is hash:
        scores is list [10, 20]
    end
end
mut slot is ref record["details"]["scores"][1]
slot -> 99
Sayln record["details"]["scores"][1]
"#;
    let (success, output) = run_source_stdout(source);
    assert!(success, "nested Hash/List borrow failed: {output}");
    assert_eq!(output, "99\n");

    let source = r#"mut grid is list [list [1, 2], list [3, 4]]
mut slot is ref grid[1][0]
slot -> 8
Sayln grid
"#;
    let (success, output) = run_source_stdout(source);
    let diagnostic = if success {
        String::new()
    } else {
        run_source(source).1
    };
    assert!(
        success,
        "nested List/List borrow failed: {output}{diagnostic}"
    );
    assert_eq!(output, "[[1, 2], [8, 4]]\n");

    let source = r#"mut grid is list [list [1, 2]]
mut first is ref grid[0][0]
mut second is ref grid[0][1]
first -> 7
second -> 8
Sayln grid
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "disjoint nested element borrows conflicted: {output}"
    );
    assert_eq!(output, "[[7, 8]]\n");
}

#[test]
fn nested_borrow_paths_compare_aliases_and_resolved_dynamic_indices() {
    let conflict = r#"mut record is hash:
    scores is list [1]
end
mut alias is record["scores"]
fn conflict():
    mut first is ref record["scores"][0]
    mut second is ref alias[0]
end
conflict()
"#;
    let (success, error) = run_source(conflict);
    assert!(!success, "aliased nested locations were borrowed twice");
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");

    let conflict = r#"mut values is list [list [1]]
mut row is values[0]
fn conflict(index as Int):
    mut first is ref values[0][index]
    mut second is ref row[0]
end
conflict(0)
"#;
    let (success, error) = run_source(conflict);
    assert!(
        !success,
        "equal runtime indices through parent aliases were treated as disjoint"
    );
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");

    let source = r#"mut values is list [1, 2]
mut left_index is 0
mut right_index is 1
mut left is ref values[left_index]
mut right is ref values[right_index]
left -> 7
right -> 8
Sayln values
"#;
    let (success, output) = run_source_stdout(source);
    assert!(success, "concrete dynamic indices conflicted: {output}");
    assert_eq!(output, "[7, 8]\n");
}

#[test]
fn nested_element_borrows_are_released_on_scope_exit_and_runtime_error() {
    let source = r#"enum Fault:
    Failed
end
mut record is hash:
    scores is list [1]
end
fn scoped():
    mut slot is ref record["scores"][0]
    slot -> 2
end
scoped()
record["scores"][0] -> 3
fn failing():
    mut slot is ref record["scores"][0]
    throw Fault::Failed
end
try:
    failing()
catch error:
    record["scores"][0] -> 4
end
Sayln record["scores"][0]
"#;
    let (success, output) = run_source_stdout(source);
    let diagnostic = if success {
        String::new()
    } else {
        run_source(source).1
    };
    assert!(
        success,
        "nested element borrow was not cleaned up: {output}{diagnostic}"
    );
    assert_eq!(output, "4\n");
}

#[test]
fn local_element_borrows_reject_conflicts_and_unsupported_locations() {
    for source in [
        r#"mut values is list [1, 2]
fn conflict():
    mut first is ref values[0]
    mut second is ref values[0]
end
conflict()
"#,
        r#"mut values is list [1]
fn mutate_owner():
    slot is ref values[0]
    values[0] -> 2
end
mutate_owner()
"#,
        r#"mut values is list [1]
fn mutate_structure():
    slot is ref values[0]
    values add 2
end
mutate_structure()
"#,
    ] {
        let (success, error) = run_source(source);
        assert!(!success, "conflicting element access was accepted");
        assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");
    }

    for source in [
        r#"mut record is hash:
    scores is list [1, 2]
end
mut scores is record["scores"]
mut slot is ref record["scores"][0]
mut alias is ref scores[0]
"#,
        r#"mut record is hash:
    scores is list [1, 2]
end
mut scores is record["scores"]
mut slot is ref record["scores"][0]
scores[1] -> 9
"#,
    ] {
        let (success, error) = run_source(source);
        assert!(
            !success,
            "nested borrow conflict or owner mutation was accepted"
        );
        assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");
    }

    for source in [
        "mut values is list [list [1]]\nslot is ref values[0]\n",
        "mut values is list [list [1]]\nslot is ref values[0][4]\n",
        "mut values is list [1]\nmut slot is ref values[4]\n",
        "values is list [1]\nmut slot is ref values[0]\n",
        "mut values is hash:\n    present is [1]\nend\nslot is ref values[\"missing\"][0]\n",
    ] {
        let (success, error) = run_source(source);
        assert!(
            !success,
            "unsupported or invalid element borrow was accepted"
        );
        assert!(
            error.contains("error[E.semantic.ref.invalid]")
                || error.contains("error[E.runtime.collection"),
            "{error}"
        );
    }
}

#[test]
fn local_references_forward_and_cannot_escape_or_be_captured() {
    let source = r#"fn first(ref values as List[Int]) gives Int:
    return values[0]
end
mut values is list [42]
fn read():
    alias is ref values
    return first(alias)
end
Sayln read()
"#;
    let (success, output) = run_source_stdout(source);
    assert!(success, "local shared borrow forwarding failed: {output}");
    assert_eq!(output, "42\n");

    let source = r#"fn set_first(mut ref values as List[Int]):
    values[0] -> 7
end
mut values is list [42]
fn update():
    mut alias is ref values
    set_first(alias)
end
update()
Sayln values
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "local exclusive borrow forwarding failed: {output}"
    );
    assert_eq!(output, "[7]\n");

    for source in [
        "mut values is list [1]\nfn leak():\n    alias is ref values\n    return alias\nend\n",
        "mut values is list [1]\nfn leak():\n    alias is ref values\n    copy is alias\nend\n",
        "mut values is list [1]\nfn leak():\n    alias is ref values\n    fn nested():\n        Sayln alias\n    end\nend\n",
    ] {
        let (valid, _, error) = check_source(source);
        assert!(
            !valid,
            "a local reference escaped its binding scope:\n{source}"
        );
        assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");
    }
}

#[test]
fn active_element_borrows_block_owner_replacement_and_structural_list_changes() {
    let source = r#"mut values is list [1, 2]
mut alias is values
mut replacement_rejected is false
mut insertion_rejected is false
mut deletion_rejected is false
mut sibling_replacement_rejected is false
mut destructuring_rejected is false
mut other is 0
fn exercise():
    mut first is ref values[0]
    try:
        values[1] -> 9
    catch error:
        sibling_replacement_rejected -> true
    end
    try:
        values add 3
    catch error:
        insertion_rejected -> true
    end
    try:
        values remove 2
    catch error:
        deletion_rejected -> true
    end
    try:
        values -> list [8]
    catch error:
        replacement_rejected -> true
    end
    try:
        (values, other) -> (list [8], 5)
    catch error:
        destructuring_rejected -> true
    end
    first -> 7
end
exercise()
Sayln sibling_replacement_rejected
Sayln insertion_rejected
Sayln deletion_rejected
Sayln replacement_rejected
Sayln destructuring_rejected
Sayln values
Sayln alias
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "a rejected owner mutation damaged the active borrow: {output}"
    );
    assert_eq!(output, "true\ntrue\ntrue\ntrue\ntrue\n[7, 2]\n[7, 2]\n");
}

#[test]
fn nested_and_hash_slot_borrows_survive_failed_replacement_and_write() {
    let source = r#"mut rows is list [list [1]]
mut alias is rows
mut parent_replacement_rejected is false
mut type_error_caught is false
fn exercise():
    mut item is ref rows[0][0]
    try:
        rows[0] -> list [9]
    catch error:
        parent_replacement_rejected -> true
    end
    try:
        item -> "wrong"
    catch error:
        type_error_caught -> true
    end
    item -> 2
end
exercise()
Sayln parent_replacement_rejected
Sayln type_error_caught
Sayln rows
Sayln alias

mut record is hash:
    first is 3
    second is 4
end
mut hash_replacement_rejected is false
fn update_hash():
    mut first is ref record["first"]
    try:
        record["first"] -> 8
    catch error:
        hash_replacement_rejected -> true
    end
    first -> 5
end
update_hash()
Sayln hash_replacement_rejected
Sayln record
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "failed nested replacement left a stale borrow or changed storage: {output}"
    );
    assert_eq!(
        output,
        "true\ntrue\n[[2]]\n[[2]]\ntrue\n{first: 5, second: 4}\n"
    );
}

#[test]
fn exclusive_reborrows_follow_forwarded_paths_and_cleanup_after_failure() {
    let source = r#"fn leaf(mut ref rows as List[List[Int]]):
    mut item is ref rows[0][0]
    item -> 4
end
fn middle(mut ref rows as List[List[Int]]):
    leaf(rows)
    rows[0][0] -> 5
end
fn outer(mut ref rows as List[List[Int]]):
    middle(rows)
    rows[0][0] -> 6
end
mut rows is list [list [1]]
outer(rows)
Sayln rows
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "forwarded nested exclusive reborrows did not release inner loans: {output}"
    );
    assert_eq!(output, "[[6]]\n");

    let source = r#"mut values is list [1]
mut failed_reborrow is false
fn attempt(mut ref values as List[Int]):
    try:
        mut invalid is ref values[4]
    catch error:
        failed_reborrow -> true
    end
    values[0] -> 2
end
attempt(values)
Sayln failed_reborrow
Sayln values
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "failed reborrow left the forwarded exclusive borrow inconsistent: {output}"
    );
    assert_eq!(output, "true\n[2]\n");
}

#[test]
fn shared_reborrows_are_scoped_inside_an_exclusive_forwarding_chain() {
    let source = r#"fn first(ref values as List[Int]) gives Int:
    return values[0]
end
fn inspect(mut ref values as List[Int]) gives Int:
    alias is ref values
    return first(alias)
end
fn update(mut ref values as List[Int]):
    Sayln inspect(values)
    values[0] -> 2
end
mut values is list [1]
update(values)
Sayln values
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "shared reborrow was not released before the outer exclusive reference resumed: {output}"
    );
    assert_eq!(output, "1\n[2]\n");
}

#[test]
fn shared_reference_iteration_reads_nested_values_and_allows_shared_aliases() {
    let source = r#"mut values is list [list [1, 2], list [3, 4]]
fn add_first(ref left as List[Int], ref right as List[Int]) gives Int:
    return left[0] + right[0]
end
mut total is 0
for ref row in values:
    alias is ref row
    total -> total + add_first(row, alias)
    total -> total + row[1]
end
Sayln total
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "shared iteration or a shared alias to its nested value failed: {output}"
    );
    assert_eq!(output, "14\n");

    let source = r#"mut records is list [hash:
    score is 5
end]
for ref record in records:
    Sayln record["score"]
end
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "nested Hash reads through a shared iteration binding failed: {output}"
    );
    assert_eq!(output, "5\n");
}

#[test]
fn shared_reference_iteration_rejects_writes_and_cleans_up() {
    let source = r#"mut values is list [1, 2]
mut element_write_rejected is false
mut structure_write_rejected is false
for ref item in values:
    try:
        mut alias is ref values[0]
    catch error:
        element_write_rejected -> true
    end
    try:
        values add 3
    catch error:
        structure_write_rejected -> true
    end
end
values add 4
Sayln element_write_rejected
Sayln structure_write_rejected
Sayln values
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "shared iteration did not reject conflicting access or release its loan: {output}"
    );
    assert_eq!(output, "true\ntrue\n[1, 2, 4]\n");

    let source = r#"enum Fault:
    Failed
end
mut values is list [1]
try:
    for ref item in values:
        throw Fault::Failed
    end
catch error:
end
values add 2
Sayln values
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "iteration error did not clean up the shared borrow: {output}"
    );
    assert_eq!(output, "[1, 2]\n");

    let source = "mut values is list [1]\nfor ref item in values:\n    values add 2\nend\n";
    let (success, error) = run_source(source);
    assert!(
        !success,
        "owner structural mutation bypassed shared iteration"
    );
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");
    assert!(error.contains("active borrow"), "{error}");
    assert!(error.contains("3:5"), "missing source location: {error}");

    let source = r#"fn first(ref values as List[Int]) gives Int:
    for ref item in values:
        return item
    end
    return 0
end
mut values is list [1, 2]
Sayln first(values)
values add 3
for ref item in values:
    break
end
values add 4
Sayln values
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "return or break failed to release the iteration borrow: {output}"
    );
    assert_eq!(output, "1\n[1, 2, 3, 4]\n");
}

#[test]
fn shared_reference_iteration_supports_empty_nested_and_hash_collections() {
    let empty = "mut values is list []\nfor ref item in values:\n    Sayln item\nend\nSayln 1\n";
    let (success, output) = run_source_stdout(empty);
    assert!(success, "empty shared iteration failed: {output}");
    assert_eq!(output, "1\n");

    let source = r#"mut values is list [list [list [7]]]
mut total is 0
for ref rows in values:
    for ref row in rows:
        total -> total + row[0]
    end
end
Sayln total
"#;
    let (success, output) = run_source_stdout(source);
    assert!(success, "nested shared iteration failed: {output}");
    assert_eq!(output, "7\n");

    let source = r#"mut values is hash:
    alpha is 1
    beta is 2
end
mut rejected is false
for ref value in values:
    Sayln value
    try:
        values["gamma"] -> 3
    catch error:
        rejected -> true
    end
end
values["gamma"] -> 3
Sayln rejected
Sayln values["gamma"]
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "Hash shared iteration changed ordering, mutation checks, or cleanup: {output}"
    );
    assert_eq!(output, "1\n2\ntrue\n3\n");

    let array = "values is [4, 5]\nfor ref item in values:\n    Sayln item\nend\n";
    let (success, output) = run_source_stdout(array);
    assert!(success, "Array shared iteration failed: {output}");
    assert_eq!(output, "4\n5\n");
}

#[test]
fn shared_reference_iteration_preserves_by_value_loops_and_rejects_unsupported_sources() {
    let source = r#"mut values is list [1, 2]
mut first is true
for item in values:
    Sayln item
    if first:
        values add 3
        first -> false
    end
end
Sayln values
"#;
    let (success, output) = run_source_stdout(source);
    assert!(success, "existing by-value iteration changed: {output}");
    assert_eq!(output, "1\n2\n[1, 2, 3]\n");

    for source in [
        "values is list [1]\nfor ref item in [1, 2]:\n    Sayln item\nend\n",
        "values is (1, 2)\nfor ref item in values:\n    Sayln item\nend\n",
        "range is range(1, 3)\nfor ref item in range:\n    Sayln item\nend\n",
        "mut values is list [1]\nfor ref item in values:\n    item -> 2\nend\n",
        "values is list [1]\nfor ref [item] in values:\n    Sayln item\nend\n",
    ] {
        let (success, error) = run_source(source);
        assert!(!success, "`for ref` accepted an unsupported use:\n{source}");
        assert!(
            error.contains("error[E.semantic.ref.invalid]")
                || error.contains("error[E.semantic.collection]")
                || error.contains("error[E.syntax.token.unexpected]"),
            "{error}"
        );
    }

    let escaping =
        "mut values is list [list [1]]\nfor ref item in values:\n    snapshot is item\nend\n";
    let (valid, _, error) = check_source(escaping);
    assert!(!valid, "a nested reference escaped iteration:\n{escaping}");
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");

    let capture = "mut values is list [list [1]]\nfor ref item in values:\n    fn escaped():\n        return item\n    end\nend\n";
    let (valid, _, error) = check_source(capture);
    assert!(!valid, "a closure captured a reference iteration binding");
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");

    let runtime_escape = r#"mut values is list [list [1]]
mut snapshots is list []
mut rejected is false
for ref item in values:
    try:
        snapshots add item
    catch error:
        rejected -> true
    end
end
values add list [2]
Sayln rejected
Sayln snapshots
Sayln values
"#;
    let (success, output) = run_source_stdout(runtime_escape);
    assert!(
        success,
        "runtime escape rejection or borrow cleanup failed: {output}"
    );
    assert_eq!(output, "true\n[]\n[[1], [2]]\n");

    let runtime_return_escape = r#"fn escape(value as List[Int]) gives List[Int]:
    return value
end
mut values is list [list [1]]
for ref item in values:
    escape(item)
end
"#;
    let (success, error) = run_source(runtime_return_escape);
    assert!(
        !success,
        "runtime allowed a borrowed value to escape by return"
    );
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");
    assert!(error.contains("2:5"), "missing source location: {error}");
    assert!(
        error.contains("shared iteration value cannot escape"),
        "{error}"
    );
}

#[test]
fn ref_values_can_be_used_to_build_independent_key_collections() {
    let source = r#"fn profile_keys(ref profile as Hash) gives Array[String]:
    return keys(profile)
end
profile is hash:
    name is "Ada"
    role is "builder"
end
names is profile_keys(profile)
Sayln names
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "independent result was rejected as a ref escape: {output}"
    );
    assert_eq!(output, "[name, role]\n");
}

#[test]
fn ref_scalar_reads_can_be_copied_into_escaping_collections() {
    let source = r#"fn first_snapshot(ref values as List[Int]) gives List[Int]:
    return list [values[0]]
end
items is list [42, 99]
snapshot is first_snapshot(items)
Sayln snapshot
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "a fresh collection of copied scalar values was rejected: {output}"
    );
    assert_eq!(output, "[42]\n");

    let source = r#"fn nested_snapshot(ref values as List[List[Int]]) gives List[List[Int]]:
    return list [values[0]]
end
items is list [list [42]]
nested_snapshot(items)
"#;
    let (valid, _, error) = check_source(source);
    assert!(
        !valid,
        "a borrowed nested collection escaped inside a new list"
    );
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");
}

#[test]
fn ref_can_flow_through_fresh_builtin_collections_without_escaping_nested_handles() {
    let source = r#"fn reversed(ref values as List[Int]) gives List[Int]:
    return reverse(values)
end
items is list [10, 20, 30]
Sayln reversed(items)
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "fresh reversed values were treated as borrowed: {output}"
    );
    assert_eq!(output, "[30, 20, 10]\n");

    let source = r#"fn reverse_nested(ref values as List[List[Int]]) gives List[List[Int]]:
    return reverse(values)
end
items is list [list [10]]
reverse_nested(items)
"#;
    let (valid, _, error) = check_source(source);
    assert!(!valid, "reverse let a nested borrowed collection escape");
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");
}

#[test]
fn invalid_ref_usage_uses_one_diagnostic_and_marks_its_source_location() {
    for (source, line, column, detail) in [
        (
            "fn invalid() gives List[Int]:\n    return ref values\nend\n",
            2,
            12,
            "`ref` cannot be returned from a function",
        ),
        (
            "fn escape(ref values as List[Int]) gives List[Int]:\n    return values\nend\n",
            2,
            5,
            "cannot escape through a return value",
        ),
        (
            "fn store(ref values as List[Int]) gives Unit:\n    copy is values\nend\n",
            2,
            5,
            "cannot escape through a variable binding",
        ),
        (
            "fn capture(ref values as List[Int]) gives Unit:\n    fn nested() gives Int:\n        return values[0]\n    end\nend\n",
            2,
            5,
            "closure cannot capture a `ref` parameter",
        ),
        (
            "fn mutate(ref values as List[Int]) gives Unit:\n    values[0] -> 2\nend\n",
            2,
            5,
            "read-only and cannot be mutated",
        ),
        (
            "fn mutate(ref values as List[Int]) gives Unit:\n    values add 2\nend\n",
            2,
            5,
            "read-only and cannot be mutated",
        ),
        (
            "fn reassign(ref values as List[Int]) gives Unit:\n    values -> list [2]\nend\n",
            2,
            5,
            "read-only and cannot be reassigned",
        ),
    ] {
        let (valid, _, error) = check_source(source);
        assert!(!valid, "invalid `ref` usage was accepted");
        assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");
        assert!(error.contains("(Semantic error)"), "{error}");
        assert!(error.contains(&format!(":{line}:{column}")), "{error}");
        assert!(error.contains(detail), "{error}");
    }
}

#[test]
fn mutable_collection_cycles_remain_safe_in_the_evaluator_arena() {
    let source = r#"mut first as Hash is hash:
    value is 1
end
mut second as Hash is hash:
    value is 1
end
first["self"] -> first
second["self"] -> second
Sayln first == second
Sayln first
mut items is [1]
alias is items
items[0] -> 2
Sayln alias[0]
"#;
    let (success, output) = run_source_stdout(source);
    assert!(success, "cyclic collection operations failed: {output}");
    assert_eq!(output, "true\n{self: <cycle>, value: 1}\n2\n");
}

#[test]
fn removing_nested_collections_does_not_hold_a_mutable_arena_borrow() {
    let source = r#"mut values is list [list [1], list [2]]
mut target is list [1]
values remove target
Sayln values
"#;
    let (success, output) = run_source_stdout(source);
    assert!(success, "removing a nested collection failed: {output}");
    assert_eq!(output, "[[2]]\n");
}

#[test]
fn runs_standard_library_builtins() {
    let output = run_example("examples/08-standard-library/builtins.si");
    assert!(output.contains("[1, 2, 3, 4]"));
    assert!(output.matches("4").count() >= 2);
    assert!(output.contains("6"));
}

#[test]
fn assertions_validate_conditions_and_report_optional_messages() {
    let source = "assert(true)\nassert(true, \"unused\")\nassert(true, substring(\"x\", 5, 1))\nSayln \"continued\"\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "valid assertions did not type-check: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "successful assertions failed: {output}");
    assert_eq!(output, "continued\n");

    let (success, error) = run_source("assert(false, \"expected a positive value\")\n");
    assert!(!success);
    assert!(
        error.contains("expected a positive value"),
        "missing assertion message: {error}"
    );

    let (valid, _, error) = check_source("assert(1)\n");
    assert!(!valid);
    assert!(error.contains("expected Bool, found Int"), "{error}");

    let (valid, _, error) = check_source("assert(true, 1)\n");
    assert!(!valid);
    assert!(error.contains("expected String, found Int"), "{error}");
}

#[test]
fn runs_boolean_and_string_collection_builtins() {
    let (success, stdout) = run_source_stdout(
        "flags is list [true, false]\nwords is list [\"Ada\", \"Lin\"]\nSayln any(flags)\nSayln all(flags)\nSayln join(words, \"-\")\n",
    );
    assert!(success);
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        ["true", "false", "Ada-Lin"]
    );

    let (success, _, error) = check_source("Sayln join(list [1], \",\")\n");
    assert!(!success);
    assert!(error.contains("expected String, found Int"));
}

#[test]
fn hash_keys_and_values_follow_deterministic_key_order() {
    let source = "profile is hash:\n\
                      name is \"Ada\"\n\
                      age is 36\n\
                  end\n\
                  scores is hash:\n\
                      z is 3\n\
                      a is 1\n\
                  end\n\
                  Sayln keys(profile)\n\
                  Sayln values(profile)\n\
                  Sayln keys(scores)\n\
                  Sayln values(scores)\n\
                  Sayln entries(profile)\n\
                  Sayln has_key(profile, \"name\")\n\
                  Sayln has_key(profile, \"missing\")\n\
                  Sayln has_key(scores, \"z\")\n\
                  for entry in entries(scores):\n\
                      Sayln entry[0] + \":\" + type_of(entry[1])\n\
                  end\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "map keys/values did not type-check: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "map keys/values failed: {output}");
    assert_eq!(
        output,
        "[age, name]\n[36, Ada]\n[a, z]\n[1, 3]\n[[age, 36], [name, Ada]]\ntrue\nfalse\ntrue\na:Int\nz:Int\n"
    );

    for source in [
        "keys([1, 2])\n",
        "values(\"text\")\n",
        "entries([1, 2])\n",
        "has_key([1, 2], \"key\")\n",
        "has_key(hash:\n    value is 1\nend, 1)\n",
    ] {
        let (valid, _, error) = check_source(source);
        assert!(!valid);
        assert!(
            error.contains("requires a Hash") || error.contains("expected String, found Int"),
            "{error}"
        );
    }

    let (success, output) = run_source_stdout(
        "fn entries(value as Int) gives Int:\n    return value\nend\nSayln entries(7)\n",
    );
    assert!(
        success,
        "user-defined entries function was shadowed: {output}"
    );
    assert_eq!(output, "7\n");
}

#[test]
fn gets_map_values_or_lazy_defaults_with_type_checks() {
    let source = "profile is hash:\n\
                      name is \"Ada\"\n\
                  end\n\
                  Sayln get(profile, \"name\", read_file(\"missing-default.txt\"))\n\
                  Sayln get(profile, \"role\", \"unknown\")\n\
                  Sayln type_of(get(profile, \"role\", \"unknown\"))\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "map get did not type-check: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "map get evaluated an unused default: {output}");
    assert_eq!(output, "Ada\nunknown\nString\n");

    let (valid, _, error) = check_source("get(hash:\n    age is 36\nend, \"age\", \"unknown\")\n");
    assert!(!valid);
    assert!(error.contains("expected Int, found String"), "{error}");

    let (valid, _, error) = check_source("get(hash:\n    age is 36\nend, 1, 0)\n");
    assert!(!valid);
    assert!(error.contains("expected String, found Int"), "{error}");

    let (success, output) =
        run_source_stdout("fn get(value as Int) gives Int:\n    return value\nend\nSayln get(7)\n");
    assert!(success, "user-defined get function was shadowed: {output}");
    assert_eq!(output, "7\n");
}

#[test]
fn without_key_returns_an_independent_hash() {
    let source = "profile is hash:\n\
                      name is \"Ada\"\n\
                      role is \"builder\"\n\
                  end\n\
                  filtered is without_key(profile, \"role\")\n\
                  missing is without_key(filtered, \"unknown\")\n\
                  catalog is hash:\n\
                      first is 1\n\
                      second is 2\n\
                  end\n\
                  filtered_map is without_key(catalog, \"first\")\n\
                  Sayln keys(filtered)\n\
                  Sayln keys(profile)\n\
                  Sayln keys(missing)\n\
                  Sayln type_of(filtered_map)\n\
                  Sayln keys(filtered_map)\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "without_key did not type-check: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "without_key failed: {output}");
    assert_eq!(output, "[name]\n[name, role]\n[name]\nHash\n[second]\n");

    for source in [
        "without_key([1], \"key\")\n",
        "without_key(hash:\n    age is 36\nend, 1)\n",
    ] {
        let (valid, _, error) = check_source(source);
        assert!(!valid);
        assert!(
            error.contains("requires a Hash") || error.contains("expected String, found Int"),
            "{error}"
        );
    }
}

#[test]
fn select_keys_filters_maps_without_changing_type_or_source() {
    let source = "profile is hash:\n\
                      name is \"Ada\"\n\
                      role is \"builder\"\n\
                      age is 36\n\
                  end\n\
                  selected is select_keys(profile, [\"role\", \"missing\", \"name\", \"role\"])\n\
                  catalog is hash:\n\
                      first is 1\n\
                      second is 2\n\
                  end\n\
                  selected_map is select_keys(catalog, (\"second\", \"missing\"))\n\
                  Sayln entries(selected)\n\
                  Sayln keys(profile)\n\
                  Sayln type_of(selected_map)\n\
                  Sayln entries(selected_map)\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "select_keys did not type-check: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "select_keys failed: {output}");
    assert_eq!(
        output,
        "[[name, Ada], [role, builder]]\n[age, name, role]\nHash\n[[second, 2]]\n"
    );

    for source in [
        "select_keys([1], [\"key\"])\n",
        "select_keys(hash:\n    value is 1\nend, [1])\n",
        "select_keys(hash:\n    value is 1\nend, \"value\")\n",
    ] {
        let (valid, _, error) = check_source(source);
        assert!(!valid);
        assert!(
            error.contains("requires a Hash")
                || error.contains("expected String, found Int")
                || error.contains("expects an array, list, or tuple of strings"),
            "{error}"
        );
    }
}

#[test]
fn runs_direct_sum_builtin() {
    let (success, stdout) = run_source_stdout(
        "values is list [10, 20, 30]\nSayln total(values)\nSayln total(array [1.5, 2.5])\n",
    );
    assert!(success);
    assert_eq!(stdout.lines().collect::<Vec<_>>(), ["60", "4"]);

    let (success, _, error) = check_source("Sayln total(list [1, \"two\"])\n");
    assert!(!success);
    assert!(error.contains("expected a number") || error.contains("String"));

    let (success, _, error) = check_source(
        "fn needs_text(value as String):\n    return value\nend\n\
         numbers is [1, 2]\nneeds_text(total(numbers))\n",
    );
    assert!(
        !success,
        "numeric total must not remain semantically Unknown"
    );
    assert!(error.contains("expected String, found Int"), "{error}");
}

#[test]
fn runs_string_builtins() {
    let (success, stdout) = run_source_stdout(
        "text is \"  Ada,Lin  \"\nSayln trim(text)\nSayln split(trim(text), \",\")\nSayln replace(text, \"Ada\", \"Citra\")\nSayln starts_with(trim(text), \"Ada\")\nSayln ends_with(trim(text), \"Lin\")\n",
    );
    assert!(success);
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        ["Ada,Lin", "[Ada, Lin]", "  Citra,Lin  ", "true", "true"]
    );
}

#[test]
fn runs_collection_utility_builtins() {
    let (success, stdout) = run_source_stdout(
        "values is list [1, 2, 3]\nempty is list []\nSayln reverse(values)\nSayln is_empty(empty)\nSayln is_empty(values)\nSayln is_empty(\"\")\n",
    );
    assert!(success);
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        ["[3, 2, 1]", "true", "false", "true"]
    );
}

#[test]
fn length_measures_collections_and_pipeline_count_counts_survivors() {
    let source = "values is list [4, 5, 6]\n\
                  Sayln length(values)\n\
                  Sayln length(\"aé\")\n\
                  selected is pipeline:\n\
                      values\n\
                      where item > 4\n\
                      count\n\
                  end\n\
                  Sayln selected\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "size/count distinction did not type-check: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "{output}");
    assert_eq!(output, "3\n2\n2\n");
}

#[test]
fn enumerate_indexes_sequences_ranges_and_unicode_strings() {
    let source = "numbers is list [7, 9]\n\
                  point is (\"Ada\", 36)\n\
                  Sayln enumerate(numbers)\n\
                  Sayln enumerate(range(3, 6))\n\
                  Sayln enumerate(\"aé\")\n\
                  Sayln enumerate(point)\n\
                  for pair in enumerate(numbers):\n\
                      Sayln pair[0] + pair[1]\n\
                  end\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "enumerate did not type-check: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "{output}");
    assert_eq!(
        output,
        "[[0, 7], [1, 9]]\n[[0, 3], [1, 4], [2, 5]]\n[[0, a], [1, é]]\n[[0, Ada], [1, 36]]\n7\n10\n"
    );
}

#[test]
fn enumerate_checks_arguments_and_preserves_user_function_shadowing() {
    let (valid, _, error) = check_source("enumerate(10)\n");
    assert!(!valid);
    assert!(
        error.contains("`enumerate` requires a sequence or string"),
        "{error}"
    );

    let (success, output) = run_source_stdout(
        "fn enumerate(value as Int) gives Int:\n\
             return value + 1\n\
         end\n\
         Sayln enumerate(4)\n",
    );
    assert!(success, "{output}");
    assert_eq!(output, "5\n");
}

#[test]
fn user_functions_shadow_builtins_consistently() {
    let source = "fn length(value as Int) gives Int:\n\
                      return value + 2\n\
                  end\n\
                  fn count(value as Int) gives Int:\n\
                      return value + 1\n\
                  end\n\
                  values is [3, 4]\n\
                  Sayln length(5)\n\
                  Sayln count(5)\n\
                  selected is pipeline:\n\
                      values\n\
                      count\n\
                  end\n\
                  Sayln selected\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "shadowed builtin did not type-check: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "{output}");
    assert_eq!(output, "7\n6\n2\n");
}

#[test]
fn zip_pairs_sequences_to_the_shorter_input() {
    let source = "numbers is list [7, 9, 11]\n\
                  letters is array [\"a\", \"b\"]\n\
                  point is (true, 4, \"extra\")\n\
                  Sayln zip(numbers, letters)\n\
                  Sayln zip(range(3, 7), \"xy\")\n\
                  Sayln zip(point, list [1, 2])\n\
                  Sayln zip(list [], range(0, 1000000000))\n\
                  for pair in zip(numbers, letters):\n\
                      Sayln pair\n\
                  end\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "zip did not type-check: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "{output}");
    assert_eq!(
        output,
        "[[7, a], [9, b]]\n[[3, x], [4, y]]\n[[true, 1], [4, 2]]\n[]\n[7, a]\n[9, b]\n"
    );
}

#[test]
fn zip_checks_arguments_and_preserves_user_function_shadowing() {
    let (valid, _, error) = check_source("zip([1], 2)\n");
    assert!(!valid);
    assert!(
        error.contains("`zip` requires sequences or strings"),
        "{error}"
    );

    let (success, output) = run_source_stdout(
        "fn zip(value as Int, other as Int) gives Int:\n\
             return value + other\n\
         end\n\
         Sayln zip(4, 5)\n",
    );
    assert!(success, "{output}");
    assert_eq!(output, "9\n");
}

#[test]
fn empty_collections_keep_unknown_element_type_for_mutation() {
    let (success, output) =
        run_source_stdout("mut values is list []\nvalues add \"ready\"\nSayln values\n");
    assert!(success, "{output}");
    assert_eq!(output, "[ready]\n");
}

#[test]
fn empty_list_mutation_infers_and_enforces_its_element_type() {
    for (source, expected_output) in [
        (
            "mut values is list []\nvalues add \"hello\"\nSayln values\n",
            "[hello]\n",
        ),
        (
            "mut values is list []\nvalues add 1\nSayln values\n",
            "[1]\n",
        ),
        (
            "mut values is list []\nvalues add \"hello\"\nvalues add \"world\"\nSayln values\n",
            "[hello, world]\n",
        ),
    ] {
        let (valid, _, check_error) = check_source(source);
        assert!(
            valid,
            "check rejected inferred list element type: {check_error}"
        );
        let (success, output) = run_source_stdout(source);
        assert!(success, "run rejected inferred list element type: {output}");
        assert_eq!(output, expected_output);
    }

    let incompatible = "mut values is list []\nvalues add \"hello\"\nvalues add 42\n";
    let (valid, _, check_error) = check_source(incompatible);
    assert!(!valid, "check accepted an incompatible list mutation");
    assert!(
        check_error.contains("expected String, found Int"),
        "{check_error}"
    );
    let (success, runtime_error) = run_source(incompatible);
    assert!(!success, "run accepted an incompatible list mutation");
    assert!(
        runtime_error.contains("expected String, found Int"),
        "{runtime_error}"
    );
}

#[test]
fn empty_float_sums_and_totals_preserve_the_inferred_float_type() {
    let cases = [
        (
            "mut values as List[Float] is list []\n\
             fn result() gives Float:\n\
                 return total(values)\n\
             end\n\
             Sayln type_of(result())\n",
            "Float\n",
        ),
        (
            "mut values as List[Float] is list []\n\
             result is pipeline:\n\
                 values\n\
                 sum\n\
             end\n\
             fn as_float() gives Float:\n\
                 return result\n\
             end\n\
             Sayln type_of(as_float())\n",
            "Float\n",
        ),
        (
            "mut values as List[Float] is list [1.0, 2.0]\n\
             result is pipeline:\n\
                 values\n\
                 where item > 10.0\n\
                 sum\n\
             end\n\
             fn as_float() gives Float:\n\
                 return result\n\
             end\n\
             Sayln type_of(as_float())\n",
            "Float\n",
        ),
        (
            "values as List[Float] is list [1.25, 2.5]\n\
             result is pipeline:\n\
                 values\n\
                 sum\n\
             end\n\
             Sayln result\n\
             Sayln type_of(result)\n",
            "3.75\nFloat\n",
        ),
        (
            "values as List[Int] is list []\n\
             result is pipeline:\n\
                 values\n\
                 derive item * 1.0\n\
                 sum\n\
             end\n\
             fn as_float() gives Float:\n\
                 return result\n\
             end\n\
             Sayln type_of(as_float())\n",
            "Float\n",
        ),
        (
            "values as List[Int] is list [1, 2]\n\
             result is pipeline:\n\
                 values\n\
                 where item > 10\n\
                 derive item * 1.0\n\
                 sum\n\
             end\n\
             fn as_float() gives Float:\n\
                 return result\n\
             end\n\
             Sayln type_of(as_float())\n",
            "Float\n",
        ),
        (
            "fn floats() gives List[Float]:\n\
                 return list []\n\
             end\n\
             result is pipeline:\n\
                 floats()\n\
                 sum\n\
             end\n\
             fn as_float() gives Float:\n\
                 return result\n\
             end\n\
             Sayln type_of(as_float())\n\
             Sayln type_of(total(floats()))\n",
            "Float\nFloat\n",
        ),
        (
            "mut values as List[Int] is list []\n\
             Sayln type_of(total(values))\n\
             result is pipeline:\n\
                 values\n\
                 sum\n\
             end\n\
             Sayln type_of(result)\n",
            "Int\nInt\n",
        ),
        (
            "values as List[Int] is list [1, 2]\n\
             result is pipeline:\n\
                 values\n\
                 where item > 10\n\
                 sum\n\
             end\n\
             fn as_int() gives Int:\n\
                 return result\n\
             end\n\
             Sayln type_of(as_int())\n",
            "Int\n",
        ),
    ];

    for (source, expected_output) in cases {
        let (valid, _, check_error) = check_source(source);
        assert!(valid, "check rejected the sum result type: {check_error}");
        let (success, output) = run_source_stdout(source);
        assert!(success, "run disagreed with the checked sum type: {output}");
        assert_eq!(output, expected_output);
    }

    let (valid, _, check_error) = check_source(
        "empty as List[Float] is list []\nflow result from empty:\n    parallel 2\n    sum\nend\n",
    );
    assert!(valid, "check rejected parallel Float sum: {check_error}");
    let (success, output) = run_source_stdout(
        "empty as List[Float] is list []\n\
         flow result from empty:\n\
             parallel 2\n\
             sum\n\
         end\n\
         Sayln type_of(result)\n",
    );
    assert!(success, "parallel Float sum failed at runtime: {output}");
    assert_eq!(output, "Float\n");

    let (valid, _, check_error) =
        check_source("result is pipeline:\n    range(4, 2)\n    sum\nend\nSayln type_of(result)\n");
    assert!(valid, "check rejected an empty range sum: {check_error}");
    let (success, output) = run_source_stdout(
        "result is pipeline:\n    range(4, 2)\n    sum\nend\nSayln type_of(result)\n",
    );
    assert!(success, "empty range sum failed at runtime: {output}");
    assert_eq!(output, "Int\n");
}

#[test]
fn builtin_semantics_match_runtime_collection_and_numeric_support() {
    let (success, output) = run_source_stdout(
        "Sayln total(range(1, 4))\n\
         Sayln clamp(5, 0.5, 4)\n\
         Sayln clamp(5.0, 0, 4)\n",
    );
    assert!(success, "{output}");
    assert_eq!(output, "6\n4\n4\n");
    let (success, error) =
        run_source("Sayln total(range(4611686018427387904, 4611686018427387906))\n");
    assert!(!success, "overflowing range total unexpectedly succeeded");
    assert!(error.contains("integer arithmetic error"), "{error}");

    let (success, output) =
        run_source_stdout("result is clamp(5, 0.5, 4)\nSayln type_of(result)\n");
    assert!(success, "{output}");
    assert_eq!(output, "Float\n");

    let (success, stdout) = run_source_stdout(
        "profile is hash:\n    name is \"Ada\"\nend\nSayln join(profile, \",\")\n",
    );
    assert!(success);
    assert_eq!(stdout, "Ada\n");

    for source in ["Sayln any(list [1, 2])\n", "Sayln round(1e308, 15)\n"] {
        let (success, output, error) = check_source(source);
        if source.contains("round") {
            assert!(success, "{output}{error}");
            let (ran, runtime_error) = run_source(source);
            assert!(!ran);
            assert!(
                runtime_error.contains("finite Float range"),
                "{runtime_error}"
            );
        } else {
            assert!(!success, "{error}");
        }
    }
}

#[test]
fn type_of_reports_range_and_csv_stream_as_distinct_runtime_kinds() {
    let (success, stdout) =
        run_source_stdout("Sayln type_of(range(1, 3))\nSayln type_of(csv_rows(\"unused.csv\"))\n");
    assert!(success);
    assert_eq!(stdout, "Range\nCsvStream\n");

    let (success, _, error) = check_source("value as Array[Int] is range(1, 3)\n");
    assert!(!success, "Range must not be an Array type alias");
    assert!(
        error.contains("expected Array[Int], found Range"),
        "{error}"
    );
}

#[test]
fn range_display_is_bounded_for_large_ranges_and_unchanged_for_small_ranges() {
    let (success, stdout) = run_source_stdout("Sayln range(0, 1000000000000)\nSayln range(1, 4)\n");
    assert!(success, "{stdout}");
    assert_eq!(stdout, "Range(0..1000000000000)\n[1, 2, 3]\n");
}

#[test]
fn check_accepts_reversing_a_range_as_supported_by_runtime() {
    let source = "reversed as Array[Int] is reverse(range(0, 3))\nSayln reversed\n";
    let (success, _, error) = check_source(source);
    assert!(
        success,
        "check rejected a runtime-supported sequence: {error}"
    );

    let (success, output) = run_source_stdout(source);
    assert!(success, "{output}");
    assert_eq!(output, "[2, 1, 0]\n");
}

#[test]
fn runs_collection_inspection_builtins() {
    let output = run_example("examples/08-standard-library/inspection.si");
    assert!(output.contains("true"));
    assert!(output.contains("false"));
    assert!(output.contains("List"));
    assert!(output.contains("Output uses Sayln"));
}

#[test]
fn checks_builtin_collection_arguments() {
    let (success, _, error) = check_source("contains(10, 10)\n");
    assert!(!success);
    assert!(error.contains("error[E.semantic.collection.shape-invalid]"));

    let (success, _, error) = check_source("length(10)\n");
    assert!(!success);
    assert!(error.contains("error[E.semantic.collection.shape-invalid]"));

    let (success, _, error) = check_source("contains(\"Ada\", 10)\n");
    assert!(!success);
    assert!(error.contains("expected String, found Int"));
}

#[test]
fn checks_empty_tuple_iteration_without_rejecting_valid_code() {
    let (success, _, error) = check_source("for item in ():\nend\n");
    assert!(success, "unexpected check error: {error}");
}

#[test]
fn checks_tuple_and_matrix_index_shapes() {
    let (success, _, error) = check_source("point is (10, \"Ada\")\nSayln point[2]\n");
    assert!(!success);
    assert!(error.contains("tuple index out of bounds"));

    let (success, _, error) = check_source("m is matrix [[1, 2]]\nSayln m[0]\n");
    assert!(!success);
    assert!(
        error.contains("matrix index requires a tuple of two integers"),
        "unexpected matrix index error: {error}"
    );
}

#[test]
fn indexes_matrix_rows_constructed_from_lists() {
    let source = "grid is matrix [list [1, 2], list [3, 4]]\nSayln grid[1, 0]\n";
    let (checked, _, error) = check_source(source);
    assert!(
        checked,
        "matrix list rows should be statically valid: {error}"
    );

    let (success, output) = run_source_stdout(source);
    assert!(success, "matrix list row indexing failed: {output}");
    assert_eq!(output, "3\n");
}

#[test]
fn matrix_operators_accept_list_rows_like_matrix_builtins() {
    let source = "left is matrix [list [1, 2]]\n\
                  right is matrix [list [3, 4]]\n\
                  total is left + right\n\
                  product_right is matrix [list [3], list [4]]\n\
                  product is left multiply product_right\n\
                  Sayln total[0, 1]\n\
                  Sayln product[0, 0]\n";
    let (success, output) = run_source_stdout(source);
    assert!(success, "matrix operator rejected list rows: {output}");
    assert_eq!(output, "6\n11\n");
}

#[test]
fn matrix_operations_reject_zero_sized_shapes_consistently() {
    for source in [
        "left is matrix []\nright is matrix []\nSayln left + right\n",
        "grid is matrix [[]]\nSayln transpose(grid)\n",
    ] {
        let (success, error) = run_source(source);
        assert!(
            !success,
            "zero-sized matrix operation unexpectedly succeeded"
        );
        assert!(
            error.contains("matrix must not be empty")
                || error.contains("matrix rows must not be empty"),
            "{error}"
        );
    }
}

#[test]
fn matrix_indices_reject_negative_and_unrepresentable_coordinates_safely() {
    for source in [
        "grid is matrix [[1]]\nSayln grid[-1, 0]\n",
        "grid is matrix [[1]]\nSayln grid[9223372036854775807, 0]\n",
    ] {
        let (success, error) = run_source(source);
        assert!(!success, "invalid matrix coordinate unexpectedly succeeded");
        assert!(
            error.contains("matrix index") || error.contains("out of bounds"),
            "{error}"
        );
        assert!(error.contains("error[E"), "{error}");
    }
}

#[test]
fn range_length_reports_unrepresentable_integer_lengths() {
    let (success, output) = run_source_stdout(
        "Sayln length(range(4, 4))\nSayln length(range(4, 2))\nSayln length(range(-2, 3))\n",
    );
    assert!(success, "{output}");
    assert_eq!(output, "0\n0\n5\n");

    let source = "Sayln length(range(-9223372036854775807, 9223372036854775807))\n";
    let (success, error) = run_source(source);
    assert!(
        !success,
        "an unrepresentable range length was silently saturated"
    );
    assert!(
        error.contains("range length exceeds the Int range"),
        "{error}"
    );
}

#[test]
fn range_expressions_are_first_class_lazy_half_open_values() {
    let source = r#"ages is 13..18
Sayln ages
Sayln 15 in ages
Sayln 18 in ages
Sayln contains(ages, 15)
Sayln -2 in -3..1
Sayln 5 in 5..2
Sayln -3..1
Sayln 5..2
for number in 1..5:
    Sayln number
end
fn allowed_scores() gives Range:
    return 10..20
end
fn contains_score(score as Int, allowed as Range) gives Bool:
    return score in allowed
end
Sayln contains_score(15, allowed_scores())
Sayln type_of(ages)
match 15:
    10..20:
        Sayln "half-open pattern"
    _:
end
match 20:
    10..=20:
        Sayln "inclusive pattern"
    _:
end
"#;
    let (valid, _, error) = check_source(source);
    assert!(valid, "range expression did not type-check: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "range expression failed: {output}");
    assert_eq!(
        output,
        "[13, 14, 15, 16, 17]\ntrue\nfalse\ntrue\ntrue\nfalse\n[-3, -2, -1, 0]\n[]\n1\n2\n3\n4\ntrue\nRange\nhalf-open pattern\ninclusive pattern\n"
    );

    let (success, error) = run_source("Sayln length(-9223372036854775807..9223372036854775807)\n");
    assert!(
        !success,
        "unrepresentable range length unexpectedly succeeded"
    );
    assert!(
        error.contains("range length exceeds the Int range"),
        "{error}"
    );

    let (valid, _, error) = check_source("ages is 1.5..4\n");
    assert!(!valid, "float range bound unexpectedly type-checked");
    assert!(error.contains("expected Int, found Float"), "{error}");
    let (success, error) = run_source("ages is 1.5..4\n");
    assert!(!success, "float range bound unexpectedly succeeded");
    assert!(error.contains("`..` requires two integers"), "{error}");
}

#[test]
fn stepped_ranges_are_lazy_and_support_both_directions() {
    let source = "up is range(0, 10, 3)\n\
                  down is range(10, 0, -3)\n\
                  Sayln up\n\
                  Sayln down\n\
                  Sayln length(up)\n\
                  Sayln up[2]\n\
                  Sayln range(-9223372036854775807, 9223372036854775807)[1]\n\
                  Sayln contains(down, 4)\n\
                  Sayln contains(down, 5)\n\
                  Sayln reverse(down)\n\
                  [first, ...tail] is down\n\
                  Sayln tail\n\
                  Sayln total(up)\n\
                  Sayln is_empty(range(0, 10, -1))\n\
                  flow stepped_total from range(0, 10, 3):\n\
                      sum\n\
                  end\n\
                  Sayln stepped_total\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "stepped ranges did not type-check: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "stepped range operations failed: {output}");
    assert_eq!(
        output,
        "[0, 3, 6, 9]\n[10, 7, 4, 1]\n4\n6\n-9223372036854775806\ntrue\nfalse\n[1, 4, 7, 10]\n[7, 4, 1]\n18\ntrue\n18\n"
    );

    let (success, error) = run_source("range(0, 10, 0)\n");
    assert!(!success);
    assert!(error.contains("`range` step cannot be zero"), "{error}");

    let (valid, _, error) = check_source("range(0, 10, 0)\n");
    assert!(!valid);
    assert!(error.contains("`range` step cannot be zero"), "{error}");
}

#[test]
fn hash_indexed_writes_follow_binding_mutability() {
    let source = r#"mut profile is hash:
    name is "Ada"
end
profile["name"] -> "Lin"
Sayln profile.name
"#;
    let (valid, _, error) = check_source(source);
    assert!(
        valid,
        "mutable hash write did not type-check:\n{source}\n{error}"
    );
    let (success, output) = run_source_stdout(source);
    assert!(success, "{output}");
    assert_eq!(output, "Lin\n");

    let source = r#"profile is hash:
    name is "Ada"
end
profile["name"] -> "Lin"
"#;
    let (valid, _, error) = check_source(source);
    assert!(!valid, "immutable hash write was accepted");
    assert!(
        error.contains("cannot mutate immutable variable"),
        "{error}"
    );
}

#[test]
fn rejects_ragged_matrices() {
    let (success, error) = run_source(
        "left is matrix [[1, 2], [3]]\nright is matrix [[1], [2]]\nSayln left + right\n",
    );
    assert!(!success);
    assert!(error.contains("equal widths"));
}

#[test]
fn rejects_ambiguous_collection_definitions() {
    let (success, duplicate_error) =
        run_source("settings is hash:\n    mode is \"a\"\n    mode is \"b\"\nend\n");
    assert!(!success);
    assert!(duplicate_error.contains("duplicate field"));

    let (success, filter_error) =
        run_source("values is list [1]\nresult is pipeline:\n    values\n    where item\nend\n");
    assert!(!success);
    assert!(
        filter_error.contains("must return a boolean"),
        "unexpected filter error: {filter_error}"
    );

    let (success, terminal_error) = run_source(
        "values is list [1]\nresult is pipeline:\n    values\n    count\n    derive item\nend\n",
    );
    assert!(!success);
    assert!(terminal_error.contains("cannot continue"));
}
