//! Regression guards for definition order, duplicate fragment names, and the
//! type-system definition scope.
//!
//! These cases pass today. They pin behavior the spec calls out so a refactor
//! cannot drift without a failing test. Expected strings match the reduction a
//! canonical GraphQL printer produces.

use graphql_drop_unused_definitions::{drop_unused_definitions, parse, print};

fn run(src: &str, op: &str) -> String {
    let doc = parse(src).expect("input parses");
    print(&drop_unused_definitions(&doc, op))
}

#[test]
fn duplicate_fragment_names_both_kept() {
    // A fragment defined twice under the same name has both copies pass the
    // filter, because the filter tests fragment name membership and keeps every
    // matching definition.
    let out = run(
        "query Q { ...F }\n\
         fragment F on A { a }\n\
         fragment F on B { b }",
        "Q",
    );
    assert_eq!(
        out,
        "query Q {\n  ...F\n}\n\n\
         fragment F on A {\n  a\n}\n\n\
         fragment F on B {\n  b\n}",
    );
}

#[test]
fn fragment_defined_before_its_operation_is_reached() {
    // Output order follows source order, so the fragment prints before the
    // operation. Reachability does not depend on definition order.
    let out = run("fragment F on A { a }\nquery Q { ...F }", "Q");
    assert_eq!(out, "fragment F on A {\n  a\n}\n\nquery Q {\n  ...F\n}",);
}

#[test]
fn reachable_fragment_preceding_its_dependency_is_kept() {
    // B is defined before both the operation and A that reaches it. B is still
    // kept, and every definition holds its source position.
    let out = run(
        "fragment B on Q { b }\n\
         query Q { ...A }\n\
         fragment A on Q { ...B }",
        "Q",
    );
    assert_eq!(
        out,
        "fragment B on Q {\n  b\n}\n\n\
         query Q {\n  ...A\n}\n\n\
         fragment A on Q {\n  ...B\n}",
    );
}

// Type-system definitions (type, schema, directive, and the rest) are out of
// scope. `parse` accepts executable documents only, so such a document never
// reaches `drop_unused_definitions`. These cases lock that contract in: the
// input is rejected at parse time rather than silently mishandled.
#[test]
fn type_definition_input_is_rejected_at_parse() {
    assert!(parse("type Foo { a: Int }\nquery Q { x }").is_err());
}

#[test]
fn schema_definition_input_is_rejected_at_parse() {
    assert!(parse("schema { query: Q }\nquery Q { x }").is_err());
}

#[test]
fn directive_definition_input_is_rejected_at_parse() {
    assert!(parse("directive @d on FIELD\nquery Q { x @d }").is_err());
}
