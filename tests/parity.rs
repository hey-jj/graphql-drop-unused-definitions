//! The five canonical cases for `drop_unused_definitions`.
//!
//! Each row pairs a GraphQL input with the exact printed output. Inputs keep the
//! leading comment and indentation of the source snapshots to prove the parser
//! and printer normalize them the same way.

use graphql_drop_unused_definitions::{drop_unused_definitions, parse, print};

struct Case {
    name: &'static str,
    src: &'static str,
    op: &'static str,
    expected: &'static str,
}

const CASES: &[Case] = &[
    Case {
        name: "anonymous operation",
        src: "#graphql\n      {abc}\n    ",
        op: "",
        expected: "{\n  abc\n}",
    },
    Case {
        name: "named operation",
        src: "#graphql\n      query MyQuery {abc}\n    ",
        op: "MyQuery",
        expected: "query MyQuery {\n  abc\n}",
    },
    Case {
        name: "multiple operations",
        src: "#graphql\n      query Keep { abc }\n      query Drop { def }\n    ",
        op: "Keep",
        expected: "query Keep {\n  abc\n}",
    },
    Case {
        name: "includes only used fragments",
        src: "#graphql\n      query Drop { ...DroppedFragment }\n      \
               fragment DroppedFragment on Query { abc }\n      \
               query Keep { ...KeptFragment }\n      \
               fragment KeptFragment on Query { def }\n    ",
        op: "Keep",
        expected:
            "query Keep {\n  ...KeptFragment\n}\n\nfragment KeptFragment on Query {\n  def\n}",
    },
    Case {
        name: "preserves entire document when operation isn't found",
        src: "#graphql\n      query Keep { ...KeptFragment }\n      \
               fragment KeptFragment on Query { abc }\n      \
               query AlsoKeep { ...AlsoKeptFragment }\n      \
               fragment AlsoKeptFragment on Query { def }\n    ",
        op: "Unknown",
        expected: "query Keep {\n  ...KeptFragment\n}\n\n\
                   fragment KeptFragment on Query {\n  abc\n}\n\n\
                   query AlsoKeep {\n  ...AlsoKeptFragment\n}\n\n\
                   fragment AlsoKeptFragment on Query {\n  def\n}",
    },
];

#[test]
fn canonical_cases() {
    for case in CASES {
        let doc = parse(case.src).expect("input parses");
        let out = print(&drop_unused_definitions(&doc, case.op));
        assert_eq!(out, case.expected, "case `{}` diverged", case.name);
    }
}

#[test]
fn readme_example() {
    let doc = parse(
        "query Drop { ...DroppedFragment }\n\
         fragment DroppedFragment on Query { abc }\n\
         query Keep { ...KeptFragment }\n\
         fragment KeptFragment on Query { def }",
    )
    .unwrap();
    assert_eq!(
        print(&drop_unused_definitions(&doc, "Keep")),
        "query Keep {\n  ...KeptFragment\n}\n\nfragment KeptFragment on Query {\n  def\n}",
    );
}
