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
    Definition, Document as RawDocument, OperationDefinition, Selection, SelectionSet, Value,
};

pub use graphql_parser::query::ParseError;

/// A parsed GraphQL document.
///
/// This is an owned tree, so it carries no borrows from the source text and can
/// be returned and stored freely.
///
/// The alias resolves to `graphql_parser::query::Document<'static, String>`.
/// Inspecting or building a [`Document`] beyond passing it back into this crate
/// needs `graphql-parser` as a direct dependency, pinned to the version this
/// crate uses. [`parse`] and [`print`] cover the round trip without that.
pub type Document = RawDocument<'static, String>;

/// Parse GraphQL source into a [`Document`].
///
/// `parse` accepts executable documents only: operations and fragment
/// definitions. Type-system definitions such as `type`, `schema`, and
/// `directive` are out of scope and make the source invalid here.
///
/// # Errors
///
/// Returns a [`ParseError`] when the source is not a valid GraphQL executable
/// document.
///
/// ```
/// use graphql_drop_unused_definitions::parse;
///
/// assert!(parse("{ field }").is_ok());
/// assert!(parse("query {").is_err());
/// ```
pub fn parse(source: &str) -> Result<Document, ParseError> {
    graphql_parser::parse_query::<String>(source).map(RawDocument::into_static)
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
#[must_use]
pub fn print(document: &Document) -> String {
    let mut definitions: Vec<_> = document
        .definitions
        .iter()
        .cloned()
        .map(shorthand_if_bare_query)
        .collect();

    // graphql-parser renders a string value that holds a newline as a block
    // string, and its control-character escapes use decimal digits. Both
    // diverge from canonical GraphQL printing. Swap each string value for a
    // unique ASCII placeholder, then put the canonical rendering back after the
    // structural print runs.
    let mut strings = StringTable::default();
    for definition in &mut definitions {
        rewrite_strings_in_definition(definition, &mut strings);
    }

    let normalized = Document { definitions };
    let rendered = format!("{normalized}").trim_end_matches('\n').to_string();

    // Put the canonical renderings back in one forward pass. Placeholders print
    // once and in interning order, so finding each in turn and never rescanning
    // what was already written leaves a value whose own text equals a
    // placeholder untouched.
    let mut out = String::with_capacity(rendered.len());
    let mut rest = rendered.as_str();
    for (placeholder, original) in strings.entries() {
        let needle = format!("\"{placeholder}\"");
        let pos = rest.find(&needle).expect("each placeholder prints once");
        out.push_str(&rest[..pos]);
        out.push_str(&canonical_string(original));
        rest = &rest[pos + needle.len()..];
    }
    out.push_str(rest);
    out
}

/// Placeholders standing in for string values during the structural print.
///
/// Each string value gets a fresh ASCII key with no characters that need
/// escaping, so graphql-parser prints it verbatim inside quotes. After printing,
/// each `"key"` is swapped for the canonical rendering of the value it stands
/// for.
#[derive(Default)]
struct StringTable {
    values: Vec<(String, String)>,
}

impl StringTable {
    /// Reserve a placeholder for `value` and return it.
    fn intern(&mut self, value: String) -> String {
        let key = format!("__GQL_STRING_PLACEHOLDER_{}__", self.values.len());
        self.values.push((key.clone(), value));
        key
    }

    /// The placeholder/value pairs in interning order.
    ///
    /// Placeholders print in this order, so a single forward pass consumes each
    /// one exactly once.
    fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.values
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
    }
}

