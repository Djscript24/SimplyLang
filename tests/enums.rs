mod common;
use common::*;

#[test]
fn constructs_unit_and_payload_variants_and_matches_them() {
    let source = "enum Result:\n    Ok as Int\n    Error as String\nend\n\
                  enum State:\n    Ready\n    Running\nend\n\
                  result is Result::Ok(42)\n\
                  state is State::Ready\n\
                  match result:\n\
                      Result::Ok(value):\n\
                          Sayln value\n\
                      Result::Error(message):\n\
                          Sayln message\n\
                  end\n\
                  match state:\n\
                      State::Ready:\n\
                          Sayln \"ready\"\n\
                      State::Running:\n\
                          Sayln \"running\"\n\
                  end\n";
    let (success, output) = run_source_stdout(source);
    assert!(success, "enum matching failed: {output}");
    assert_eq!(output, "42\nready\n");
}

#[test]
fn unknown_runtime_enum_variant_has_a_specific_diagnostic() {
    let (success, error) = run_source(
        "enum State:\n\
             Ready\n\
         end\n\
         Sayln State::Missing\n",
    );

    assert!(!success);
    assert!(
        error.contains("unknown variant `State::Missing`"),
        "{error}"
    );
    assert!(
        error.contains("error[E.runtime.enum.variant-unknown]"),
        "{error}"
    );
}

#[test]
fn match_is_an_expression_and_supports_wildcards() {
    let source = "enum Result:\n    Ok as Int\n    Error as String\nend\n\
                  result is Result::Ok(21)\n\
                  doubled is match result:\n\
                      Result::Ok(value):\n\
                          value * 2\n\
                      _:\n\
                          0\n\
                  end\n\
                  Sayln doubled\n";
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "match expression failed: {output} {}",
        run_source(source).1
    );
    assert_eq!(output, "42\n");
}

#[test]
fn message_return_values_can_be_matched() {
    let source = "enum Result:\n    Ok as Int\n    Error as String\nend\n\
                  type Worker:\n    code as Int\nend\n\
                  on Worker receive result:\n\
                      return Result::Ok(code)\n\
                  end\n\
                  worker is Worker(7)\nresult is worker :: result\n\
                  match result:\n\
                      Result::Ok(value):\n\
                          Sayln value\n\
                      Result::Error(message):\n\
                          Sayln message\n\
                  end\n";
    let (success, output) = run_source_stdout(source);
    assert!(success, "matching message result failed: {output}");
    assert_eq!(output, "7\n");
}

#[test]
fn struct_payloads_preserve_nominal_type_and_shared_instance_identity() {
    let source = "type User:\n    name as String\n    age as Int\nend\n\
                  on User receive rename(new_name as String):\n\
                      name -> new_name\n\
                  end\n\
                  on User receive describe:\n    return name\nend\n\
                  enum Response:\n    Success as User\n    Error as String\nend\n\
                  user is User(\"Budi\", 17)\n\
                  response is Response::Success(user)\n\
                  user :: rename(\"Andi\")\n\
                  message is match response:\n\
                      Response::Success(person):\n\
                          person :: describe\n\
                      Response::Error(error):\n\
                          error\n\
                  end\n\
                  Sayln message\n";
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "struct payload integration failed: {output} {}",
        run_source(source).1
    );
    assert_eq!(output, "Andi\n");
}

#[test]
fn functions_return_structs_and_enums_and_collections_store_them() {
    let source = "type User:\n    name as String\nend\n\
                  enum Result:\n    Ok as Int\n    Error as String\nend\n\
                  fn make_user() gives User:\n    return User(\"Ada\")\nend\n\
                  fn make_result() gives Result:\n    return Result::Ok(42)\nend\n\
                  on User receive name:\n    return name\nend\n\
                  user is make_user()\nresult is make_result()\n\
                  users is [user]\n\
                  results is [result, Result::Error(\"failed\")]\n\
                  Sayln users[0] :: name\n\
                  match results[0]:\n\
                      Result::Ok(value):\n    Sayln value\n\
                      Result::Error(error):\n    Sayln error\n\
                  end\n";
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "function/collection integration failed: {output} {}",
        run_source(source).1
    );
    assert_eq!(output, "Ada\n42\n");
}

