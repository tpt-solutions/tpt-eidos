# CLAUDE.md

This file mirrors AGENTS.md for coding agents that read CLAUDE.md. See AGENTS.md
for the authoritative workspace documentation.

tpt-eidos: proof-native, dependently-typed systems language. Phase 1 MVK =
eidos-parser (AST) → eidos-kernel (refinement subtyping + division safety +
termination) → eidos-verifier (QF_LRA Fourier-Motzkin). Binary `eidos` in
eidos-cli. Integration tests in eidos-tests over examples/.

Conventions: pure std, no external crates. Keep `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, and
`cargo test --workspace` clean before finishing.