/// Render a string value as a canonical single-line GraphQL string.
///
/// This matches graphql-js: wrap in double quotes, escape `"` and `\`, use the
/// short forms for backspace, tab, newline, form feed, and carriage return, and
/// use `\uXXXX` with uppercase hex for the other control characters. Every other
/// character, including non-ASCII, is emitted as is.
fn canonical_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{000C}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 || (0x7F..=0x9F).contains(&(c as u32)) => {
                use std::fmt::Write;
                write!(out, "\\u{:04X}", c as u32).expect("write to String is infallible");
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Replace every string value in a definition with an interned placeholder.
fn rewrite_strings_in_definition(
    definition: &mut Definition<'static, String>,
    table: &mut StringTable,
) {
    match definition {
        Definition::Operation(operation) => rewrite_strings_in_operation(operation, table),
        Definition::Fragment(fragment) => {
            for directive in &mut fragment.directives {
                rewrite_strings_in_arguments(&mut directive.arguments, table);
            }
            rewrite_strings_in_selection_set(&mut fragment.selection_set, table);
        }
    }
}

fn rewrite_strings_in_operation(
    operation: &mut OperationDefinition<'static, String>,
    table: &mut StringTable,
) {
    let (variable_definitions, directives, selection_set) = match operation {
        OperationDefinition::SelectionSet(set) => {
            rewrite_strings_in_selection_set(set, table);
            return;
        }
        OperationDefinition::Query(query) => (
            &mut query.variable_definitions,
            &mut query.directives,
            &mut query.selection_set,
        ),
        OperationDefinition::Mutation(mutation) => (
            &mut mutation.variable_definitions,
            &mut mutation.directives,
            &mut mutation.selection_set,
        ),
        OperationDefinition::Subscription(subscription) => (
            &mut subscription.variable_definitions,
            &mut subscription.directives,
            &mut subscription.selection_set,
        ),
    };
    for variable in variable_definitions {
        if let Some(default) = &mut variable.default_value {
            rewrite_strings_in_value(default, table);
        }
    }
    for directive in directives {
        rewrite_strings_in_arguments(&mut directive.arguments, table);
    }
    rewrite_strings_in_selection_set(selection_set, table);
}

fn rewrite_strings_in_selection_set(
    selection_set: &mut SelectionSet<'static, String>,
    table: &mut StringTable,
) {
    for selection in &mut selection_set.items {
        match selection {
            Selection::Field(field) => {
                rewrite_strings_in_arguments(&mut field.arguments, table);
                for directive in &mut field.directives {
                    rewrite_strings_in_arguments(&mut directive.arguments, table);
                }
                rewrite_strings_in_selection_set(&mut field.selection_set, table);
            }
            Selection::InlineFragment(inline) => {
                for directive in &mut inline.directives {
                    rewrite_strings_in_arguments(&mut directive.arguments, table);
                }
                rewrite_strings_in_selection_set(&mut inline.selection_set, table);
            }
            Selection::FragmentSpread(spread) => {
                for directive in &mut spread.directives {
                    rewrite_strings_in_arguments(&mut directive.arguments, table);
                }
            }
        }
    }
}

fn rewrite_strings_in_arguments(
    arguments: &mut [(String, Value<'static, String>)],
    table: &mut StringTable,
) {
    for (_, value) in arguments {
        rewrite_strings_in_value(value, table);
    }
}

fn rewrite_strings_in_value(value: &mut Value<'static, String>, table: &mut StringTable) {
    match value {
        Value::String(text) => {
            let placeholder = table.intern(std::mem::take(text));
            *text = placeholder;
        }
        Value::List(items) => {
            for item in items {
                rewrite_strings_in_value(item, table);
            }
        }
        Value::Object(fields) => {
            for field in fields.values_mut() {
                rewrite_strings_in_value(field, table);
            }
        }
        _ => {}
    }
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
/// unchanged. When two operations share a name, the later one in document order
/// wins.
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
#[must_use]
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
    // the earlier choice, matching last writer wins. Keep the node alongside its
    // index so the filter can match by position and no second lookup is needed.
    let mut chosen: Option<(usize, &OperationDefinition<'static, String>)> = None;
    for (index, definition) in ast.definitions.iter().enumerate() {
        if let Definition::Operation(operation) = definition {
            if name_of(operation) == operation_name {
                chosen = Some((index, operation));
            }
        }
    }

    let (operation_index, operation) = chosen?;

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
/// The walk uses an explicit stack, so a long fragment chain cannot overflow the
/// call stack. The visited guard makes it terminate on cyclic fragment
/// references. A name with no entry in `dep_graph`, such as a spread of an
/// undefined fragment, is still added but contributes no children.
fn collect_transitive<'a>(
    collected: &mut HashSet<&'a str>,
    dep_graph: &HashMap<&'a str, Vec<&'a str>>,
    from: &'a str,
) {
    let mut stack = vec![from];
    while let Some(name) = stack.pop() {
        if !collected.insert(name) {
            continue;
        }
        if let Some(deps) = dep_graph.get(name) {
            stack.extend(deps.iter().copied());
        }
    }
}
