//! Property tests over generated documents.
//!
//! Each case builds a document with one named query plus a set of fragments and
//! random spread edges, then checks invariants that hold no matter how the
//! document prints: reachability is sound and complete, a found result holds
//! exactly one operation, a second drop is a no-op, an unknown name returns the
//! input, and output definitions are a subset of input definitions.

use std::collections::BTreeSet;

use graphql_drop_unused_definitions::{drop_unused_definitions, parse, print};
use proptest::collection::vec;
use proptest::prelude::*;

/// A generated document: the query's direct spreads and each fragment's spreads.
///
/// Fragments are named `F0..Fn`. Edges run only from a lower index to a higher
/// one, so the spread graph is acyclic and every spread names a defined
/// fragment.
#[derive(Debug, Clone)]
struct Doc {
    query_spreads: Vec<usize>,
    fragment_spreads: Vec<Vec<usize>>,
}

impl Doc {
    fn fragment_count(&self) -> usize {
        self.fragment_spreads.len()
    }

    /// Render the document as GraphQL source.
    fn source(&self) -> String {
        let mut out = String::from("query Q {");
        for &target in &self.query_spreads {
            out.push_str(&format!(" ...F{target}"));
        }
        out.push_str(" __typename }\n");
        for (index, spreads) in self.fragment_spreads.iter().enumerate() {
            out.push_str(&format!("fragment F{index} on T {{"));
            for &target in spreads {
                out.push_str(&format!(" ...F{target}"));
            }
            out.push_str(&format!(" field{index} }}\n"));
        }
        out
    }

    /// The fragment indices reachable from the query, computed independently of
    /// the crate under test.
    fn reachable(&self) -> BTreeSet<usize> {
        let mut seen = BTreeSet::new();
        let mut stack: Vec<usize> = self.query_spreads.clone();
        while let Some(name) = stack.pop() {
            if seen.insert(name) {
                stack.extend(self.fragment_spreads[name].iter().copied());
            }
        }
        seen
    }
}

/// The set of `Fi` fragment names present in a printed document.
fn printed_fragment_names(printed: &str) -> BTreeSet<usize> {
    let mut names = BTreeSet::new();
    for line in printed.lines() {
        if let Some(rest) = line.strip_prefix("fragment F") {
            if let Some(end) = rest.find(' ') {
                if let Ok(index) = rest[..end].parse::<usize>() {
                    names.insert(index);
                }
            }
        }
    }
    names
}

/// Count `fragment Fi on` definition headers in a printed document.
fn count_fragment_defs(printed: &str) -> usize {
    printed
        .lines()
        .filter(|line| line.starts_with("fragment F"))
        .count()
}

/// Count operation definition headers in a printed document.
fn count_operations(printed: &str) -> usize {
    printed
        .lines()
        .filter(|line| line.starts_with("query "))
        .count()
}

prop_compose! {
    fn arb_doc()(fragment_count in 0usize..8)
        (query_spreads in vec(0..fragment_count.max(1), 0..6),
         fragment_spreads in (0..fragment_count)
            .map(|i| vec((i + 1)..(fragment_count + 1), 0..4))
            .collect::<Vec<_>>(),
         fragment_count in Just(fragment_count))
        -> Doc
    {
        // Clamp generated targets to the valid fragment range. The query may
        // spread any fragment. Fragment i spreads only fragments after it.
        let query_spreads = query_spreads
            .into_iter()
            .filter(|&t| t < fragment_count)
            .collect();
        let fragment_spreads = fragment_spreads
            .into_iter()
            .map(|spreads| spreads.into_iter().filter(|&t| t < fragment_count).collect())
            .collect();
        Doc { query_spreads, fragment_spreads }
    }
}

proptest! {
    #[test]
    fn reachability_is_sound_and_complete(doc in arb_doc()) {
        let src = doc.source();
        let ast = parse(&src).expect("generated source parses");
        let printed = print(&drop_unused_definitions(&ast, "Q"));

        let expected = doc.reachable();
        let actual = printed_fragment_names(&printed);
        // Sound: nothing extra. Complete: nothing missing.
        prop_assert_eq!(actual, expected);
    }

    #[test]
    fn found_result_holds_exactly_one_operation(doc in arb_doc()) {
        let ast = parse(&doc.source()).expect("generated source parses");
        let printed = print(&drop_unused_definitions(&ast, "Q"));
        prop_assert_eq!(count_operations(&printed), 1);
    }

    #[test]
    fn output_definition_count_matches_reachable_set(doc in arb_doc()) {
        let ast = parse(&doc.source()).expect("generated source parses");
        let printed = print(&drop_unused_definitions(&ast, "Q"));
        prop_assert_eq!(count_fragment_defs(&printed), doc.reachable().len());
    }

    #[test]
    fn second_drop_is_idempotent(doc in arb_doc()) {
        let ast = parse(&doc.source()).expect("generated source parses");
        let once = drop_unused_definitions(&ast, "Q");
        let twice = drop_unused_definitions(&once, "Q");
        prop_assert_eq!(print(&once), print(&twice));
    }

    #[test]
    fn unknown_name_returns_input_unchanged(doc in arb_doc()) {
        let ast = parse(&doc.source()).expect("generated source parses");
        let dropped = drop_unused_definitions(&ast, "NoSuchOperation");
        prop_assert_eq!(print(&dropped), print(&ast));
    }

    #[test]
    fn output_is_a_subset_of_input(doc in arb_doc()) {
        let ast = parse(&doc.source()).expect("generated source parses");
        let dropped = drop_unused_definitions(&ast, "Q");

        let input_fragments: BTreeSet<usize> = (0..doc.fragment_count()).collect();
        let output_fragments = printed_fragment_names(&print(&dropped));
        prop_assert!(output_fragments.is_subset(&input_fragments));
    }
}
