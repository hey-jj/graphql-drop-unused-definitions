//! Prune a GraphQL document down to one operation and the fragments it needs.
//!
//! A request document can hold several named operations plus fragment
//! definitions. The client picks one operation by name and the rest are dead
//! weight on the wire and in the parser. [`drop_unused_definitions`] takes a
//! parsed document and an operation name and returns a new document with only
//! that operation and the fragments it reaches through `...Spread` references,
//! at any depth.
//!
//! If the name matches no operation, the function returns the input document
//! unchanged. It does no validation and never errors.
//!
//! ```
//! use graphql_drop_unused_definitions::{drop_unused_definitions, parse, print};
//!
//! let doc = parse(
//!     "query Drop { ...DroppedFragment }\n\
//!      fragment DroppedFragment on Query { abc }\n\
//!      query Keep { ...KeptFragment }\n\
//!      fragment KeptFragment on Query { def }",
//! )
//! .unwrap();
//!
//! let kept = drop_unused_definitions(&doc, "Keep");
//! assert_eq!(
//!     print(&kept),
//!     "query Keep {\n  ...KeptFragment\n}\n\nfragment KeptFragment on Query {\n  def\n}"
//! );
//! ```
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::{HashMap, HashSet};

use graphql_parser::query::{
    Definition, Document as RawDocument, OperationDefinition, ParseError, Selection, SelectionSet,
};

/// A parsed GraphQL document.
///
/// This is an owned tree, so it carries no borrows from the source text and can
/// be returned and stored freely.
pub type Document = RawDocument<'static, String>;

/// Parse GraphQL source into a [`Document`].
///
/// Returns a [`ParseError`] if the source is not a valid executable document.
///
/// ```
/// use graphql_drop_unused_definitions::parse;
///
/// assert!(parse("{ field }").is_ok());
/// assert!(parse("query {").is_err());
/// ```
pub fn parse(source: &str) -> Result<Document, ParseError> {
    graphql_parser::parse_query::<String>(source).map(|doc| doc.into_static())
}

/// Render a [`Document`] back to GraphQL source.
///
/// Output uses two-space indentation, one selection per line, and a single
/// blank line between top level definitions. There is no trailing newline. A
/// `query` with no name, no variables, and no directives prints in shorthand
/// form as just its selection set.
///
/// ```
/// use graphql_drop_unused_definitions::{parse, print};
///
/// let doc = parse("{abc}").unwrap();
/// assert_eq!(print(&doc), "{\n  abc\n}");
///
/// // A bare `query` with nothing but a selection set prints in shorthand.
/// let doc = parse("query { abc }").unwrap();
/// assert_eq!(print(&doc), "{\n  abc\n}");
/// ```
pub fn print(document: &Document) -> String {
    let normalized = Document {
        definitions: document
            .definitions
            .iter()
            .cloned()
            .map(shorthand_if_bare_query)
            .collect(),
    };
    format!("{normalized}").trim_end_matches('\n').to_string()
}

