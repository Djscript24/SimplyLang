mod common;
use common::*;

#[test]
fn constructs_nominal_structs_and_dispatches_type_specific_messages() {
    let source = "type Person:\n    name as String\n    age as Int\nend\n\
                  type User:\n    name as String\n    age as Int\nend\n\
                  fn greet(value):\n    return \"global\"\nend\n\
                  on Person receive greet:\n    return \"Hello \" + name\nend\n\
                  on User receive greet:\n    return \"Welcome \" + name\nend\n\
                  person is Person(\"Ada\", 37)\n\
                  user is User(\"Lin\", 20)\n\
                  Sayln person :: greet\n\
                  Sayln user :: greet\n\
                  Sayln type_of(person)\n";
    let (success, output) = run_source_stdout(source);
    assert!(success, "struct dispatch failed: {output}");
    assert_eq!(output, "Hello Ada\nWelcome Lin\nPerson\n");
}

#[test]
fn message_behaviors_read_instance_state_without_cross_instance_leaks() {
    let source = "type Person:\n    name as String\nend\n\
                  on Person receive introduce(to as String):\n\
                      return \"Hello \" + to + \", I'm \" + name\n\
                  end\n\
                  ada is Person(\"Ada\")\n\
                  grace is Person(\"Grace\")\n\
                  Sayln ada :: introduce(\"Budi\")\n\
                  Sayln grace :: introduce(\"Lin\")\n\
                  Sayln ada :: introduce(\"Mira\")\n";
    let (success, output) = run_source_stdout(source);
    assert!(success, "message state access failed: {output}");
    assert_eq!(
        output,
        "Hello Budi, I'm Ada\nHello Lin, I'm Grace\nHello Mira, I'm Ada\n"
    );
}

#[test]
fn rejects_unknown_structs_invalid_construction_and_nominal_type_mismatches() {
    for (source, expected) in [
        (
            "person is DoesNotExist(\"Ada\")\n",
            "unknown struct type `DoesNotExist`",
        ),
        (
            "type Person:\n    name as String\n    age as Int\nend\nperson is Person(\"Ada\")\n",
            "struct `Person` expects 2 fields, got 1",
        ),
        (
            "type Person:\n    name as String\nend\nperson is Person(\"Ada\", 1)\n",
            "struct `Person` expects 1 fields, got 2",
        ),
        (
            "type Person:\n    age as Int\nend\nperson is Person(\"old\")\n",
            "expected Int, found String",
        ),
        (
            "type Person:\n    name as String\nend\ntype User:\n    name as String\nend\n\
             value as User is Person(\"Ada\")\n",
            "expected User, found Person",
        ),
    ] {
        let (success, _, error) = check_source(source);
        assert!(!success, "invalid struct source passed checking");
        assert!(error.contains(expected), "missing {expected:?}: {error}");
    }
}

#[test]
fn diagnoses_duplicate_struct_fields_messages_and_unknown_state() {
    let duplicate_field = "type Person:\n    name as String\n    name as String\nend\n";
    let (success, _, error) = check_source(duplicate_field);
    assert!(!success);
    assert!(error.contains("duplicate field `name`"));
    assert!(error.contains("3:5"));

    let duplicate_message = "type Person:\n    name as String\nend\n\
                             on Person receive greet:\n    return name\nend\n\
                             on Person receive greet:\n    return name\nend\n";
    let (success, _, error) = check_source(duplicate_message);
    assert!(!success);
    assert!(error.contains("message `greet` is already defined for `Person`"));

    let unknown_field = "type Person:\n    name as String\nend\n\
                         on Person receive greet:\n    return nickname\nend\n";
    let (success, _, error) = check_source(unknown_field);
    assert!(!success);
    assert!(error.contains("unknown variable `nickname`"));
}

#[test]
fn rejects_unknown_messages_and_wrong_message_argument_counts() {
    let unknown = "type Person:\n    name as String\nend\nperson is Person(\"Ada\")\n\
                   person :: fly\n";
    let (success, _, error) = check_source(unknown);
    assert!(!success);
    assert!(error.contains("message `fly` is not defined for `Person`"));
    let (success, error) = run_source(unknown);
    assert!(!success);
    assert!(error.contains("message `fly` is not understood by `Person`"));
    assert!(error.contains("5:1"));

    for (call, expected) in [
        (
            "person :: rename()",
            "message `rename` expects 1 arguments, got 0",
        ),
        (
            "person :: rename(\"A\", \"B\")",
            "message `rename` expects 1 arguments, got 2",
        ),
    ] {
        let source = format!(
            "type Person:\n    name as String\nend\n\
             on Person receive rename(value):\n    return value\nend\n\
             person is Person(\"Ada\")\n{call}\n"
        );
        let (success, _, error) = check_source(&source);
        assert!(!success, "invalid message arity passed checking");
        assert!(error.contains(expected), "missing {expected:?}: {error}");
    }
}

