# tpt-eidos-parser

Lexer, AST, and recursive-descent parser for the [`tpt-eidos`](https://github.com/tpt-solutions/tpt-eidos) language.

This crate turns `.eidos` source text into the typed AST that the rest of the
toolchain consumes. It knows nothing about verification — it only parses the
grammar (including refinement types `{ x: T | p }`, `Array<T, N>`,
`requires`/`ensures`, and `effects [...]`).

## API

- `parse(source: &str) -> Result<Module, ParseError>` — parse a whole module.
- `parse_expr(source: &str) -> Result<Expr, ParseError>` — parse a single expression.
- `Module`, `Expr`, `Fun`, `Item`, `Pattern`, `Type`, `BinOp`, `UnOp` — the AST.
- `ParseError` — parse failure (kind only; no source position yet).

The grammar is documented in [`src/grammar.ebnf`](src/grammar.ebnf).

## Example

```rust
use tpt_eidos_parser::{parse, ParseError};

let src = "fn id(x: f64) -> f64 { x }";
let module = parse(src).expect("parse");
assert_eq!(module.items.len(), 1);
# let _ = ParseError::Unnamed; // keep the import used
```

## License

Licensed under either of Apache-2.0 or MIT at your option. See the
[workspace README](https://github.com/tpt-solutions/tpt-eidos) for details.
