//! String-value printing cases.
//!
//! `print` renders every string value as a canonical single-line GraphQL
//! string: double quotes, escaped control characters, no block-string form.
//! Each expected output matches a canonical GraphQL printer byte for byte.
//!
//! The parser decodes a block string such as `"""hi"""` to the same value as
//! `"hi"`, so the printed form is the non-block string in both cases. A test
//! below records that.

use graphql_drop_unused_definitions::{drop_unused_definitions, parse, print};

struct Case {
    name: &'static str,
    src: &'static str,
    expected: &'static str,
}

// The `src` strings carry GraphQL escape sequences. In a Rust literal `\\n` is
// the two source characters backslash and n, which GraphQL decodes to a newline.
const CASES: &[Case] = &[
    Case {
        name: "newline string stays single line",
        src: "query Q { f(a: \"a\\nb\") }",
        expected: "query Q {\n  f(a: \"a\\nb\")\n}",
    },
    Case {
        name: "mixed newline and tab escapes",
        src: "query Q { f(a: \"a\\nb\\tc\") }",
        expected: "query Q {\n  f(a: \"a\\nb\\tc\")\n}",
    },
    Case {
        name: "plain string",
        src: "query Q { f(a: \"plain\") }",
        expected: "query Q {\n  f(a: \"plain\")\n}",
    },
    Case {
        name: "embedded quote is escaped",
        src: "query Q { f(a: \"has\\\"quote\") }",
        expected: "query Q {\n  f(a: \"has\\\"quote\")\n}",
    },
    Case {
        name: "backslash is escaped",
        src: "query Q { f(a: \"back\\\\slash\") }",
        expected: "query Q {\n  f(a: \"back\\\\slash\")\n}",
    },
    Case {
        name: "tab only",
        src: "query Q { f(a: \"\\tonly\") }",
        expected: "query Q {\n  f(a: \"\\tonly\")\n}",
    },
    Case {
        name: "empty string",
        src: "query Q { f(a: \"\") }",
        expected: "query Q {\n  f(a: \"\")\n}",
    },
    Case {
        name: "string inside a list argument",
        src: "query Q { f(a: [\"x\\ny\", \"z\"]) }",
        expected: "query Q {\n  f(a: [\"x\\ny\", \"z\"])\n}",
    },
    Case {
        name: "string inside an object argument",
        src: "query Q { f(a: {k: \"v\\nw\"}) }",
        expected: "query Q {\n  f(a: {k: \"v\\nw\"})\n}",
    },
    Case {
        name: "string in a variable default value",
        src: "query Q($v: String = \"d\\ne\") { f }",
        expected: "query Q($v: String = \"d\\ne\") {\n  f\n}",
    },
    Case {
        name: "string in a directive argument",
        src: "query Q { f @dir(note: \"l1\\nl2\") }",
        expected: "query Q {\n  f @dir(note: \"l1\\nl2\")\n}",
    },
];

#[test]
fn string_values_print_canonically() {
    for case in CASES {
        let doc = parse(case.src).expect("input parses");
        let out = print(&drop_unused_definitions(&doc, "Q"));
        assert_eq!(out, case.expected, "case `{}` diverged", case.name);
    }
}

#[test]
fn control_character_uses_uppercase_unicode_escape() {
    // U+001F has no short escape, so it prints as the escape sequence backslash
    // u 0 0 1 F with uppercase hex. The source carries the same escape.
    let doc = parse("query Q { f(a: \"x\\u001Fy\") }").expect("input parses");
    let out = print(&drop_unused_definitions(&doc, "Q"));
    assert_eq!(out, "query Q {\n  f(a: \"x\\u001Fy\")\n}");
}

#[test]
fn string_value_matching_a_placeholder_token_is_preserved() {
    // A string value whose text equals an internal placeholder token must print
    // verbatim. The substitution pass must not rewrite it into another value.
    let doc = parse("query Q { f(a: \"hello\", b: \"__GQL_STRING_PLACEHOLDER_0__\") }")
        .expect("input parses");
    let out = print(&drop_unused_definitions(&doc, "Q"));
    assert_eq!(
        out,
        "query Q {\n  f(a: \"hello\", b: \"__GQL_STRING_PLACEHOLDER_0__\")\n}"
    );
}

#[test]
fn block_string_decodes_to_the_same_value_as_a_plain_string() {
    // The parser strips block-string syntax during decode, so a block string and
    // the plain string with the same content are indistinguishable in the AST
    // and print identically.
    let block = parse("query Q { f(x: \"\"\"hi\"\"\") }").expect("input parses");
    let plain = parse("query Q { f(x: \"hi\") }").expect("input parses");
    assert_eq!(
        print(&drop_unused_definitions(&block, "Q")),
        print(&drop_unused_definitions(&plain, "Q")),
    );
}
