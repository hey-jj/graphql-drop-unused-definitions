//! Fragment reachability cases.
//!
//! These exercise the transitive closure: spreads chained through fragments,
//! spreads buried in nested fields and inline fragments, shared and diamond
//! shapes, and cycles. Expected outputs come from the same reduction graphql-js
//! performs.

use graphql_drop_unused_definitions::{drop_unused_definitions, parse, print};

fn run(src: &str, op: &str) -> String {
    let doc = parse(src).expect("input parses");
    print(&drop_unused_definitions(&doc, op))
}

#[test]
fn transitive_chain_keeps_all_links() {
    // Q -> A -> B -> C. D is never referenced and is dropped.
    let out = run(
        "query Q { ...A }\n\
         fragment A on Query { ...B }\n\
         fragment B on Query { ...C }\n\
         fragment C on Query { c }\n\
         fragment D on Query { d }",
        "Q",
    );
    assert_eq!(
        out,
        "query Q {\n  ...A\n}\n\n\
         fragment A on Query {\n  ...B\n}\n\n\
         fragment B on Query {\n  ...C\n}\n\n\
         fragment C on Query {\n  c\n}",
    );
}

#[test]
fn spread_inside_nested_field_is_reached() {
    let out = run(
        "query Q { obj { ...Frag } }\nfragment Frag on Obj { x }",
        "Q",
    );
    assert_eq!(
        out,
        "query Q {\n  obj {\n    ...Frag\n  }\n}\n\nfragment Frag on Obj {\n  x\n}",
    );
}

#[test]
fn spread_inside_inline_fragment_is_reached() {
    let out = run(
        "query Q { ... on Query { ...Frag } }\nfragment Frag on Query { x }",
        "Q",
    );
    assert_eq!(
        out,
        "query Q {\n  ... on Query {\n    ...Frag\n  }\n}\n\nfragment Frag on Query {\n  x\n}",
    );
}

#[test]
fn shared_fragment_kept_once_dropped_only_removed() {
    // Both ops spread Shared. Drop also spreads DropOnly. Selecting Keep keeps
    // Shared and removes DropOnly and the Drop operation.
    let out = run(
        "query Keep { ...Shared }\n\
         query Drop { ...Shared ...DropOnly }\n\
         fragment Shared on Query { s }\n\
         fragment DropOnly on Query { d }",
        "Keep",
    );
    assert_eq!(
        out,
        "query Keep {\n  ...Shared\n}\n\nfragment Shared on Query {\n  s\n}",
    );
}

#[test]
fn diamond_keeps_shared_target_once() {
    // Q -> A, Q -> B, A -> C, B -> C. C appears once in output.
    let out = run(
        "query Q { ...A ...B }\n\
         fragment A on Query { ...C }\n\
         fragment B on Query { ...C }\n\
         fragment C on Query { c }",
        "Q",
    );
    assert_eq!(
        out,
        "query Q {\n  ...A\n  ...B\n}\n\n\
         fragment A on Query {\n  ...C\n}\n\n\
         fragment B on Query {\n  ...C\n}\n\n\
         fragment C on Query {\n  c\n}",
    );
}

#[test]
fn self_referential_fragment_terminates() {
    let out = run("query Q { ...A }\nfragment A on Query { ...A x }", "Q");
    assert_eq!(
        out,
        "query Q {\n  ...A\n}\n\nfragment A on Query {\n  ...A\n  x\n}",
    );
}

#[test]
fn mutually_recursive_fragments_terminate() {
    let out = run(
        "query Q { ...A }\n\
         fragment A on Query { ...B }\n\
         fragment B on Query { ...A x }",
        "Q",
    );
    assert_eq!(
        out,
        "query Q {\n  ...A\n}\n\n\
         fragment A on Query {\n  ...B\n}\n\n\
         fragment B on Query {\n  ...A\n  x\n}",
    );
}

#[test]
fn dangling_spread_is_ignored_without_error() {
    // ...Missing has no fragment definition. It stays in the operation text but
    // adds no definition to the output.
    let out = run("query Q { ...Missing y }", "Q");
    assert_eq!(out, "query Q {\n  ...Missing\n  y\n}");
}
