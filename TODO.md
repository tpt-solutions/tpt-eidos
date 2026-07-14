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
- [x] Resolve the non-linear arithmetic question: Fourier-Motzkin only covers QF_LRA.
       Decision — **axiomatized trusted lemmas** (the `TrustedLemmas` boundary in
       `eidos-kernel`). Interval-arithmetic approximation was rejected: it adds a
       whole numeric-domain engine for what the MVK needs in a handful of textbook
       facts, and generalizes poorly to quaternion/trig obligations. A heavier SMT
       solver (e.g. dReal/Z3) was rejected to keep the TCB pure-`std` and CI offline.
       Non-linear facts (e.g. "normalize-by-own-magnitude → unit vector") are admitted
       by named, reviewable lemmas whose use is recorded in `Report::obligations`, so
       every trusted step is traceable. See `eidos-kernel::lemma_normalized_vector`
       and `eidos-flight-math::FLIGHT_LEMMAS`.
- [x] `tpt-eidos-flight-math`: pre-proved DCM↔quaternion normalization, PID bounds, and
       other common flight-control primitives. Ships `PRIMITIVES_EIDOS`
       (`safe_direction`, `quat_normalize`, `pid_linear`) verified against the
       domain-lemma set, plus `check_module`/`check_source` entry points that combine
       the kernel defaults with the flight lemmas. (`crates/eidos-flight-math`)
- [x] **Milestone:** a real flight-control control law (`examples/attitude_control.eidos`)
       verifies under the domain-library lemma set and erases to clean `no_std` Rust.
       Covered by `eidos-tests::attitude_control_verifies_with_domain_library` and
       `eidos-tests::attitude_control_emits_no_std_rust`.

## Phase 4: AI-Assisted Proof Synthesis
- [x] Reintroduce a `CodeAgent`-style loop adapted to suggest kernel proof steps
       instead of whole-function bodies. `eidos-flight-math::prover` exposes
       `suggest_and_verify`, which applies agent-proposed steps (`StrengthenRequires`,
       `ApplyLemma`) to a fresh module copy and re-verifies with the kernel.
       (`crates/eidos-flight-math/src/prover.rs`)
- [x] **Milestone:** an LLM-suggested proof step is mechanically verified or rejected by
       the kernel — never trusted without kernel approval. Covered by
       `eidos-tests::proof_suggestion_accepted_and_rejected` (a sound `requires`
       strengthening is accepted; a bound that still admits `x == 0` is rejected) and
       `eidos-flight-math::prover::tests` (agent lemma only accepted when it actually
       discharges the obligation).

## Open design questions
- [x] Effect system: `effects [IO, RealTime<2ms>]` in the spec implies a real-time budget
       checker, not just an effect *label*. **Decision — purely descriptive in the MVK.**
       Effect labels are parsed and carried through the AST/IR for documentation and
       future checking, but no WCET proof is required in v1 (out of MVK scope; marked
       directional in `spec.txt`).
- [x] Linear/affine resource types (hardware register locks, sensor buffers): **decision —
       lean on Rust's move semantics at the erasure target for v1.** The MVK does not
       enforce a from-scratch linearity checker; owned `no_std` Rust values already give
       the move/borrow guarantees needed, and a dedicated affine type layer is deferred.

