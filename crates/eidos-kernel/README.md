# tpt-eidos-kernel

The trusted refinement-subtyping typechecker for [`tpt-eidos`](https://github.com/tpt-solutions/tpt-eidos).

The kernel is the heart of the MVK: it typechecks a parsed module, enforces
division-by-zero safety, checks structural-termination of recursive functions,
and collects the proof obligations that the QF_LRA verifier must discharge. It
is deliberately small and auditable (no general dependent pattern matching in
v1), and every trusted step is recorded in the returned `Report`.

## API

- `check(module: &Module) -> Report` — check with the default lemma set.
- `check_with(module: &Module, lemmas: &[Lemma]) -> Report` — check with extra
  trusted lemmas (e.g. a domain library's facts).
- `Report` — per-obligigation verdicts (`Obligation`, `ObligationStatus`) plus
  any `CheckError`s. `Report::ok()` is true only when every obligation is
  `Verified` or `Trusted`.
- `Lemma`, `DEFAULT_LEMMAS`, `lemma_normalized_vector` — the trusted-lemma
  boundary for non-linear facts the QF_LRA prover cannot handle.

## Example

```rust
use tpt_eidos_kernel::check;

let module = tpt_eidos_parser::parse("fn id(x: f64) -> f64 { x }").unwrap();
let report = check(&module);
assert!(report.ok());
```

## License

Licensed under either of Apache-2.0 or MIT at your option. See the
[workspace README](https://github.com/tpt-solutions/tpt-eidos) for details.
