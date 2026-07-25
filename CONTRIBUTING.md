# Contributing to tpt-eidos

Contributions go through GitHub Issues only.

## How to contribute

1. **Open an issue** describing the bug, feature, or design question.
2. Discuss the approach with maintainers before writing code.
3. Fork the repo and submit a pull request referencing the issue.

## Development setup

```sh
git clone https://github.com/tpt-solutions/tpt-eidos
cd tpt-eidos
cargo test --workspace
```

All external crate dependencies are forbidden — the workspace is pure `std`
so the trusted computing base stays auditable and CI stays offline.

## Code quality

Before submitting a PR, ensure:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

All three must pass with zero warnings.

## Architecture

See `AGENTS.md` for the workspace layout and pipeline. The key trust boundary
is the kernel (`tpt-eidos-kernel`): every change that affects proof-obligation
discharge or refinement subtyping needs careful review.

## License

By contributing, you agree that your contributions will be licensed under
Apache-2.0 OR MIT, consistent with the project's dual license.
