# tpt-eidos-codegen

Lowers the erased [`tpt-eidos`](https://github.com/tpt-solutions/tpt-eidos) computational core to a `no_std` Rust crate.

Given a `CoreModule` from `tpt-eidos-erasure`, this crate emits a self-contained
`lib.rs` (plus, in the CLI, a `Cargo.toml`) containing only the computational
content — no verification machinery leaks through. The emitted crate is
`#![no_std]` and allocation-free by construction.

## API

- `codegen(module: &CoreModule) -> Result<String, String>` — emit Rust source.
- `eidos_len`, `eidos_sqrt`, `eidos_magnitude`, `eidos_map`, `eidos_zip` — the
  small runtime helper prelude that generated crates use for array/vector ops.

## Example

```rust
use tpt_eidos_codegen::codegen;
use tpt_eidos_erasure::erase;
use tpt_eidos_parser::parse;

let module = parse("fn id(x: f64) -> f64 { x }").unwrap();
let rust = codegen(&erase(&module)).unwrap();
assert!(rust.contains("#![no_std]"));
```

## License

Licensed under either of Apache-2.0 or MIT at your option. See the
[workspace README](https://github.com/tpt-solutions/tpt-eidos) for details.