#[test]
fn struct_fields_are_not_exposed_as_dot_access_or_automatic_messages() {
    for source in [
        "type Person:\n    name as String\nend\nperson is Person(\"Ada\")\nSayln person.name\n",
        "type Person:\n    name as String\nend\nperson is Person(\"Ada\")\nperson :: name\n",
    ] {
        let (success, _, error) = check_source(source);
        assert!(!success);
        assert!(
            error.contains("value has no field `name`")
                || error.contains("message `name` is not defined for `Person`"),
            "unexpected field-access result: {error}"
        );
    }
}

#[test]
fn validates_struct_field_types_at_runtime_without_static_analysis() {
    let source = "type Person:\n    age as Int\nend\nperson is Person(\"old\")\n";
    let (success, error) = run_source(source);
    assert!(!success);
    assert!(error.contains("expected Int"), "{error}");
    assert!(error.contains("found String"), "{error}");

    let nominal = "type Person:\n    name as String\nend\n\
                   type User:\n    person as User\nend\n\
                   person is Person(\"Ada\")\nuser is User(person)\n";
    let (success, error) = run_source(nominal);
    assert!(!success);
    assert!(error.contains("expected User"));
    assert!(error.contains("found Person"));
}

#[test]
fn messages_mutate_persistent_state_and_return_values_as_expressions() {
    let source = "type Counter:\n    value as Int\nend\n\
                  on Counter receive increment:\n\
                      value -> value + 1\n\
                      return value\n\
                  end\n\
                  on Counter receive get:\n    return value\nend\n\
                  fn calculate(number as Int) gives Int:\n\
                      return number * 2\n\
                  end\n\
                  counter is Counter(0)\n\
                  other is counter\n\
                  first is counter :: increment\n\
                  second is counter :: increment()\n\
                  third is counter :: increment\n\
                  result is calculate(counter :: get)\n\
                  Sayln first\nSayln second\nSayln third\n\
                  Sayln result\nSayln other :: get\n\
                  Sayln counter :: get()\n";
    let (success, output) = run_source_stdout(source);
    assert!(success, "stateful message execution failed: {output}");
    assert_eq!(output, "1\n2\n3\n6\n3\n3\n");
}

#[test]
fn messages_can_mutate_typed_fields_and_accept_arguments() {
    let source = "type Person:\n    name as String\n    age as Int\nend\n\
                  on Person receive rename(new_name as String):\n\
                      name -> new_name\n\
                  end\n\
                  on Person receive birthday:\n    age -> age + 1\nend\n\
                  on Person receive get_name:\n    return name\nend\n\
                  on Person receive get_age:\n    return age\nend\n\
                  person is Person(\"Budi\", 17)\n\
                  person :: rename(\"Andi\")\n\
                  person :: birthday\n\
                  name is person :: get_name\n\
                  age is person :: get_age\n\
                  Sayln name\nSayln age\n";
    let (success, output) = run_source_stdout(source);
    assert!(success, "typed message mutation failed: {output}");
    assert_eq!(output, "Andi\n18\n");
}

#[test]
fn message_state_mutations_preserve_declared_field_types() {
    let invalid = "type Counter:\n    value as Int\nend\n\
                   on Counter receive corrupt:\n    value -> \"bad\"\nend\n";
    let (success, _, error) = check_source(invalid);
    assert!(!success);
    assert!(error.contains("expected Int, found String"));

    let runtime = "type Counter:\n    value as Int\nend\n\
                   on Counter receive corrupt:\n    value -> \"bad\"\nend\n\
                   counter is Counter(0)\ncounter :: corrupt\n";
    let (success, error) = run_source(runtime);
    assert!(!success);
    assert!(error.contains("type String"), "{error}");
    assert!(error.contains("remains type Int"), "{error}");
}

#[test]
fn nested_collection_mutations_in_message_state_persist() {
    let source = "type Bag:\n    items as List[Int]\nend\n\
                  on Bag receive add(item as Int):\n    items add item\nend\n\
                  on Bag receive get:\n    return items\nend\n\
                  bag is Bag(list [1])\n\
                  alias is bag\n\
                  bag :: add(2)\n\
                  Sayln alias :: get\n";
    let (success, output) = run_source_stdout(source);
    assert!(success, "nested collection state did not persist: {output}");
    assert_eq!(output, "[1, 2]\n");
}
