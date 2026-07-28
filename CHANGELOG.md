# Changelog

All notable user-visible changes to `tpt-eidos` are recorded here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added
- **Doc comments** (`///`): doc-comment lines in `.eidos` source are parsed and
  threaded through erasure into the generated `lib.rs`, so emitted functions
  carry their source documentation.
- **`eidos fmt`** subcommand: normalises whitespace (trailing spaces, trailing
  newlines) in a `.eidos` file after verifying it parses.  `--check` mode exits
  nonzero if the file needs changes, suitable for CI.
- **`eidos new` workspace scaffold**: in addition to the starter `.eidos` file,
  `eidos new <name>` now writes a `Cargo.toml` workspace wrapper and pre-creates
  the `verified/<name>/` output directory, so `eidos build` and `cargo build` work
  immediately without extra setup.
- **`eidos build --run`**: after emitting the verified crate, automatically
  invokes `cargo build --manifest-path <out-dir>/Cargo.toml`.
- **GitHub issue templates**: `bug_report` and `domain_lemma` templates in
  `.github/ISSUE_TEMPLATE/`.

## [0.2.0] — Phase 6 + 7 hardening

### Added
- **Source spans** (`path:line:col`): `ParseError`, `CheckError`, and
  `Obligation` now carry `Span` (byte-offset range).  `eidos check` and
  `eidos build` render `path:line:col: message` diagnostics.
- **`eidos new <name>`**: scaffolds a starter `.eidos` file.
- **`eidos test <dir>`**: batch-verifies all `.eidos` files in a directory;
  supports `--verbose` and `--json`.
- **`--verbose` / `--explain`**: `eidos check` and `eidos build` print the
  `Verified`/`Trusted`/`Unverified` status of every individual obligation.
- **`--json`**: `eidos check`, `eidos build`, and `eidos test` emit structured
  JSON output for machine/editor consumption.
- **`--emit ast|core`**: `eidos check` can dump the parsed AST or the erased
  computational core for debugging.
- **`--help` / `--version`**: standard flags handled without falling into the
  unknown-subcommand error path.
- **Exact-rational Fourier-Motzkin** (`tpt-eidos-verifier`): a private `Rat`
  (`i128` numerator/denominator) replaces floating-point arithmetic in the
  decision procedure; `EPS` eliminated from the trusted path.
- **`NormRel` type** in `tpt-eidos-verifier`: `Le`/`Lt`-only invariant enforced
  by the type system instead of `unreachable!()`.
- **SoS certificate checker** (`tpt-eidos-verifier`): `SosCertificate` and
  `check_sos_certificate` for exact polynomial non-linear certificate checking.
- **`ProposeNonlinearCertificate` proof step** (`tpt-eidos-flight-math`): the
  agent loop can propose SoS certificates; retired the unsound `triangle_for_add`
  lemma.
- **`tpt-eidos-controls-math`**: second domain library (output clamping, rate
  limiting) reusing the `Lemma`/`TrustedLemmas` pattern.
- **`MAX_CONSTRAINTS` guard** fixed: the DoS guard in Fourier-Motzkin now bails
  *before* the `uppers.len() * lowers.len()` constraint construction.
- **Parser depth limit** (`MAX_PARSE_DEPTH`): prevents stack-overflow DoS from
  deeply nested expressions.
- **`%` division-safety checking**: `BinOp::Rem` now goes through
  `check_division` (was silently exempt).
- **Stronger termination checker**: rejects `f(a - 0.0)` and mutual recursion.
- **`let`-binding proof context**: manifest literal values bound by `let` now
  enter the linear constraint context so `a / x` verifies when `x` was bound
  to a nonzero literal.
- **Rust-keyword escaping** in codegen: identifiers that are Rust keywords are
  emitted with the `r#` raw-identifier prefix.
- **Non-finite float literals** in codegen: `inf`/`NaN` emit `f64::INFINITY` /
  `f64::NAN` instead of invalid `inf.0`/`NaN.0`.
- **`eidos_sqrt` precision fix**: scales `x` into `[1, 4)` before Newton
  iterations to maintain full `f64` precision across extreme magnitudes.
- **`--force` flag** for `eidos build`: opt-in clobber of non-empty `--out-dir`.
- **`docs/getting-started.md`**, **`docs/language-tour.md`**,
  **`docs/cookbook.md`**: end-to-end guides.
- **`CONTRIBUTING.md`**: contribution policy (Issues only).

### Fixed
- `eidos build` no longer unconditionally overwrites `--out-dir` contents
  (requires `--force`).
- Eraser no longer hardcodes the refinement bind name `"v"`.
- `.map`/`.zip` with a missing argument now returns `Err` instead of
  panicking or emitting silently-invalid Rust.
- `crate_name` sanitizes digit-leading and all-non-alphanumeric file stems.
- Silent saturating cast for `Array<T, N>` length replaced with a range check.

## [0.1.0] — Minimal Viable Kernel (MVK)

### Added
- `tpt-eidos-parser`: lexer, recursive-descent parser, AST for the eidos
  surface language.
- `tpt-eidos-kernel`: refinement subtyping, division-safety checking, and
  structural-recursion termination checking.
- `tpt-eidos-verifier`: QF_LRA Fourier-Motzkin decision procedure
  (`unsat`/`entails`/`model`/`counterexample`).
- `tpt-eidos-erasure`: proof-term erasure to a computational-core IR.
- `tpt-eidos-codegen`: lowers the erased core to a `#![no_std]` Rust crate.
- `tpt-eidos-flight-math`: pre-proved flight-control domain library
  (DCM/quaternion normalization, PID bounds, `safe_direction`,
  `quat_normalize`, `pid_linear`).
- `tpt-eidos-cli`: `eidos check` and `eidos build` subcommands.
- `examples/calibrate_gyro.eidos` and `examples/calibrate_gyro_broken.eidos`:
  worked example and regression fixture from `spec.txt §4`.
- `examples/attitude_control.eidos`: flight-control control law verified under
  the domain-library lemma set.
