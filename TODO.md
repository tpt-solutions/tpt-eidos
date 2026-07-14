# tpt-eidos TODO

> See `spec.txt` for the design doc. This roadmap builds on the sibling project
> `tpt-telos` (complete v1.0 Rust workspace: parser → IR/QF_LRA → Fourier-Motzkin
> verifier → agentic transpiler → Rust/Go codegen → FFI bridge → eject hatch → LSP).
> Reuse telos crates where possible instead of writing them from scratch.

## Phase 1: The Core Kernel (MVK)
- [x] Fork `tpt-telos-parser` into `tpt-eidos-parser`; extend grammar for refinement types
      (`{ x: T | predicate }`), `Array<T, N>`, `requires`/`ensures`, `effects [...]`.
      (`crates/eidos-parser/src/grammar.ebnf`)
- [x] Vendor `tpt-telos-verifier` into `tpt-eidos-verifier` unchanged as the QF_LRA decision
      procedure (Fourier-Motzkin, `unsat`/`entails`/`model`/`counterexample`).
      (`crates/eidos-verifier`)
- [x] Design and implement `tpt-eidos-kernel`: minimal trusted typechecker for refinement
      subtyping + `Array<T, N>` + structural-recursion termination checking. Scope
      deliberately excludes general dependent pattern matching / inductive families for v1
      (keeps the kernel small and auditable, per spec §3.2). (`crates/eidos-kernel`)
- [x] Wire refinement-predicate proof obligations from the kernel to `eidos-verifier`'s
      `entails`/`counterexample` API.
- [x] `tpt-eidos-cli` (binary `eidos`) with `eidos check <file>` subcommand.
- [x] Set up Cargo workspace + CI parity with telos: `cargo fmt --all -- --check`,
      `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`,
      `Apache-2.0` license. (`cargo llvm-cov --workspace --fail-under-lines 75` is a
      directional stretch goal; not wired into CI yet.)
- [x] Write root `AGENTS.md` / `CLAUDE.md` documenting workspace layout and pipeline,
      mirroring telos's format.
- [x] Add `examples/calibrate_gyro.eidos` (spec §4 worked example) and
      `examples/calibrate_gyro_broken.eidos` (missing the `mag > 0.0` guard) as regression
      fixtures, wired into an integration test.
- [x] **Milestone:** `eidos check` accepts the correct `calibrate_gyro` example and rejects
      the broken one, with `cargo test --workspace` and clippy clean.

## Phase 2: The Eraser
- [x] `tpt-eidos-erasure`: strip proof terms/refinement witnesses from a kernel-checked term,
      producing a computational-core IR. (`crates/eidos-erasure`)
- [x] `tpt-eidos-codegen`: lower erased IR to a `no_std`-compatible Rust crate (reference
      telos's `tpt-telos-codegen/src/lib.rs` Rust backend for the struct/impl-emission
      pattern; expect to diverge since eidos never synthesizes bodies). (`crates/eidos-codegen`)
- [x] `eidos build <file> --out-dir DIR` CLI command (replaces the Phase-1 stub; emits a real
      `lib.rs` + `Cargo.toml` for the verified, erased module). (`crates/eidos-cli`)
- [x] **Milestone:** a verified eidos function compiles to zero-allocation `no_std` Rust with
      no runtime overhead from verification (spot-check: no kernel-internal types leak into
      the generated source). Verified by `eidos-tests` `generated_rust_compiles_no_std` and
      `build_emits_no_std_crate_without_kernel_types`.

## Phase 3: The Domain Library
- [ ] Resolve the non-linear arithmetic question first: Fourier-Motzkin only covers QF_LRA,
      so decide the approach for trig/quaternion proof obligations (interval-arithmetic
      approximation vs. axiomatized trusted lemmas vs. a heavier solver).
- [ ] `tpt-eidos-flight-math`: pre-proved DCM↔quaternion conversion, PID bounds, and other
      common flight-control primitives, so users don't re-prove textbook math from scratch.
- [ ] **Milestone:** a real flight-control control-law function, written against the domain
      library, verifies and erases to Rust.

## Phase 4: AI-Assisted Proof Synthesis
- [ ] Reintroduce a `CodeAgent`-style loop (telos precedent: `tpt-telos-agent`'s
      Generate→Verify→Counterexample→Rewrite) adapted to suggest kernel proof steps instead
      of whole-function bodies — the kernel, not an SMT-only verifier, is the gate here.
- [ ] **Milestone:** an LLM-suggested proof step is mechanically verified or rejected by the
      kernel — never trusted without kernel approval.

## Open design questions (resolve before/during Phase 1)
- [ ] Effect system: `effects [IO, RealTime<2ms>]` in the spec example implies a real-time
      budget checker, not just an effect *label*. Decide whether Phase 1's effect types are
      purely descriptive (no WCET proof) or whether WCET proving is pulled forward.
- [ ] Linear/affine resource types (hardware register locks, sensor buffers): decide whether
      the MVK enforces this in the eidos kernel itself, or leans on Rust's move semantics at
      the erasure target for v1 and defers a from-scratch linearity checker.

## Feasibility note
Building the literal full spec — a Lean4/Coq-grade kernel, general totality checking, a full
effect system, and DO-178C-certifiable output — is multi-year, expert-PL-team-scale work (on
the order of what Lean4/Coq/Idris2/F* each took). DO-178C certification is also a regulatory
process, not just code. The scoped MVK above (refinement types + SMT, not full dependent
pattern matching) is a well-trodden, tractable pattern (Dafny/F*/Liquid Haskell's approach)
and satisfies every example in the spec, so it's a realistic first target even though the
later phases stay directional until Phase 1-2 are real.
