//! The no-op fallback and keying edge cases.
//!
//! When no operation matches the requested name, the whole document is returned
//! unchanged. These cases also pin operation-name keying, operation types, and
//! last-writer-wins on duplicate names. Expected outputs match graphql-js.

use graphql_drop_unused_definitions::{drop_unused_definitions, parse, print};

fn run(src: &str, op: &str) -> String {
    let doc = parse(src).expect("input parses");
    print(&drop_unused_definitions(&doc, op))
}

#[test]
fn unknown_name_returns_whole_document() {
    let out = run("query Only { x }", "Missing");
    assert_eq!(out, "query Only {\n  x\n}");
}

#[test]
fn empty_string_against_named_only_doc_is_not_found() {
    // The only operation is named, so there is no "" key. Selecting "" falls
    // back to the original document.
    let out = run("query Named { x }", "");
    assert_eq!(out, "query Named {\n  x\n}");
}

#[test]
fn named_lookup_against_anonymous_only_doc_is_not_found() {
    // The only operation is anonymous, keyed by "". A named lookup falls back.
    let out = run("{ x }", "Named");
    assert_eq!(out, "{\n  x\n}");
}

#[test]
fn anonymous_operation_selected_by_empty_string() {
    let out = run("{ x }", "");
    assert_eq!(out, "{\n  x\n}");
}

#[test]
fn bare_query_keyword_is_anonymous() {
    // A `query { ... }` with no name keys to "" like the shorthand form.
    let out = run("query { x }", "");
    assert_eq!(out, "{\n  x\n}");
}

#[test]
fn mutation_keyword_preserved() {
    let out = run("mutation Foo { bar }", "Foo");
    assert_eq!(out, "mutation Foo {\n  bar\n}");
}

#[test]
fn subscription_keyword_preserved() {
    let out = run("subscription Foo { bar }", "Foo");
    assert_eq!(out, "subscription Foo {\n  bar\n}");
}

#[test]
fn variables_directives_and_arguments_round_trip() {
    let out = run("query Q($id: ID!) @dir { field(x: $id) }", "Q");
    assert_eq!(out, "query Q($id: ID!) @dir {\n  field(x: $id)\n}");
}

#[test]
fn duplicate_operation_name_keeps_last() {
    // Two operations named Dup. The later one wins.
    let out = run("query Dup { a }\nquery Dup { b }", "Dup");
    assert_eq!(out, "query Dup {\n  b\n}");
}

#[test]
fn multiple_anonymous_operations_keep_last() {
    // Both anonymous operations key to "". The later one wins.
    let out = run("{ a }\n{ b }", "");
    assert_eq!(out, "{\n  b\n}");
}

#[test]
fn fallback_keeps_all_definitions_in_order() {
    let src = "query Keep { ...KeptFragment }\n\
               fragment KeptFragment on Query { abc }\n\
               query AlsoKeep { ...AlsoKeptFragment }\n\
               fragment AlsoKeptFragment on Query { def }";
    let doc = parse(src).unwrap();
    // Unknown name returns the same content the document prints to.
    assert_eq!(
        print(&drop_unused_definitions(&doc, "Unknown")),
        print(&doc),
    );
}