#[test]
fn enum_construction_checks_type_variant_and_payload_shape() {
    for (source, expected) in [
        (
            "enum Result:\n    Ok as Int\nend\nresult is Result::Ok(\"bad\")\n",
            "expected Int, found String",
        ),
        (
            "enum Result:\n    Ok as Int\nend\nresult is Result::Ok\n",
            "expects a payload of type Int",
        ),
        (
            "enum State:\n    Ready\nend\nstate is State::Ready(10)\n",
            "does not accept a payload",
        ),
        (
            "enum Result:\n    Ok as Int\nend\nresult is Result::Unknown\n",
            "unknown variant `Result::Unknown`",
        ),
        ("result is Missing::Value\n", "unknown enum type `Missing`"),
    ] {
        let (success, _, error) = check_source(source);
        assert!(!success, "invalid enum program passed checking");
        assert!(error.contains(expected), "missing {expected:?}: {error}");
    }
}

#[test]
fn enums_reject_duplicate_types_and_variants() {
    let duplicate_variant = "enum State:\n    Ready\n    Ready\nend\n";
    let (success, _, error) = check_source(duplicate_variant);
    assert!(!success);
    assert!(error.contains("duplicate variant `State::Ready`"));

    let duplicate_enum = "enum State:\n    Ready\nend\n\
                         enum State:\n    Running\nend\n";
    let (success, _, error) = check_source(duplicate_enum);
    assert!(!success);
    assert!(error.contains("type `State` is already declared"));
}

#[test]
fn enum_identity_is_nominal_and_match_is_exhaustive() {
    let nominal = "enum A:\n    Value as Int\nend\n\
                   enum B:\n    Value as Int\nend\n\
                   value as B is A::Value(1)\n";
    let (success, _, error) = check_source(nominal);
    assert!(!success);
    assert!(error.contains("expected B, found A"));

    let incomplete = "enum State:\n    Ready\n    Running\nend\n\
                      state is State::Ready\n\
                      match state:\n    State::Ready:\n        1\nend\n";
    let (success, _, error) = check_source(incomplete);
    assert!(!success);
    assert!(error.contains("missing variant `State::Running`"));
}

#[test]
fn match_patterns_validate_payloads_and_branch_result_types() {
    let mismatched_payload = "enum Result:\n    Ok as Int\nend\n\
                              result is Result::Ok(1)\n\
                              match result:\n\
                                  Result::Ok:\n    1\n\
                              end\n";
    let (success, _, error) = check_source(mismatched_payload);
    assert!(!success);
    assert!(error.contains("requires a payload binding"));

    let mismatched_branches = "enum State:\n    Ready\n    Running\nend\n\
                               state is State::Ready\n\
                               value is match state:\n\
                                   State::Ready:\n    1\n\
                                   State::Running:\n    \"running\"\n\
                               end\n";
    let (success, _, error) = check_source(mismatched_branches);
    assert!(!success);
    assert!(error.contains("expected Int, found String"), "{error}");
}

#[test]
fn match_evaluates_scrutinee_once_and_payload_names_are_arm_local() {
    let once = "enum Result:\n    Ok as Int\n    Error as String\nend\n\
                type Counter:\n    value as Int\nend\n\
                on Counter receive next:\n\
                    value -> value + 1\n\
                    return Result::Ok(value)\n\
                end\n\
                counter is Counter(0)\n\
                result is match counter :: next:\n\
                    Result::Ok(value):\n    value\n\
                    Result::Error(message):\n    0\n\
                end\n\
                Sayln result\n\
                Sayln counter :: next\n";
    let (success, output) = run_source_stdout(once);
    assert!(
        success,
        "match scrutinee side effect failed: {output} {}",
        run_source(once).1
    );
    assert_eq!(output, "1\nResult::Ok(2)\n");

    let scope = "enum Result:\n    Ok as Int\n    Error as String\nend\n\
                 result is Result::Ok(1)\n\
                 match result:\n\
                     Result::Ok(value):\n    Sayln value\n\
                     Result::Error(message):\n    Sayln message\n\
                 end\n\
                 Sayln value\n";
    let (success, error) = run_source(scope);
    assert!(!success);
    assert!(error.contains("unknown variable `value`"));
}

