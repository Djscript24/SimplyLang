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
fn checks_message_argument_and_inferred_return_types() {
    let argument_source = "type Person:\n    name as String\nend\n\
                          on Person receive rename(new_name as String):\n\
                              name -> new_name\n\
                          end\n\
                          person is Person(\"Ada\")\nperson :: rename(1)\n";
    let (success, _, error) = check_source(argument_source);
    assert!(!success);
    assert!(error.contains("expected String, found Int"), "{error}");

    let return_source = "type Person:\n    name as String\nend\n\
                         on Person receive get_name:\n    return name\nend\n\
                         person is Person(\"Ada\")\nvalue as Int is person :: get_name\n";
    let (success, _, error) = check_source(return_source);
    assert!(!success);
    assert!(error.contains("expected Int, found String"), "{error}");
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
fn messages_can_dispatch_to_another_message_on_the_same_receiver() {
    let source = "type Counter:\n    value as Int\nend\n\
                  counter is Counter(0)\n\
                  on Counter receive get:\n    return value\nend\n\
                  on Counter receive outer:\n\
                      value -> value + 1\n\
                      previous is counter :: get\n\
                      value -> value + 1\n\
                      return previous + value\n\
                  end\n\
                  Sayln counter :: outer\n\
                  Sayln counter :: get\n";
    let (success, output) = run_source_stdout(source);
    assert!(success, "nested message invocation failed: {output}");
    assert_eq!(output, "3\n2\n");
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
fn struct_equality_uses_instance_identity() {
    let source = "type Person:\n    name as String\nend\n\
                  on Person receive rename(next as String):\n\
                      name -> next\n\
                  end\n\
                  first is Person(\"Alice\")\n\
                  second is Person(\"Alice\")\n\
                  alias is first\n\
                  first :: rename(\"Alicia\")\n\
                  second :: rename(\"Alicia\")\n\
                  Sayln first == second\n\
                  Sayln first == alias\n";
    let (success, output) = run_source_stdout(source);

    assert!(success, "struct equality failed: {output}");
    assert_eq!(output, "false\ntrue\n");
}

#[test]
fn struct_identity_equality_terminates_for_cyclic_instances() {
    let source = "type Node:\n    links as List[Node]\nend\n\
                  on Node receive link(other as Node):\n\
                      links add other\n\
                  end\n\
                  first is Node(list [])\n\
                  second is Node(list [])\n\
                  first :: link(first)\n\
                  second :: link(second)\n\
                  Sayln first == second\n\
                  Sayln first == first\n";
    let (success, output) = run_source_stdout(source);

    assert!(success, "cyclic struct equality failed: {output}");
    assert_eq!(output, "false\ntrue\n");
}

#[test]
fn recursive_message_dispatch_is_bounded() {
    let source = "type Counter:\n    value as Int\nend\n\
                  counter is Counter(0)\n\
                  on Counter receive recurse(amount as Int):\n\
                      return counter :: recurse(amount)\n\
                  end\n\
                  counter :: recurse(0)\n";
    let (success, error) = run_source(source);
    assert!(!success, "recursive message dispatch did not stop");
    assert!(
        error.contains("function call depth exceeds the limit"),
        "{error}"
    );
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

#[test]
fn message_receivers_respect_borrows_of_collection_fields() {
    let source = r#"type Box:
    items as List[Int]
end
on Box receive first:
    return items[0]
end
on Box receive add(item as Int):
    items add item
end
mut values is list [1]
box is Box(values)
fn read_while_exclusive(mut ref active as List[Int]):
    return box :: first
end
Sayln read_while_exclusive(values)
"#;
    let (success, error) = run_source(source);
    assert!(
        !success,
        "a message read bypassed an exclusive borrow of a field alias"
    );
    assert!(
        error.contains("error[E.semantic.ref.invalid]"),
        "missing borrow-conflict diagnostic: {error}"
    );

    let source = r#"type Box:
    items as List[Int]
end
on Box receive add(item as Int):
    items add item
end
mut values is list [1]
box is Box(values)
fn mutate_while_shared(ref active as List[Int]):
    box :: add(2)
end
mutate_while_shared(values)
"#;
    let (success, error) = run_source(source);
    assert!(
        !success,
        "a mutating message bypassed a shared borrow of a field alias"
    );
    assert!(
        error.contains("error[E.semantic.ref.invalid]"),
        "missing borrow-conflict diagnostic: {error}"
    );
}

#[test]
fn struct_fields_can_be_borrowed_through_bracket_paths() {
    let source = r#"type Stats:
    score as Int
end
type Profile:
    name as String
    stats as Stats
    scores as List[Int]
end
mut profile is Profile("Ada", Stats(7), list [2, 3])
name is ref profile["name"]
mut score is ref profile["stats"]["score"]
mut first_score is ref profile["scores"][0]
Sayln name
score -> 9
first_score -> 8
Sayln profile["stats"]["score"]
Sayln profile["scores"][0]
"#;
    let (success, output) = run_source_stdout(source);
    assert!(success, "nested Struct field borrowing failed: {output}");
    assert_eq!(output, "Ada\n9\n8\n");
}

#[test]
fn struct_field_borrows_detect_aliases_and_block_message_mutation() {
    let source = r#"type Person:
    age as Int
end
mut person is Person(1)
mut alias is person
fn conflict():
    mut first is ref person["age"]
    mut second is ref alias["age"]
end
conflict()
"#;
    let (success, error) = run_source(source);
    assert!(
        !success,
        "aliased Struct fields were borrowed exclusively twice"
    );
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");
    assert!(error.contains("conflicting borrow"), "{error}");
    assert!(
        error.contains("8:5"),
        "diagnostic must locate the conflicting borrow: {error}"
    );

    let source = r#"type Person:
    age as Int
end
on Person receive set_age(next as Int):
    age -> next
end
mut person is Person(1)
age is ref person["age"]
person :: set_age(4)
"#;
    let (success, error) = run_source(source);
    assert!(!success, "message mutation bypassed a shared field borrow");
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");
    assert!(error.contains("mutation of Struct field `age`"), "{error}");
    assert!(
        error.contains("5:5"),
        "diagnostic must locate the conflicting field mutation: {error}"
    );

    let source = r#"type Person:
    name as String
    age as Int
end
on Person receive rename(next as String):
    name -> next
end
on Person receive read_name:
    return name
end
mut person is Person("Ada", 37)
age is ref person["age"]
Sayln person :: read_name
person :: rename("Grace")
Sayln age
Sayln person["name"]
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "disjoint field message access conflicted with a shared field borrow: {output}"
    );
    assert_eq!(output, "Ada\n37\nGrace\n");
}

#[test]
fn struct_field_borrows_allow_disjoint_fields_and_reject_whole_object_overlap() {
    let source = r#"type Person:
    age as Int
    score as Int
end
mut person is Person(1, 10)
mut age is ref person["age"]
mut score is ref person["score"]
age -> 2
score -> 20
Sayln person["age"]
Sayln person["score"]
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "disjoint Struct fields could not be borrowed simultaneously: {output}"
    );
    assert_eq!(output, "2\n20\n");

    let source = r#"type Person:
    age as Int
end
mut person is Person(1)
mut whole is ref person
mut age is ref person["age"]
"#;
    let (success, error) = run_source(source);
    assert!(!success, "whole-Struct and field borrows did not conflict");
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");

    let source = r#"type Stats:
    score as Int
end
type Profile:
    stats as Stats
end
mut profile is Profile(Stats(1))
mut stats is profile["stats"]
fn conflict():
    mut first is ref profile["stats"]["score"]
    mut second is ref stats["score"]
end
conflict()
"#;
    let (success, error) = run_source(source);
    assert!(
        !success,
        "aliases reaching the same nested Struct field through different paths did not conflict"
    );
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");
}

#[test]
fn message_field_access_is_conservative_for_exclusive_and_specific_for_shared_borrows() {
    let source = r#"type Pair:
    a as Int
    b as Int
end
on Pair receive read_a:
    return a
end
on Pair receive read_b:
    return b
end
on Pair receive set_a(value as Int):
    a -> value
end
on Pair receive set_b(value as Int):
    b -> value
end
on Pair receive nested_read_a:
    return pair :: read_a
end
on Pair receive nested_write_a:
    pair :: set_a(8)
end
mut pair is Pair(1, 2)
a is ref pair["a"]
Sayln pair :: read_b
pair :: set_b(3)
Sayln a
Sayln pair["b"]
Sayln pair :: nested_read_a
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "shared field borrow should permit unrelated field accesses: {output}"
    );
    assert_eq!(output, "2\n1\n3\n1\n");

    let source = r#"type Pair:
    a as Int
    b as Int
end
on Pair receive read_b:
    return b
end
mut pair is Pair(1, 2)
mut a is ref pair["a"]
pair :: read_b
"#;
    let (success, error) = run_source(source);
    assert!(
        !success,
        "Struct message dispatch should conservatively reject an exclusive field borrow"
    );
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");
    assert!(error.contains("active exclusive borrow"), "{error}");

    let source = r#"type Pair:
    a as Int
    b as Int
end
on Pair receive set_a(value as Int):
    a -> value
end
on Pair receive nested_write_a:
    pair :: set_a(8)
end
mut pair is Pair(1, 2)
a is ref pair["a"]
pair :: nested_write_a
"#;
    let (success, error) = run_source(source);
    assert!(
        !success,
        "nested message mutation bypassed a shared borrow of the same field"
    );
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");
    assert!(error.contains("mutation of Struct field `a`"), "{error}");

    let source = r#"type Pair:
    items as List[Int]
    count as Int
end
on Pair receive read_count:
    return count
end
on Pair receive set_count(value as Int):
    count -> value
end
mut pair is Pair(list [1], 2)
items is ref pair["items"]
Sayln pair :: read_count
pair :: set_count(3)
Sayln items[0]
Sayln pair["count"]
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "shared nested-collection borrow should permit unrelated field access: {output}"
    );
    assert_eq!(output, "2\n1\n3\n");

    let source = r#"type Pair:
    items as List[Int]
    count as Int