/// Collapse a `query` with no name, variables, or directives into shorthand.
///
/// GraphQL source allows `{ ... }` as a synonym for an unadorned anonymous
/// query, and that is the canonical printed form. Other operations pass through
/// untouched.
fn shorthand_if_bare_query(definition: Definition<'static, String>) -> Definition<'static, String> {
    match definition {
        Definition::Operation(OperationDefinition::Query(query))
            if query.name.is_none()
                && query.variable_definitions.is_empty()
                && query.directives.is_empty() =>
        {
            Definition::Operation(OperationDefinition::SelectionSet(query.selection_set))
        }
        other => other,
    }
}

/// Return a new document holding only `operation_name` and the fragments it
/// reaches.
///
/// The result keeps the operation whose name matches `operation_name` plus
/// every fragment reachable from it through fragment spreads, transitively and
/// at any depth. Other operations and unreachable fragments are dropped.
/// Definitions stay in their original order.
///
/// An anonymous operation (shorthand `{ ... }` or a bare `query { ... }`) is
/// keyed by the empty string, so pass `""` to select it.
///
/// If no operation matches `operation_name`, the input document is returned
/// unchanged, including any non executable definitions it holds. When two
/// operations share a name, the later one in document order wins.
///
/// ```
/// use graphql_drop_unused_definitions::{drop_unused_definitions, parse, print};
///
/// let doc = parse("query Keep { abc }\nquery Drop { def }").unwrap();
/// assert_eq!(print(&drop_unused_definitions(&doc, "Keep")), "query Keep {\n  abc\n}");
///
/// // Unknown name returns the whole document.
/// assert_eq!(print(&drop_unused_definitions(&doc, "Nope")), print(&doc));
/// ```
pub fn drop_unused_definitions(ast: &Document, operation_name: &str) -> Document {
    match separate(ast, operation_name) {
        Some(doc) => doc,
        None => ast.clone(),
    }
}

/// Build the pruned document for one operation name, or `None` if absent.
///
/// This mirrors indexing the per operation map that a full separation would
/// produce, but it only computes the entry for `operation_name`. Last writer
/// wins on duplicate names: scanning operations in order and overwriting the
/// chosen node reproduces that.
fn separate(ast: &Document, operation_name: &str) -> Option<Document> {
    // Fragment name to the spread names it contains, at any depth.
    let mut dep_graph: HashMap<&str, Vec<&str>> = HashMap::new();
    for definition in &ast.definitions {
        if let Definition::Fragment(fragment) = definition {
            dep_graph.insert(
                fragment.name.as_str(),
                collect_spread_names(&fragment.selection_set),
            );
        }
    }

    // Walk operations in order. A later operation with the same name overwrites
    // the earlier choice, matching last writer wins.
    let mut chosen: Option<usize> = None;
    for (index, definition) in ast.definitions.iter().enumerate() {
        if let Definition::Operation(operation) = definition {
            if name_of(operation) == operation_name {
                chosen = Some(index);
            }
        }
    }

    let operation_index = chosen?;
    let operation = match &ast.definitions[operation_index] {
        Definition::Operation(operation) => operation,
        Definition::Fragment(_) => unreachable!("index points at an operation"),
    };

    // Collect the transitive fragment closure reachable from this operation.
    let mut dependencies: HashSet<&str> = HashSet::new();
    for spread in collect_spread_names(selection_set_of(operation)) {
        collect_transitive(&mut dependencies, &dep_graph, spread);
    }

    // Keep the chosen operation node and every fragment in the closure, in the
    // original document order.
    let definitions = ast
        .definitions
        .iter()
        .enumerate()
        .filter(|(index, definition)| {
            *index == operation_index
                || matches!(
                    definition,
                    Definition::Fragment(fragment) if dependencies.contains(fragment.name.as_str())
                )
        })
        .map(|(_, definition)| definition.clone())
        .collect();

    Some(Document { definitions })
}

/// The name key for an operation. Anonymous operations key to the empty string.
fn name_of<'a>(operation: &'a OperationDefinition<'static, String>) -> &'a str {
    match operation {
        OperationDefinition::SelectionSet(_) => "",
        OperationDefinition::Query(query) => query.name.as_deref().unwrap_or(""),
        OperationDefinition::Mutation(mutation) => mutation.name.as_deref().unwrap_or(""),
        OperationDefinition::Subscription(subscription) => {
            subscription.name.as_deref().unwrap_or("")
        }
    }
}

/// The top level selection set of an operation.
fn selection_set_of<'a>(
    operation: &'a OperationDefinition<'static, String>,
) -> &'a SelectionSet<'static, String> {
    match operation {
        OperationDefinition::SelectionSet(set) => set,
        OperationDefinition::Query(query) => &query.selection_set,
        OperationDefinition::Mutation(mutation) => &mutation.selection_set,
        OperationDefinition::Subscription(subscription) => &subscription.selection_set,
    }
}

/// Collect the names of every fragment spread in a selection set subtree.
///
/// The walk is depth first and visits fields, inline fragments, and their
/// nested selection sets. Names appear in encounter order with no
/// deduplication. The set guard in [`collect_transitive`] handles repeats and
/// cycles.
fn collect_spread_names<'a>(selection_set: &'a SelectionSet<'static, String>) -> Vec<&'a str> {
    let mut names = Vec::new();
    gather(selection_set, &mut names);
    names
}

fn gather<'a>(selection_set: &'a SelectionSet<'static, String>, names: &mut Vec<&'a str>) {
    for selection in &selection_set.items {
        match selection {
            Selection::FragmentSpread(spread) => names.push(spread.fragment_name.as_str()),
            Selection::Field(field) => gather(&field.selection_set, names),
            Selection::InlineFragment(inline) => gather(&inline.selection_set, names),
        }
    }
}

/// Add `from` and everything it reaches to `collected`.
///
/// The visited guard makes this terminate on cyclic fragment references. A name
/// with no entry in `dep_graph`, such as a spread of an undefined fragment, is
/// still added but contributes no children.
fn collect_transitive<'a>(
    collected: &mut HashSet<&'a str>,
    dep_graph: &HashMap<&'a str, Vec<&'a str>>,
    from: &'a str,
) {
    if !collected.insert(from) {
        return;
    }
    if let Some(deps) = dep_graph.get(from) {
        for &dep in deps {
            collect_transitive(collected, dep_graph, dep);
        }
    }
}