#[test]
fn wildcard_patterns_cover_non_enum_values_and_enum_display_is_nominal() {
    let source = "value is match 2:\n    _:\n        3\nend\n\
                  enum A:\n    Item\nend\n\
                  Sayln value\nSayln A::Item\nSayln type_of(A::Item)\n";
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "wildcard match failed: {output} {}",
        run_source(source).1
    );
    assert_eq!(output, "3\nA::Item\nA\n");
}

#[test]
fn enum_equality_compares_nominal_type_variant_and_payload() {
    let source = "enum A:\n    Value as Int\nend\n\
                  enum B:\n    Value as Int\nend\n\
                  Sayln A::Value(1) == A::Value(1)\n\
                  Sayln A::Value(1) == A::Value(2)\n\
                  Sayln A::Value(1) == B::Value(1)\n";
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "enum equality failed: {output} {}",
        run_source(source).1
    );
    assert_eq!(output, "true\nfalse\nfalse\n");
}

#[test]
fn enum_equality_uses_struct_identity_for_payloads_and_nested_collections() {
    let source = "type Person:\n    name as String\nend\n\
                  enum Result:\n    Ok as Person\nend\n\
                  first is Person(\"Alice\")\n\
                  alias is first\n\
                  second is Person(\"Alice\")\n\
                  same_a is Result::Ok(first)\n\
                  same_b is Result::Ok(alias)\n\
                  different is Result::Ok(second)\n\
                  list_a is [[first]]\n\
                  list_b is [[alias]]\n\
                  list_c is [[second]]\n\
                  Sayln same_a == same_b\n\
                  Sayln same_a == different\n\
                  Sayln list_a == list_b\n\
                  Sayln list_a == list_c\n";
    let (success, output) = run_source_stdout(source);

    assert!(success, "nested identity equality failed: {output}");
    assert_eq!(output, "true\nfalse\ntrue\nfalse\n");
}

#[test]
fn enum_equality_recurses_through_nested_enum_and_collection_payloads() {
    let source = "enum State:\n\
                      Ready\n\
                      Running\n\
                  end\n\
                  enum Envelope:\n\
                      StateValue as State\n\
                      States as List[State]\n\
                      Nested as Envelope\n\
                  end\n\
                  first is Envelope::States(list [State::Ready, State::Running])\n\
                  second is Envelope::States(list [State::Ready, State::Running])\n\
                  different is Envelope::States(list [State::Ready])\n\
                  nested_a is Envelope::Nested(Envelope::StateValue(State::Ready))\n\
                  nested_b is Envelope::Nested(Envelope::StateValue(State::Ready))\n\
                  Sayln first == second\n\
                  Sayln first == different\n\
                  Sayln [first] == [second]\n\
                  Sayln nested_a == nested_b\n";
    let (success, output) = run_source_stdout(source);

    assert!(
        success,
        "nested enum equality failed: {output} {}",
        run_source(source).1
    );
    assert_eq!(output, "true\nfalse\ntrue\ntrue\n");
}

#[test]
fn declared_lowercase_enum_names_are_resolved_as_variant_constructors() {
    let source = "enum result:\n    Ok as Int\nend\n\
                  value is result::Ok(42)\n\
                  match value:\n\
                      result::Ok(number):\n    Sayln number\n\
                  end\n";
    let (success, output) = run_source_stdout(source);
    assert!(success, "lowercase enum declaration failed: {output}");
    assert_eq!(output, "42\n");
}

#[test]
fn enum_patterns_cannot_match_struct_or_primitive_values() {
    let source = "enum State:\n    Ready\nend\n\
                  value is 1\n\
                  match value:\n\
                      State::Ready:\n    1\n\
                  end\n";
    let (success, _, error) = check_source(source);
    assert!(!success);
    assert!(error.contains("cannot match value of type Int"), "{error}");
}

#[test]
fn enum_values_reject_message_dispatch_at_runtime() {
    let source = "enum State:\n    Ready\nend\n\
                  fn greet(value):\n    return \"hello\"\nend\n\
                  state is State::Ready\nstate :: greet\n";
    let (success, error) = run_source(source);
    assert!(!success);
    assert!(
        error.contains("enum values do not support message dispatch")
            || error.contains("enum value `State::Ready` does not support message dispatch"),
        "{error}"
    );
}
