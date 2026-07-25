# tpt-eidos-verifier

A transparent QF_LRA decision procedure for [`tpt-eidos`](https://github.com/tpt-solutions/tpt-eidos).

This crate implements Fourier–Motzkin variable elimination over quantifier-free
linear real arithmetic. It is the trusted core that discharges the proof
obligations the kernel derives (division safety, refinement subtyping,
`ensures` clauses). It is pure `std` with no external dependencies, so the
trusted computing base is auditable and CI runs fully offline.

## API

- `unsat(constraints: &[Constraint]) -> bool` — is the system unsatisfiable?
- `entails(premises: &[Constraint], conclusion: &Constraint) -> bool` — does the
  premise set entail the conclusion?
- `find_model(constraints: &[Constraint]) -> Option<BTreeMap<String, f64>>` — a
  satisfying model, if any.
- `counterexample(premises: &[Constraint], conclusion: &Constraint)` — a model
  that satisfies the premises but violates the conclusion (when `entails` is false).
- `Constraint`, `LinExpr`, `Rel` — the constraint representation.

## Example

```rust
use tpt_eidos_verifier::{unsat, Constraint, LinExpr, Rel};

// x > 0 and x < 0 is unsatisfiable.
let cs = vec![
    Constraint::new(LinExpr::var("x"), Rel::Lt, 0.0),
    Constraint::new(LinExpr::var("x"), Rel::Gt, 0.0),
];
assert!(unsat(&cs));
```

## License

Licensed under either of Apache-2.0 or MIT at your option. See the
[workspace README](https://github.com/tpt-solutions/tpt-eidos) for details.
