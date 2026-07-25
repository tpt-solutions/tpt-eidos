# Getting Started with tpt-eidos

This guide walks you through verifying and building a tpt-eidos program from
scratch. It uses the `calibrate_gyro` example to demonstrate the full pipeline.

## Prerequisites

- Rust 1.74+ (with `cargo`)
- The `eidos` CLI: `cargo install tpt-eidos-cli`

## 1. Verify a source file

The `eidos check` command parses a `.eidos` file, type-checks it against the
refinement subtyping rules, and discharges every proof obligation via the
QF_LRA decision procedure. It reports `path:line:col` diagnostics on failure.

```sh
eidos check examples/calibrate_gyro.eidos
```

Expected output:

```
eidos: examples/calibrate_gyro.eidos: verified (5 verified, 1 trusted-lemma)
```

The "trusted-lemma" count reflects non-linear obligations (like
`v.magnitude() <= 1.0`) that the kernel admits via named, reviewable axioms
rather than the linear prover. Every such step is traceable in the report.

## 2. Try a broken file

The file `examples/calibrate_gyro_broken.eidos` omits the `mag > 0.0` guard,
so the kernel cannot prove that division by zero is impossible:

```sh
eidos check examples/calibrate_gyro_broken.eidos
```

Expected output:

```
eidos: examples/calibrate_gyro_broken.eidos: REJECTED
  examples/calibrate_gyro_broken.eidos:20:12: error: possible division by zero: denominator could be zero. counterexample: ...
```

This is the core safety guarantee: `eidos check` rejects programs where a
division-by-zero path exists, even if it would not happen at runtime with
specific inputs.

## 3. Build a verified crate

The `eidos build` command runs `check` and, on success, erases all proof
terms and codegen to a `no_std` Rust crate:

```sh
eidos build examples/calibrate_gyro.eidos --out-dir out/calibrate_gyro
```

This writes `out/calibrate_gyro/lib.rs` and `out/calibrate_gyro/Cargo.toml`.
The generated crate:

- Is `#![no_std]` (zero allocations, suitable for bare-metal targets)
- Contains no kernel, verifier, or parser types
- Has identical runtime behavior to the original eidos source
- Can be built with `cargo build --release` in the output directory

```sh
cd out/calibrate_gyro && cargo build --release
```

## 4. Scaffold a new project

```sh
eidos new my_project
```

This creates `my_project/my_project.eidos` with a starter example that
demonstrates refinement types, division safety, and postconditions.

## 5. What just happened?

The pipeline:

```
source.eidos
  -> tpt-eidos-parser    (lex + parse to AST)
  -> tpt-eidos-kernel    (refinement-subtyping + proof obligations)
  -> tpt-eidos-verifier  (QF_LRA: Fourier-Motzkin decision procedure)
  -> accept / reject

eidos build (on success):
  -> tpt-eidos-erasure   (strip refinements/contracts/effects)
  -> tpt-eidos-codegen   (emit no_std Rust)
```

The kernel is the trusted core: it enforces that every division is safe, every
cast satisfies its refinement predicate, and every recursive function terminates.
The verifier is a transparent, in-repo QF_LRA solver with no external SMT
dependency, so the trusted computing base is auditable and CI runs offline.

## Next steps

- Read `spec.txt` for the full language design
- Explore `crates/tpt-eidos-flight-math/` for a real-world domain library
- See `TODO.md` for the roadmap and upcoming features
