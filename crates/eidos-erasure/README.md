# tpt-eidos-erasure

Proof-term erasure for [`tpt-eidos`](https://github.com/tpt-solutions/tpt-eidos).

Once the kernel has verified a module, the refinement types, contracts, and
effect labels carry no computational content — they exist only to be believed.
This crate strips them, producing a small, pure computational-core IR
(`CoreModule`) that the code generator lowers to `no_std` Rust. No kernel or
verifier types survive into the erased core.

## API

- `erase(module: &Module) -> CoreModule` — produce the computational core.
- `CoreModule`, `CoreFun`, `CExpr`, `CExprKind`, `CoreType`, `StructDef` — the
  erased IR.

## Example

```rust
use tpt_eidos_erasure::erase;
use tpt_eidos_parser::parse;

let module = parse("fn id(x: f64) -> f64 { x }").unwrap();
let core = erase(&module);
assert_eq!(core.funs.len(), 1);
```

## License

Licensed under either of Apache-2.0 or MIT at your option. See the
[workspace README](https://github.com/tpt-solutions/tpt-eidos) for details.
