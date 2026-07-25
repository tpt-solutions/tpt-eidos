# tpt-eidos-flight-math

Pre-proved flight-control domain library for [`tpt-eidos`](https://github.com/tpt-solutions/tpt-eidos).

This crate combines the kernel's defaults with a set of trusted flight-control
facts (DCM↔quaternion normalization, PID bounds, and other common
control-law primitives) so that real attitude/control laws verify end-to-end.
It also ships the Phase-4 proof-suggestion loop, where an external agent may
propose a proof step that is always re-checked by the kernel before being
trusted.

## API

- `check_module(module: &Module) -> Report` — verify with the flight-math lemmas.
- `check_module_with(module: &Module, extra: &[Lemma]) -> Report` — add more lemmas.
- `check_source(src: &str) -> Result<Report, ParseError>` — parse + verify.
- `PRIMITIVES_EIDOS` — verified primitive sources (`safe_direction`,
  `quat_normalize`, `pid_linear`).
- `FLIGHT_LEMMAS`, `AGENT_LEMMAS` — the trusted lemma tables.
- `suggest_and_verify` / `ProofStep` / `SuggestOutcome` — the agent proof loop.

## Example

```rust
use tpt_eidos_flight_math::check_module;
use tpt_eidos_parser::parse;

let src = tpt_eidos_flight_math::PRIMITIVES_EIDOS;
let module = parse(src).unwrap();
let report = check_module(&module);
assert!(report.ok());
```

## License

Licensed under either of Apache-2.0 or MIT at your option. See the
[workspace README](https://github.com/tpt-solutions/tpt-eidos) for details.
