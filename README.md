# tpt-eidos

**Proof-native, dependently-typed systems language for safety-critical code.**

`tpt-eidos` is a compiler that is also a theorem prover: it refuses to emit code
unless the program is a valid proof of its own correctness, then erases all proof
terms for zero-cost extraction to `no_std` Rust.

The current release implements the **Minimal Viable Kernel (MVK)** plus the
**Eraser**: a trusted refinement-type checker and a transparent QF_LRA decision
procedure (Fourier–Motzkin), with proof-term erasure to a computational core and
codegen to a `no_std` Rust crate. A pre-proved flight-control domain library
(`tpt-eidos-flight-math`) is included.

## Install

```sh
cargo install tpt-eidos-cli
```

This installs the `eidos` binary.

## Usage

```sh
# Verify a .eidos source file (refinement subtyping + division safety + termination)
eidos check examples/calibrate_gyro.eidos

# Verify and emit a verified, erased no_std Rust crate (lib.rs + Cargo.toml)
eidos build examples/calibrate_gyro.eidos --out-dir out/
```

## Pipeline

```
source .eidos
  -> tpt-eidos-parser   (lexer + AST)
  -> tpt-eidos-kernel   (refinement-subtyping typecheck + proof obligations)
  -> tpt-eidos-verifier (QF_LRA obligation discharge: unsat / entails / model / counterexample)
  -> accept / reject
eidos build then runs:
  -> tpt-eidos-erasure  (strip refinements/contracts/effects to a computational core)
  -> tpt-eidos-codegen  (emit a no_std Rust crate)
```

## Workspace crates

| Crate | Purpose |
| --- | --- |
| `tpt-eidos-parser` | Lexer, AST, recursive-descent parser |
| `tpt-eidos-kernel` | Trusted refinement-subtyping typechecker (MVK) |
| `tpt-eidos-verifier` | Transparent QF_LRA decision procedure (Fourier–Motzkin) |
| `tpt-eidos-erasure` | Proof-term erasure to a computational-core IR |
| `tpt-eidos-codegen` | Lower the erased core to a `no_std` Rust crate |
| `tpt-eidos-flight-math` | Pre-proved flight-control domain library |
| `tpt-eidos-cli` | The `eidos` command-line tool |

## Trust and scope

The MVK deliberately excludes general dependent pattern matching / inductive
families (v1 scope). All proof obligations are discharged by the in-repo QF_LRA
procedure; a small, reviewable set of non-linear facts is admitted via named
trusted lemmas whose use is recorded in the verification report. The toolchain is
pure `std` with no external crate dependencies, so the trusted computing base is
auditable and CI runs fully offline.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
