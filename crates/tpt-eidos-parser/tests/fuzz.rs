//! Dependency-free fuzz/property tests for the eidos parser.
//!
//! These address the open question in TODO.md (Phase 5): "arbitrary strings must
//! never panic/hang". They use a small deterministic PRNG so the runs are
//! reproducible in CI without pulling in `proptest`/`cargo-fuzz` (the project
//! convention is pure `std`, no external crates — even as dev-dependencies).
//!
//! The parser's `MAX_PARSE_DEPTH` guard bounds stack usage, so a deeply nested
//! or otherwise adversarial source string can neither stack-overflow nor hang.

use tpt_eidos_parser::parse;

/// Deterministic xorshift64 PRNG (no external crates).
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed ^ 0x9e3779b97f4a7c15)
    }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
    fn pick(&mut self, s: &[char]) -> char {
        s[self.below(s.len())]
    }
}

/// Characters that can plausibly appear in eidos source. Includes operators,
/// punctuation, digits, letters and whitespace; the fuzz driver assembles random
/// sequences from this alphabet and feeds them to the parser.
const ALPHABET: &[char] = &[
    'a', 'b', 'c', 'f', 'n', 'x', 'y', 'z', '0', '1', '2', '3', '9', '.', ' ', '\n', '\t', '(',
    ')', '{', '}', '[', ']', '<', '>', '=', '!', '+', '-', '*', '/', '%', '&', '|', '.', ',', ';',
    ':', 'r', 'e', 't', 'u', 'i', 'o', 'm', 'g', 'v', 'p', 'l',
];

#[test]
fn random_sources_never_panic_or_hang() {
    let mut rng = Rng::new(0xdead_beef);
    let mut buffer = String::new();
    for _ in 0..4000 {
        buffer.clear();
        let len = 1 + rng.below(120);
        for _ in 0..len {
            buffer.push(rng.pick(ALPHABET));
        }
        // The parser must return a `Result` for any input: it may be `Ok` or
        // `Err`, but it must never panic or loop forever.
        let _ = parse(&buffer);
    }
}

#[test]
fn adversarial_deep_recursion_is_bounded() {
    // Pathological paren nesting must error (depth guard), not stack-overflow.
    for depth in [100, 1000, 5000, 50000] {
        let src = format!("{}{}{}", "(".repeat(depth), "1.0", ")".repeat(depth));
        assert!(
            parse(&src).is_err(),
            "deeply nested source must be rejected, not overflow"
        );
    }
}

#[test]
fn adversarial_deep_unary_chains_are_bounded() {
    // Long chains of unary minus also exercise the recursion guard.
    let src = format!("{}-1.0", "-".repeat(5000));
    assert!(parse(&src).is_err());
}

#[test]
fn adversarial_unterminated_structures_are_bounded() {
    // Unterminated tokens / brackets must yield an `Err`, never a panic.
    for prefix in ["fn f(", "type T = { x: f64 |", "[", "(", "{", "1.0 +"] {
        assert!(parse(prefix).is_err());
    }
}