end
on Pair receive read_count:
    return count
end
mut pair is Pair(list [1], 2)
mut items is ref pair["items"]
pair :: read_count
"#;
    let (success, error) = run_source(source);
    assert!(
        !success,
        "an unrelated message bypassed the receiver identity of an exclusive nested borrow"
    );
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");
    assert!(error.contains("active exclusive borrow"), "{error}");
}

#[test]
fn whole_object_field_borrows_block_replacement_and_allow_field_local_mutation() {
    let source = r#"type Box:
    items as List[Int]
end
fn append(mut ref items as List[Int]):
    items add 3
end
mut box is Box(list [1])
mut items is ref box["items"]
items :: append
Sayln box["items"]
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "forwarding a Struct-field collection borrow failed: {output}"
    );
    assert_eq!(output, "[1, 3]\n");

    let source = r#"type Box:
    items as List[Int]
end
mut box is Box(list [1])
mut items is ref box["items"]
items add 2
Sayln box["items"]
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "exclusive collection field borrow failed: {output}"
    );
    assert_eq!(output, "[1, 2]\n");

    let source = r#"type Person:
    age as Int
end
mut person is Person(1)
age is ref person["age"]
person -> Person(2)
"#;
    let (success, error) = run_source(source);
    assert!(!success, "borrowed Struct owner was reassigned");
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");

    let source = r#"type Box:
    items as List[Int]
