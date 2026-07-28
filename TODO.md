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
- [x] `eidos-kernel`: add termination-checker tests — **currently zero tests exist for it,
      positive or negative** — despite it being a headline MVK invariant. Cover: a valid
      structurally-decreasing recursive function (must accept), a non-decreasing self-call
      (must reject), and mutual recursion between two functions (currently unchecked).
- [x] `eidos-kernel`: add a test proving `a % b` is currently unguarded by division-safety
      checking (regression test for bug #1 below; flip to a positive test once fixed).
- [x] `eidos-kernel`: add tests for nested if/else path-constraint propagation, a
      contradictory `requires` clause, and an isolated `Lemma`/`apply_to` test that doesn't
      go through `DEFAULT_LEMMAS`.
- [x] `eidos-kernel`: add a test for `let`-bound values not entering the proof context
      (regression test for bug #8).
- [x] `eidos-parser`: add tests that trigger each `ParseError` variant and assert on the
      message — today every parser test is a happy-path `.unwrap()`. Cover: unexpected EOF,
      unexpected token, invalid number literal.
- [x] `eidos-parser`: add tests for operator precedence/associativity, lambda/tuple
      patterns, and `effects [...]` parsing (a grammar feature named in `AGENTS.md` with no
      dedicated test), plus a direct test of the public `parse_expr` entry point.
- [x] `eidos-verifier`: either wire up `LinExpr::variables()` to something or remove it —
      it's currently dead code, never called anywhere in the workspace.
- [x] `eidos-verifier`: add tests with 3+ variables, degenerate/unbounded constraint
      systems, and cases exercising the `EPS = 1e-9` boundary directly.
- [x] `eidos-erasure`/`eidos-codegen`: add a test using a refinement bind name other than
      `"v"` (regression test for bug #9), a `.map`/`.zip` call with a missing argument
      (regression test for bug #10), a record literal not immediately wrapped in a `Cast`,
      and a field/function name colliding with a Rust keyword (regression test for #13).
- [x] `eidos-codegen`: add a test that actually **executes** generated code and asserts on
      the runtime result (e.g. `eidos_sqrt`/`eidos_magnitude` on known inputs, including
      extreme magnitudes — regression test for bug #11) — today coverage only checks that
      generated code compiles, never that it computes the right answer. Also add a
      non-finite float literal test (regression test for bug #12).
- [x] `eidos-cli`: add unit/E2E tests for no-args/usage, an unknown subcommand, a missing
      file path, missing `--out-dir`, and `crate_name`'s sanitization edge cases
      (regression test for bug #16).
- [x] `eidos-flight-math`: fix or rename `primitives_rejected_without_domain_env` — it
      doesn't currently test what its name claims (see notes). Add a negative test showing
      `lemma_triangle_for_add` accepts an obviously-false bound (regression test for bug
      #6), and a test of a malformed `extra` expression string reaching
      `suggest_and_verify`'s error path.
- [x] Add adversarial/negative example fixtures under `examples/`: a broken flight-math
      case, a recursive-but-non-terminating function, an `Array<T,N>` size mismatch, and an
      `effects [...]` example — every existing fixture today is a single-reason
      accept/reject case.
- [x] Wire up `cargo llvm-cov --workspace --fail-under-lines 75` in CI — already flagged in
      Phase 1 as a stretch goal "not wired into CI yet."
- [x] **Open question (resolved):** add property-based/fuzz-style tests for the parser
       (arbitrary strings must never panic/hang) and the verifier (arbitrary constraint
       systems must terminate) — directly relevant to bugs #3/#4. **Decision:** implement as
       **dependency-free, pure-`std` fuzz harnesses** (a small deterministic xorshift PRNG,
       no `proptest`/`cargo-fuzz` even as dev-deps) so the TCB and the dev-tooling stay
       external-crate-free and CI stays offline. Added `crates/eidos-parser/tests/fuzz.rs`
       (4000 random sources + adversarial deep-paren/unary/unterminated cases) and
       `crates/eidos-verifier/tests/fuzz.rs` (random constraint systems, degenerate cases,
       a guard-stress case). These also **caught a real regression**: the `MAX_CONSTRAINTS`
       guard was checked *after* building the next constraint set, so an adversarial system
       could allocate hundreds of millions of constraints before the guard fired; fixed by
       bailing *before* the `uppers.len() * lowers.len()` construction in both `fm_unsat`
       and `solve`.

## Known bugs / soundness gaps
- [x] **[High]** `%` (modulo) is completely exempt from division-safety checking —
      `eidos-kernel/src/lib.rs:179-184` only calls `check_division` for `BinOp::Div`, never
      for `BinOp::Rem`. `x % y` with an unguarded `y` verifies with zero obligations.
- [x] **[High]** Termination checker is nearly decorative —
      `eidos-kernel/src/lib.rs:560-580,624-626` only rejects a recursive call whose
      arguments are syntactically identical to the parameters; `f(a - 0.0)` or mutual
      recursion between two functions both pass today.
- [x] **[High]** Fourier-Motzkin elimination has no complexity/depth guard (DoS) —
      `eidos-verifier/src/lib.rs:166-211,259-332`; each elimination step can roughly square
      the constraint count, with no fuel limit, on a decision procedure invoked for every
      `requires`/`if`/division/`ensures` obligation derived from source text.
- [x] **[High/Medium-High]** Unbounded recursion depth in the parser (stack-overflow DoS) —
      `eidos-parser/src/lib.rs`'s expression grammar has no depth counter anywhere
      (parens, unary chains, array/record literals, lambda bodies all recurse freely); the
      same pattern repeats in the kernel's `walk`/`subst`/`simplify`/`linearize`.
- [x] **[Medium-High]** The Phase-4 agent-proposal path
      (`eidos-flight-math/src/prover.rs:40-85`) feeds an untrusted external string straight
      through `parse_expr` and `check_module_with`, inheriting the two DoS surfaces above by
      design, not just by malformed-file accident.
- [x] **[Medium]** `triangle_for_add` agent lemma admits an unconstrained bound (regression test pinned in `eidos-flight-math`: `triangle_for_add_accepts_false_bound`; still unsound by design, gated behind the agent loop) —
      `eidos-flight-math/src/lib.rs:74-85` matches `(a+b).magnitude() <= K` for any `K` with
      zero side conditions checking `K >= |a| + |b|` — the one visible hole in "an
      agent-suggested proof step is never trusted without kernel approval." **Fixed as part of
      Phase 7c**: retired the lemma in favor of `ProposeNonlinearCertificate` + `SosCertificate`.
- [x] **[Medium]** Fixed-epsilon floating point (`EPS = 1e-9`, formerly
      `eidos-verifier/src/lib.rs:10`) was the sole soundness oracle for the trusted decision
      procedure; no exact rational arithmetic was used anywhere — fixed by adding an internal
      exact-rational core (`eidos-verifier/src/lib.rs`'s private `rat` module: `Rat`, an
      `i128` numerator/denominator fraction always kept reduced) and rerouting
      `fm_unsat`/`solve` through it. Every `f64` literal reaching the solver is losslessly
      decomposed into its exact IEEE-754 fraction (`Rat::from_f64`); all elimination-round
      arithmetic (`add`/`sub`/`scale`) is now exact, so satisfiability decisions no longer
      depend on floating-point rounding and `EPS` is gone from the decision procedure (it
      survives only as a display tolerance in a couple of tests, for the final `Rat -> f64`
      conversion). `checked_add`/`checked_mul` combine via LCM/cross-GCD reduction rather
      than a naive `d1*d2` product — Fourier-Motzkin repeatedly re-derives bounds sharing a
      common denominator factor, and the naive product would square that shared factor at
      every combination step, overflowing `i128` almost immediately; reducing through the GCD
      first keeps results only as large as the algebra actually requires. Any operation that
      would still overflow `i128` (or a literal that can't be represented exactly) bails
      conservatively toward "unverified", matching the `MAX_CONSTRAINTS` guard's existing
      philosophy elsewhere in the module. Regression tests
      (`exact_boundary_lt_excludes_tiny_positive`, `exact_boundary_le_rejects_tiny_positive`)
      pin the fixed behavior: `1e-9` used to be silently tolerated as "close enough to zero"
      under the old fudge factor and is now correctly excluded.
- [x] **[Medium]** `let`-bindings never enter the linear proof context —
      `eidos-kernel/src/lib.rs:197-200`; `let x = 5.0; return a / x;` spuriously fails to
      verify even though `x` is a manifest nonzero literal.
- [x] **[Medium]** Eraser hardcodes the refinement bind name `"v"` —
      `eidos-erasure/src/lib.rs:368`; a record using any other bind name, not immediately
      wrapped in a `Cast`, produces invalid Rust from `gen_record`.
- [x] **[Medium]** `.map`/`.zip` with a missing argument breaks codegen two different bad
      ways — `eidos-codegen/src/lib.rs:222` (`map`) panics the whole `eidos build` process;
      `zip` (`lines 230-233`) silently emits invalid Rust and reports success.
- [x] **[Medium]** `eidos_sqrt`'s fixed 32-iteration Newton method
      (`eidos-codegen/src/lib.rs` prelude) can be inaccurate for extreme magnitudes — a gap
      between what the kernel proves (exact real arithmetic) and what generated code
      actually computes at runtime.
- [x] **[Medium]** Non-finite float literals produce invalid Rust —
      `eidos-codegen/src/lib.rs:276-283` (`float_lit`) turns `inf`/`NaN` into `inf.0`/`NaN.0`.
- [x] **[Low-Medium]** No Rust-keyword escaping for emitted identifiers — codegen never
      uses `r#ident`, so a field/function named e.g. `loop` produces uncompilable Rust.
- [x] **[Medium]** `eidos build` unconditionally overwrites `--out-dir` contents —
      `eidos-cli/src/main.rs:121-132`; no `--force` gate, no check for pre-existing content.
- [x] **[Low]** Silent saturating cast for `Array<T, N>` length —
      `eidos-parser/src/lib.rs:351` (`n as u64`); `Array<f64, 1e30>` saturates to
      `u64::MAX` instead of erroring.
- [x] **[Low]** `crate_name` can emit an invalid Cargo package name —
      `eidos-cli/src/main.rs:40-54` for file stems that are all-non-alphanumeric or start
      with a digit.
- [x] **[Low]** `unreachable!()` relied on an unenforced invariant (defensive: `Constraint::normalize` only ever emitted `Le`/`Lt`, so these arms were unreachable in practice, with no type-level guarantee) —
      fixed by introducing a dedicated `NormRel { Le, Lt }` type for the solver's internal
      `Norm` representation (`eidos-verifier/src/lib.rs`), so the `Le`/`Lt`-only invariant is
      now enforced by the type checker instead of an `unreachable!()` arm; `Rel` (which still
      has `Ge`/`Gt`/`Eq`) remains the public constraint-construction API and is reduced to
      `NormRel` only inside `Constraint::normalize`.

## Phase 6: Hardening & adoption
- [x] Thread source spans (line/col) from the lexer through `ParseError`, and from the AST
      through `eidos-kernel`'s `CheckError`/`Obligation`, so `eidos check`/`eidos build`
      report `path:line:col: message` instead of a bare message. Touches
      `eidos-parser/src/ast.rs` (`Span`, wrap `Expr` as `{ kind: ExprKind, span: Span }`),
      `eidos-parser/src/lib.rs` (lexer + parser), `eidos-kernel/src/lib.rs`,
      `eidos-erasure`/`eidos-codegen`/`eidos-flight-math` (mechanical `.kind` match updates),
      and `eidos-cli/src/main.rs` (error rendering).
- [x] `eidos new <name>` scaffold subcommand (`eidos-cli`): writes a minimal
      `calibrate_gyro`-style starter `.eidos` file.
- [x] `docs/getting-started.md`: an end-to-end walkthrough of `eidos check`/`eidos build`
      over `examples/calibrate_gyro.eidos` and `examples/calibrate_gyro_broken.eidos`,
      linked from `README.md`.
- [x] Root `CONTRIBUTING.md`: contributions go through GitHub Issues only.
- [x] Rename `crates/eidos-*` directories to `crates/tpt-eidos-*` to match the published
      crate names; update root `Cargo.toml` workspace members/paths and `AGENTS.md`'s
      workspace-layout tree.
- [x] Give each crate its own crates.io `keywords`/`categories` in its `Cargo.toml` instead
      of inheriting the shared `workspace.package` list.
- [x] `tpt-eidos-controls-math`: a second, non-aerospace domain-library crate (generic
      control-systems primitives — output clamping, rate limiting) reusing the
      `Lemma`/`TrustedLemmas` pattern from `eidos-flight-math`, with an `examples/` fixture
      and an `eidos-tests` integration test.

## Phase 7: Path to full spec (v2)
> `spec.txt`'s full vision has three pillars the MVK deliberately doesn't attempt yet:
> real-time (WCET) budget checking, a linearity/affine checker for hardware resources, and
> genuine non-linear arithmetic (beyond the fixed named-lemma table). Each is roughly
> crate-sized and touches the trusted kernel, so they're tracked and designed here
> individually rather than bundled — 7c lands first (this pass); 7a/7b are design-only,
> each its own future implementation session, so a soundness issue in one can't taint the
> others.

### 7a. WCET / `RealTime<Nms>` budget checker (design only)
- [x] Extend the `effects` grammar (`eidos-parser/src/grammar.ebnf:12`, currently
      `effects = "effects", "[", ident, { ",", ident }, "]"` — bare idents only,
      `RealTime<2ms>` does not actually parse today) to accept a parameterized
      `effect_atom = ident, [ "<", number, ident, ">" ]`; `Fun.effects` becomes
      `Vec<Effect>` (`Effect { name, budget: Option<(f64, TimeUnit)> }`).
- [x] `eidos-kernel`: a `CostModel` table of abstract cost units per `BinOp`/`UnOp`/known
      method call (`.magnitude()`, `.map`, `eidos_sqrt`'s 32 Newton iterations, ...);
      compute a worst-case cost bound by structural recursion over the expression tree.
      `if/else` becomes two obligations (`cost(then) <= budget`, `cost(else) <= budget`),
      reusing the existing `entails`/`unsat` linear-obligation machinery unchanged — no new
      solver needed.
- [x] Restrict v1 to non-recursive functions (recursive WCET bounding is a known hard
      problem); `RealTime<N>` on a recursive function is a hard rejection, not a silent
      skip.
- [x] Document as an **abstract-cost proxy**, not a hardware-certified timing bound —
      consistent with the project's existing DO-178C honesty in the Feasibility note above.
- [x] **Milestone:** a function declaring `effects [RealTime<2ms>]` whose structural cost
      exceeds a configured budget is rejected with a named WCET obligation failure; one
      that fits is accepted.

### 7b. Linearity/affine checker (design only)
- [x] Surface syntax: a `linear` type modifier (e.g. `fn lock(r: linear Register) -> Unit`);
      AST `Type` gains a `Linear(Box<Type>)` variant.
- [x] New kernel pass `check_linearity(fun)`: tracks per-path usage counts for every
      `linear`-typed binding. Sequential composition sums usage; `if/else` requires
      **exactly one** use of each live linear variable on **both** branches (the same
      substructural merge rule Rust's own move checker applies under conditionals — this
      finally puts that guarantee in eidos source itself, not just at the erasure target,
      closing the Phase-1 "lean on Rust's move semantics" deferral under Open design
      questions above). `Lambda` bodies (`.map`/`.zip` closures) may not capture a linear
      variable (the closure conceptually runs once per array element) — rejected outright.
      Reject 0 uses ("unused linear resource") and 2+ uses on one path ("used more than
      once").
- [x] **Milestone:** a `linear` parameter used twice, unused, or captured inside `.map` is
      rejected; used exactly once on every path is accepted.

### 7c. Non-linear arithmetic via checked certificates — implemented this pass
> Answers "could we do better than Z3?": embedding Z3 would contradict the TCB-purity
> decision already on record above ("a heavier SMT solver... was rejected to keep the TCB
> pure-`std` and CI offline") — Z3 is ~1M lines of unauditable C++, the opposite of
> `spec.txt` §3.2's "transparent, minimal kernel" pillar. The better answer reuses the same
> Generate→Verify split the project already uses for Phase-4 LLM proof suggestions: let
> *anything* (a human, an LLM, or an out-of-band Z3/dReal run) **propose** a certificate;
> trust nothing but a small, in-repo, exact-rational **checker**. This is strictly stronger
> than trusting Z3 directly (Z3's own soundness is never part of the TCB either way) and
> strictly more general than the fixed named-lemma table (covers any provable polynomial
> fact, not just hand-picked ones).
- [x] `eidos-verifier`: new private `poly` module — a sparse multivariate polynomial over
      the existing exact `Rat` type, with exact `add`/`sub`/`mul`/`eval`.
- [x] `eidos-verifier`: public `SosCertificate` type proving `bound - expr = Σ c_i * s_i²`
      for proposer-supplied polynomials `s_i` and nonnegative rational coefficients `c_i`;
      public `check_sos_certificate(claim, cert) -> bool` expands and checks the identity
      exactly over `Rat` — pure arithmetic, no search, the entire new trusted surface.
- [x] `eidos-flight-math::prover`: `ProofStep` gains `ProposeNonlinearCertificate`, checked
      exactly like today's `StrengthenRequires`/`ApplyLemma` — never trusted without the
      verifier's independent check.
- [x] Retire the unsound `triangle_for_add` lemma (see "Known bugs" above) in favor of a
      real `SosCertificate` for the triangle-inequality bound — closes that soundness gap
      and is the first real exercise of the new mechanism.
- [x] Tests: `check_sos_certificate` accepts a correct triangle-inequality certificate and
      rejects a negative-coefficient certificate, a mismatched-expansion certificate, and
      the old false-`K` case `triangle_for_add_accepts_false_bound` used to (wrongly)
      accept.
- [x] **Milestone:** general kernel-level "try a certificate for any non-linear obligation"
      (beyond the agent-proposal path) — good follow-up, not required for this pass's
      landing.

## Ideas: ease of use / innovation (directional, not scheduled)
- [x] Parse errors carry zero position info — **resolved in Phase 6**: `ParseError`
  (`eidos-parser`) and `CheckError`/`Obligation` (`eidos-kernel`) now carry `Span`/
  line-col, and `eidos check`/`eidos build` report `path:line:col: message`.
- [x] `--help`/`-h`/`--version`/`-V` on the CLI: both flags are handled in
  `eidos-cli/src/main.rs::run`, tested by `help_flag_succeeds`/`version_flag_succeeds`.
- [x] Missing subcommands users would reasonably expect: `eidos fmt` (normalize whitespace,
  `--check` mode for CI), `eidos new <name>` (scaffold — see Phase 6 + now includes Cargo
  workspace wrapper), `eidos test` (batch-check a directory, `--verbose`/`--json`),
  `eidos build --run` (auto-invokes `cargo build` on the emitted crate), and
  `--emit=ast|core` debug-dump flag on `eidos check`.
- [x] No `--json` output mode on `eidos check` for machine/editor consumption: `--json` is
  now supported on `eidos check`, `eidos build`, and `eidos test`. Batch/glob mode is
  served by `eidos test <dir>`.
- [x] `eidos check` only reports an aggregate count: `--verbose`/`--explain` mode now shows
  the `Verified`/`Trusted`/`Unverified` status of every individual obligation.
- [x] Counterexample reporting is inconsistent: `check_division` attaches a counterexample
  model to its error; the general `discharge` path (used for `ensures`/refinement
  obligations) doesn't for non-linear obligations, even though `eidos_verifier::counterexample`
  is already available on the linear path. **Fixed**: non-linear discharge path now calls
  `tpt_eidos_verifier::find_model(ctx)` and appends `context witness: {:?}` or notes
  the context is unsatisfiable (vacuously true) to every non-linear failure message.
- [x] No provenance header/fingerprint in generated Rust: generated `lib.rs` now includes
  `// Source: <path>` and `// Eidos version: <version>` in the file header.
- [x] No LSP/editor integration yet (naturally deferred, but worth naming alongside Phase 4,
  mirroring what `tpt-telos` eventually had). **Implemented**: `tpt-eidos-lsp` crate provides
  a minimal JSON-RPC 2.0 LSP server over stdio (`initialize`/`textDocument/didOpen|didChange|
  didClose`/`shutdown`/`exit`); wired as `eidos lsp` CLI subcommand.

## Phase 8: Adoption & Developer Experience (directional, not scheduled)
- [x] **Web playground**: compile `eidos-kernel`/`eidos-verifier` to WASM and ship a
  browser-based "try eidos" page (paste a `.eidos` snippet, see verify/reject +
  generated Rust) — single highest-leverage adoption lever for a language with no
  install-and-try path today. **Implemented as `eidos serve [--port P]`**: a pure-`std`
  HTTP server (no WASM, no external crates) serves an interactive playground page at
  `http://localhost:7070/` and accepts POST `/check` requests for on-the-fly verification.
- [x] **Editor support / syntax highlighting**: a tree-sitter or TextMate grammar for
  `.eidos` files (VS Code extension at minimum) — currently zero editor support beyond
  plain text, which is a major friction point before LSP (already tracked above) is
  feasible. **Implemented**: `editors/vscode/` contains a full VS Code extension with
  TextMate grammar (`syntaxes/eidos.tmLanguage.json`), language configuration, and README.
- [x] **`README.md` elevator pitch + comparison table**: "Why eidos?" section added with
  a table contrasting eidos with Dafny/F*/Liquid Haskell on TCB-purity, offline CI,
  no external SMT, and zero-cost `no_std` Rust emission.
- [x] **`eidos new` project scaffold, not just file scaffold**: `eidos new <name>` now
  emits a `Cargo.toml` workspace wrapper and pre-creates `verified/<name>/` so
  `eidos build` + `cargo build` work immediately without extra setup.
- [x] **Doc comments carried into generated Rust**: `///`-prefixed lines in `.eidos`
  source are parsed, threaded through erasure into `CoreFun.doc`, and emitted as
  `/// …` doc comments above the corresponding `pub fn` in the generated `lib.rs`.
- [x] **`CHANGELOG.md`**: added; tracks user-visible changes for v0.1.0, v0.2.0, and
  the current unreleased set.
- [x] **CI/coverage badges in `README.md`**: CI badge and a ≥75% coverage badge added
  at the top of `README.md`.
- [x] **More domain-library crates**: beyond `eidos-flight-math`/`eidos-controls-math`,
  a third domain (e.g. medical dosing bounds, robotics kinematics) would demonstrate
  the `Lemma`/`TrustedLemmas` pattern generalizes beyond aerospace, strengthening the
  "proof-native systems language" pitch rather than "flight-control DSL." **Implemented**:
  `tpt-eidos-medical` crate with `SafeDose`/`DoseRate` refinement types and `clamp_dose`,
  `apply_rate`, `safe_split` primitives; example at `examples/medication_dose.eidos`.
- [x] **GitHub issue templates / "good first issue" labeling**: `bug_report` and
  `domain_lemma` issue templates added in `.github/ISSUE_TEMPLATE/`.
