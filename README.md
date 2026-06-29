# graphql-drop-unused-definitions

Prune a GraphQL document down to one operation and the fragments it needs.

A request document can carry several named operations plus fragment
definitions, with the client choosing one operation by name. The rest is dead
weight on the wire and in the parser. This crate trims a document to the chosen
operation and the fragments it reaches through `...Spread` references, at any
depth. Drop unused definitions on the client before sending to save bandwidth
and parse time.

## Installation

```toml
[dependencies]
graphql-drop-unused-definitions = "0.1"
```

## Usage

```rust
use graphql_drop_unused_definitions::{drop_unused_definitions, parse, print};

let doc = parse(
    "query Drop { ...DroppedFragment }\n\
     fragment DroppedFragment on Query { abc }\n\
     query Keep { ...KeptFragment }\n\
     fragment KeptFragment on Query { def }",
)
.unwrap();

let kept = drop_unused_definitions(&doc, "Keep");
assert_eq!(
    print(&kept),
    "query Keep {\n  ...KeptFragment\n}\n\nfragment KeptFragment on Query {\n  def\n}",
);
```

The result holds `query Keep` and `fragment KeptFragment`. The `Drop` operation
and `DroppedFragment` are gone.

## Behavior

- The chosen operation plus every fragment it reaches transitively is kept.
  Other operations and unreachable fragments are dropped.
- Definitions keep their original document order.
- An anonymous operation, written as `{ ... }` or a bare `query { ... }`, is
  keyed by the empty string. Pass `""` to select it.
- If no operation matches the name, the whole document is returned unchanged.
- No schema, no validation. A spread of an undefined fragment is ignored rather
  than an error. Cyclic fragments terminate.
- When two operations share a name, the later one in document order wins.

## License

Licensed under the [MIT license](LICENSE).
