# AGENTS.md — tpt-eidos

tpt-eidos is a proof-native, dependently-typed systems language for safety-critical
code (flight control, etc.). The compiler is a theorem prover: it refuses to emit
code unless the program is a valid proof of its own correctness, then erases all
proof terms for zero-cost extraction to `no_std` Rust.

This repo implements **Phase 1: the Minimal Viable Kernel (MVK)** — the
trusted refinement-type checker plus a transparent QF_LRA decision procedure —
and **Phase 2: the Eraser** — proof-term erasure to a computational core and
codegen to `no_std` Rust. The remaining phases (flight-math domain library,
AI proof synthesis) are still directional.

## Workspace layout

```
spec.txt                 design doc (the language vision)
TODO.md                  phased roadmap (source of truth for tasks)
crates/
  eidos-verifier/        QF_LRA Fourier-Motzkin decision procedure
                         (unsat / entails / model / counterexample)
  eidos-parser/          lexer + recursive-descent parser + AST
                         (crates/eidos-parser/src/grammar.ebnf)
  eidos-kernel/          trusted refinement-subtyping typechecker,
                         division-by-zero safety, termination check
  eidos-cli/             `eidos check <file>` / `eidos build <file>`
  eidos-erasure/        proof-term erasure to a computational-core IR
  eidos-codegen/        lower erased IR to a `no_std` Rust crate
  eidos-tests/           integration tests over examples/
examples/
  calibrate_gyro.eidos          spec §4 worked example (must verify)
  calibrate_gyro_broken.eidos   same, missing the `mag > 0.0` guard (must reject)
```

## Pipeline

`source .eidos` → `eidos-parser` (AST) → `eidos-kernel` (typecheck + collect proof
obligations) → `eidos-verifier` (discharge QF_LRA obligations) → accept/reject.
`eidos build` then runs `eidos-erasure` (strip refinements/contracts/effects to a
computational core) → `eidos-codegen` (emit `no_std` Rust: `lib.rs` + `Cargo.toml`).

## Invariants the kernel enforces (MVK scope)

- **Division safety**: every `a / b` must be provably non-zero, i.e.
  `unsat(context ∧ b == 0)` via the QF_LRA verifier. This is what separates
  `calibrate_gyro` (passes: guard proves `mag > 0`) from
  `calibrate_gyro_broken` (fails: no such proof).
- **Refinement subtyping**: casts/`ensures` obligations that are linear are
  discharged by the verifier; non-linear ones (e.g. `v.magnitude() <= 1.0`)
  are discharged via the trusted-lemma table in the kernel (the Phase-3
  domain-library boundary — see `TrustedLemmas`).
- **Termination**: non-recursive functions pass; recursive functions must call
  on a structurally decreasing argument.

## Conventions

- Pure `std` only; no external crates (keeps the TCB auditable and CI offline).
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace` must be clean.
- Do not add comments to code unless asked.
- New crates: add to `Cargo.toml` `[workspace.members]`.