end
mut box is Box(list [1])
mut items is ref box["items"]
mut first is ref box["items"][0]
"#;
    let (success, error) = run_source(source);
    assert!(!success, "whole-field and descendant borrows overlapped");
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");

    let source = r#"type Box:
    items as List[Int]
end
on Box receive add(item as Int):
    items add item
end
mut box is Box(list [1])
mut slot is ref box["items"][0]
box :: add(2)
"#;
    let (success, error) = run_source(source);
    assert!(!success, "message mutation bypassed a nested field borrow");
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");
}

#[test]
fn struct_field_borrow_paths_reject_missing_fields_and_cleanup_on_errors() {
    let (success, _, error) = check_source(
        "type Person:\n    age as Int\nend\nperson is Person(1)\nvalue is person[\"missing\"]\n",
    );
    assert!(!success, "unknown Struct field passed semantic checking");
    assert!(error.contains("has no field `missing`"), "{error}");

    let runtime_missing = r#"type Person:
    age as Int
end
person is Person(1)
key is "missing"
Sayln person[key]
"#;
    let (success, error) = run_source(runtime_missing);
    assert!(!success, "unknown dynamic Struct field was accepted");
    assert!(error.contains("unknown field `missing`"), "{error}");

    let source = r#"type Person:
    age as Int
end
mut person is Person(1)
fn update():
    mut age is ref person["age"]
    try:
        age -> "wrong"
    catch error:
        age -> 2
    end
end
update()
Sayln person["age"]
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "failed write damaged the field borrow or its cleanup: {output}"
    );
    assert_eq!(output, "2\n");

    let source = r#"enum Fault:
    Failed
end
type Person:
    age as Int
end
on Person receive fail:
    throw Fault::Failed
end
on Person receive set_age(next as Int):
    age -> next
end
mut person is Person(1)
fn scoped():
    age is ref person["age"]
    try:
        person :: fail
    catch error:
        Sayln age
    end
end
scoped()
person :: set_age(4)
Sayln person["age"]
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "message error or scope exit left a field borrow active: {output}"
    );
    assert_eq!(output, "1\n4\n");
}
