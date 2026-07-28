# tpt-eidos-controls-math

Pre-proved generic control-systems domain library for [`tpt-eidos`](https://github.com/tpt-solutions/tpt-eidos).

This crate provides verified control-systems primitives — output clamping, bounded
increments, and rate limiting — using the same `Lemma`/`TrustedLemmas` pattern as
`tpt-eidos-flight-math`. All obligations are discharged by the kernel and the
QF_LRA verifier; no proof steps are trusted on faith.

## API

- `check_module(module: &Module) -> Report` — verify with the controls-math lemmas.
- `check_module_with(module: &Module, extra: &[Lemma]) -> Report` — add more lemmas.
- `check_source(src: &str) -> Result<Report, ParseError>` — parse + verify.
- `PRIMITIVES_EIDOS` — verified primitive sources (`clamp`, `bounded_add`, `rate_limit`).
- `CONTROLS_LEMMAS` — the domain lemma table (extends `DEFAULT_LEMMAS`).

## Example

```rust
use tpt_eidos_controls_math::check_module;
use tpt_eidos_parser::parse;

let src = tpt_eidos_controls_math::PRIMITIVES_EIDOS;
let module = parse(src).unwrap();
let report = check_module(&module);
assert!(report.ok());
```

## License

Licensed under either of Apache-2.0 or MIT at your option. See the
[workspace README](https://github.com/tpt-solutions/tpt-eidos) for details.
