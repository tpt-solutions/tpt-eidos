# Changelog

All notable changes to this project will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-07-16

### Added

- **eidos-parser**: Lexer, recursive-descent parser, and typed AST for the
  tpt-eidos MVK surface language. Supports refinement types `{ x: T | p }`,
  `Array<T, N>`, `requires`/`ensures` contracts, and `effects [...]` labels.
  Pure `std`; no external crates.

- **eidos-kernel**: Trusted refinement-subtyping typechecker (Minimal Viable
  Kernel). Verifies three properties: division safety (every `a / b` requires
  a provable `b ≠ 0` guard), refinement subtyping (`as T` casts and `ensures`
  obligations discharged by the QF_LRA prover or named trusted lemmas), and
  termination (recursive calls must reduce a decreasing metric). Returns a
  `Report` with per-obligation provenance.

- **eidos-verifier**: Self-contained QF_LRA decision procedure via
  Fourier-Motzkin elimination. No external SMT dependency; the trusted
  computing base stays auditable and CI runs fully offline. Exposes `unsat`,
  `entails`, `find_model`, and `counterexample`.

- **eidos-erasure**: Proof-term erasure. Strips refinements, contracts,
  and effects labels from a kernel-checked `Module`, producing a
  `CoreModule` annotated with erased `CoreType`s for the code generator.

- **eidos-codegen**: Lowers the erased computational core to a complete,
  self-contained `#![no_std]` Rust crate. All proof machinery is removed;
  zero runtime cost from verification. Array helpers (`eidos_map`,
  `eidos_zip`, `eidos_magnitude`, `eidos_len`) are emitted as
  `const`-generic, stack-only functions.

- **eidos-flight-math**: Pre-proved flight-control domain library (Phase 3).
  Ships `PRIMITIVES_EIDOS` (reusable vector/quaternion/PID primitives),
  domain-specific trusted lemmas, and a kernel-gated proof-step suggester
  (`suggest_and_verify`) for the Phase-4 AI-assist workflow.

- **eidos-cli**: The `eidos` binary. `eidos check <file>` verifies a source
  file; `eidos build <file> --out-dir D` additionally erases and emits a
  `no_std` Rust crate.

[0.1.0]: https://github.com/tpt-solutions/tpt-eidos/releases/tag/v0.1.0
