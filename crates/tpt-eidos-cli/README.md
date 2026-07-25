# tpt-eidos-cli

The `eidos` command-line tool for [`tpt-eidos`](https://github.com/tpt-solutions/tpt-eidos).

This is the user-facing entry point. Install it with `cargo install tpt-eidos-cli`,
which provides the `eidos` binary.

## Commands

```sh
# Verify a .eidos source file (refinement subtyping + division safety + termination).
eidos check examples/calibrate_gyro.eidos

# Verify and emit a verified, erased no_std Rust crate (lib.rs + Cargo.toml).
eidos build examples/calibrate_gyro.eidos --out-dir out/
```

`eidos build` refuses to emit code for any module the kernel has not verified,
and the emitted crate contains no verification machinery — only the erased
computational core.

## Pipeline

```
source .eidos
  -> tpt-eidos-parser      (parse)
  -> tpt-eidos-kernel      (typecheck + proof obligations)
  -> tpt-eidos-verifier    (discharge QF_LRA obligations)
eidos build then runs
  -> tpt-eidos-erasure     (strip proofs)
  -> tpt-eidos-codegen     (emit no_std Rust)
```

## License

Licensed under either of Apache-2.0 or MIT at your option. See the
[workspace README](https://github.com/tpt-solutions/tpt-eidos) for details.