## Feasibility note
Building the literal full spec — a Lean4/Coq-grade kernel, general totality checking, a full
effect system, and DO-178C-certifiable output — is multi-year, expert-PL-team-scale work (on
the order of what Lean4/Coq/Idris2/F* each took). DO-178C certification is also a regulatory
process, not just code. The scoped MVK above (refinement types + SMT, not full dependent
pattern matching) is a well-trodden, tractable pattern (Dafny/F*/Liquid Haskell's approach)
and satisfies every example in the spec, so it's a realistic first target even though the
later phases stay directional until Phase 1-2 are real.

## Phase 5: Hardening — full test coverage
- [ ] `eidos-kernel`: add termination-checker tests — **currently zero tests exist for it,
      positive or negative** — despite it being a headline MVK invariant. Cover: a valid
      structurally-decreasing recursive function (must accept), a non-decreasing self-call
      (must reject), and mutual recursion between two functions (currently unchecked).
- [ ] `eidos-kernel`: add a test proving `a % b` is currently unguarded by division-safety
      checking (regression test for bug #1 below; flip to a positive test once fixed).
- [ ] `eidos-kernel`: add tests for nested if/else path-constraint propagation, a
      contradictory `requires` clause, and an isolated `Lemma`/`apply_to` test that doesn't
      go through `DEFAULT_LEMMAS`.
- [ ] `eidos-kernel`: add a test for `let`-bound values not entering the proof context
      (regression test for bug #8).
- [ ] `eidos-parser`: add tests that trigger each `ParseError` variant and assert on the
      message — today every parser test is a happy-path `.unwrap()`. Cover: unexpected EOF,
      unexpected token, invalid number literal.
- [ ] `eidos-parser`: add tests for operator precedence/associativity, lambda/tuple
      patterns, and `effects [...]` parsing (a grammar feature named in `AGENTS.md` with no
      dedicated test), plus a direct test of the public `parse_expr` entry point.
- [ ] `eidos-verifier`: either wire up `LinExpr::variables()` to something or remove it —
      it's currently dead code, never called anywhere in the workspace.
- [ ] `eidos-verifier`: add tests with 3+ variables, degenerate/unbounded constraint
      systems, and cases exercising the `EPS = 1e-9` boundary directly.
- [ ] `eidos-erasure`/`eidos-codegen`: add a test using a refinement bind name other than
      `"v"` (regression test for bug #9), a `.map`/`.zip` call with a missing argument
      (regression test for bug #10), a record literal not immediately wrapped in a `Cast`,
      and a field/function name colliding with a Rust keyword (regression test for #13).
- [ ] `eidos-codegen`: add a test that actually **executes** generated code and asserts on
      the runtime result (e.g. `eidos_sqrt`/`eidos_magnitude` on known inputs, including
      extreme magnitudes — regression test for bug #11) — today coverage only checks that
      generated code compiles, never that it computes the right answer. Also add a
      non-finite float literal test (regression test for bug #12).
- [ ] `eidos-cli`: add unit/E2E tests for no-args/usage, an unknown subcommand, a missing
      file path, missing `--out-dir`, and `crate_name`'s sanitization edge cases
      (regression test for bug #16).
- [ ] `eidos-flight-math`: fix or rename `primitives_rejected_without_domain_env` — it
      doesn't currently test what its name claims (see notes). Add a negative test showing
      `lemma_triangle_for_add` accepts an obviously-false bound (regression test for bug
      #6), and a test of a malformed `extra` expression string reaching
      `suggest_and_verify`'s error path.
- [ ] Add adversarial/negative example fixtures under `examples/`: a broken flight-math
      case, a recursive-but-non-terminating function, an `Array<T,N>` size mismatch, and an
      `effects [...]` example — every existing fixture today is a single-reason
      accept/reject case.
- [ ] Wire up `cargo llvm-cov --workspace --fail-under-lines 75` in CI — already flagged in
      Phase 1 as a stretch goal "not wired into CI yet."
- [ ] **Open question:** add property-based/fuzz-style tests for the parser (arbitrary
      strings must never panic/hang) and the verifier (arbitrary constraint systems must
      terminate) — directly relevant to bugs #3/#4. The "pure std, no external crates"
      convention rules out `proptest`/`cargo-fuzz` as *regular* dependencies, but they could
      still be added as **dev-dependencies only** (test-only, never shipped in the trusted
      binary). Needs a decision before scheduling this work.

## Known bugs / soundness gaps
- [ ] **[High]** `%` (modulo) is completely exempt from division-safety checking —
      `eidos-kernel/src/lib.rs:179-184` only calls `check_division` for `BinOp::Div`, never
      for `BinOp::Rem`. `x % y` with an unguarded `y` verifies with zero obligations.
- [ ] **[High]** Termination checker is nearly decorative —
      `eidos-kernel/src/lib.rs:560-580,624-626` only rejects a recursive call whose
      arguments are syntactically identical to the parameters; `f(a - 0.0)` or mutual
      recursion between two functions both pass today.
- [ ] **[High]** Fourier-Motzkin elimination has no complexity/depth guard (DoS) —
      `eidos-verifier/src/lib.rs:166-211,259-332`; each elimination step can roughly square
      the constraint count, with no fuel limit, on a decision procedure invoked for every
      `requires`/`if`/division/`ensures` obligation derived from source text.
- [ ] **[High/Medium-High]** Unbounded recursion depth in the parser (stack-overflow DoS) —
      `eidos-parser/src/lib.rs`'s expression grammar has no depth counter anywhere
      (parens, unary chains, array/record literals, lambda bodies all recurse freely); the
      same pattern repeats in the kernel's `walk`/`subst`/`simplify`/`linearize`.
- [ ] **[Medium-High]** The Phase-4 agent-proposal path
      (`eidos-flight-math/src/prover.rs:40-85`) feeds an untrusted external string straight
      through `parse_expr` and `check_module_with`, inheriting the two DoS surfaces above by
      design, not just by malformed-file accident.
- [ ] **[Medium]** `triangle_for_add` agent lemma admits an unconstrained bound —
      `eidos-flight-math/src/lib.rs:74-85` matches `(a+b).magnitude() <= K` for any `K` with
      zero side conditions checking `K >= |a| + |b|` — the one visible hole in "an
      agent-suggested proof step is never trusted without kernel approval."
- [ ] **[Medium]** Fixed-epsilon floating point (`EPS = 1e-9`,
      `eidos-verifier/src/lib.rs:10`) is the sole soundness oracle for the trusted decision
      procedure; no exact rational arithmetic is used anywhere.
- [ ] **[Medium]** `let`-bindings never enter the linear proof context —
      `eidos-kernel/src/lib.rs:197-200`; `let x = 5.0; return a / x;` spuriously fails to
      verify even though `x` is a manifest nonzero literal.
- [ ] **[Medium]** Eraser hardcodes the refinement bind name `"v"` —
      `eidos-erasure/src/lib.rs:368`; a record using any other bind name, not immediately
      wrapped in a `Cast`, produces invalid Rust from `gen_record`.
- [ ] **[Medium]** `.map`/`.zip` with a missing argument breaks codegen two different bad
      ways — `eidos-codegen/src/lib.rs:222` (`map`) panics the whole `eidos build` process;
      `zip` (`lines 230-233`) silently emits invalid Rust and reports success.
- [ ] **[Medium]** `eidos_sqrt`'s fixed 32-iteration Newton method
      (`eidos-codegen/src/lib.rs` prelude) can be inaccurate for extreme magnitudes — a gap
      between what the kernel proves (exact real arithmetic) and what generated code
      actually computes at runtime.
- [ ] **[Medium]** Non-finite float literals produce invalid Rust —
      `eidos-codegen/src/lib.rs:276-283` (`float_lit`) turns `inf`/`NaN` into `inf.0`/`NaN.0`.
- [ ] **[Low-Medium]** No Rust-keyword escaping for emitted identifiers — codegen never
      uses `r#ident`, so a field/function named e.g. `loop` produces uncompilable Rust.
- [ ] **[Medium]** `eidos build` unconditionally overwrites `--out-dir` contents —
      `eidos-cli/src/main.rs:121-132`; no `--force` gate, no check for pre-existing content.
- [ ] **[Low]** Silent saturating cast for `Array<T, N>` length —
      `eidos-parser/src/lib.rs:351` (`n as u64`); `Array<f64, 1e30>` saturates to
      `u64::MAX` instead of erroring.
- [ ] **[Low]** `crate_name` can emit an invalid Cargo package name —
      `eidos-cli/src/main.rs:40-54` for file stems that are all-non-alphanumeric or start
      with a digit.
- [ ] **[Low]** `unreachable!()` relies on an unenforced invariant —
      `eidos-verifier/src/lib.rs:172,265` assume only `Le`/`Lt` ever reach these arms, with
      no type-level guarantee.

## Ideas: ease of use / innovation (directional, not scheduled)
- Parse errors carry zero position info (no line/column anywhere in `ParseError`) — the
  single biggest DX gap for a language meant for careful, safety-critical authoring.
  `CheckError`/`Obligation` diagnostics are equally locationless.
- No `--help`/`-h` or `--version`/`-V` on the CLI — `eidos --help` currently falls into the
  "unknown subcommand" error path with a nonzero exit code.
- Missing subcommands users would reasonably expect: `eidos fmt`, `eidos new <name>`
  (scaffold), `eidos test` (batch-check a directory), `eidos build --run` (auto-invoke
  `cargo build`/`run` on the emitted crate), and a `--emit=ast|core` debug-dump flag.
- No `--json` output mode on `eidos check` for machine/editor consumption, no batch/glob
  mode (`eidos check src/**/*.eidos`).
- `eidos check` only reports an aggregate "N verified, M trusted-lemma" count; a
  `--verbose`/`--explain` mode showing the `Verified`/`Trusted` status of every individual
  obligation would make the solver-proven-vs-admitted-axiom distinction visible, which
  matters a lot for this project's "proof-native" trust story.
- Counterexample reporting is inconsistent: `check_division` attaches a counterexample
  model to its error; the general `discharge` path (used for `ensures`/refinement
  obligations) doesn't, even though `eidos_verifier::counterexample` is already available.
- No provenance header/fingerprint in generated Rust tying it back to the exact verified
  source (relevant to the project's DO-178C-traceability aspirations in `spec.txt`).
- No LSP/editor integration yet (naturally deferred, but worth naming alongside Phase 4,
  mirroring what `tpt-telos` eventually had).
