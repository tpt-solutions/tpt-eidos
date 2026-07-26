# Cookbook

Practical recipes for common eidos patterns.

## Clamping a value

Keep output within bounds:

```
type Clamped = { v: f64 | v >= -1.0 && v <= 1.0 };

fn clamp(x: f64) -> Clamped {
    if x > 1.0 {
        return { v: 1.0 } as Clamped;
    } else {
        if x < -1.0 {
            return { v: -1.0 } as Clamped;
        } else {
            return { v: x } as Clamped;
        }
    }
}
```

Note: eidos requires `{ }` around both `if` and `else` bodies. There is no
`else if` — use nested `else { if ... }`.

## Safe division with guard

The standard pattern for division safety:

```
fn safe_div(a: f64, b: f64) -> f64 {
    if b > 0.0 {
        return a / b;
    } else {
        if b < 0.0 {
            return a / b;  // OK: b < 0.0, so b != 0
        } else {
            return 0.0;    // b == 0, can't divide
        }
    }
}
```

The kernel tracks path constraints: inside `if b > 0.0`, it knows `b > 0.0`,
which implies `b != 0`.

## Array normalization

Normalize a vector to unit length:

```
type UnitVec3 = { v: Array<f64, 3> | v.magnitude() <= 1.0 };

fn normalize(raw: Array<f64, 3>) -> UnitVec3
    requires raw.len() == 3
{
    let mag = raw.magnitude();
    if mag > 0.0 {
        return { v: raw.map(|x| x / mag) } as UnitVec3;
    } else {
        return { v: [0.0, 0.0, 0.0] } as UnitVec3;
    }
}
```

The `normalized_vector` trusted lemma discharges the
`v.magnitude() <= 1.0` obligation when the array is formed by dividing each
element by its magnitude.

## Using `let` bindings

`let` bindings enter the proof context:

```
fn let_example(a: f64) -> f64 {
    let threshold = 0.0;
    if a > threshold {
        return a / threshold;  // REJECTED: threshold == 0.0
    } else {
        return 0.0;
    }
}
```

But manifest nonzero literals work:

```
fn let_nonzero(a: f64) -> f64 {
    let scale = 5.0;
    return a / scale;  // OK: scale == 5.0 != 0
}
```

## Recursive functions

Structurally decreasing recursion:

```
fn factorial(n: f64) -> f64 {
    if n > 1.0 {
        return n * factorial(n - 1.0);  // OK: n - 1.0 < n
    } else {
        return 1.0;
    }
}
```

The kernel verifies that each recursive call's argument is strictly less
than the corresponding parameter. Non-decreasing calls are rejected.

## Using the domain library

Import flight-control primitives:

```
// The domain library ships pre-verified eidos source.
// Parse and verify it with the flight-math lemma set:
//
//   use tpt_eidos_flight_math::{check_source, PRIMITIVES_EIDOS};
//   let report = check_source(PRIMITIVES_EIDOS).unwrap();
//
// The primitives include:
//   - safe_direction(raw) -> UnitVec3
//   - quat_normalize(q) -> UnitQuat
//   - pid_linear(err, integ, deriv, kp, ki, kd) -> Vec3
```

See `crates/tpt-eidos-flight-math/src/primitives.eidos` for the full source.

## Debugging with `--verbose`

Use `--verbose` to see per-obligation details:

```
eidos check examples/calibrate_gyro.eidos --verbose
```

Output:

```
eidos: examples/calibrate_gyro.eidos: verified
  [Verified] division by zero: mag != 0
  [Verified] refinement NormalizedVector3: (v.magnitude() <= 1.0)
  [Trusted]  ensures: (result.v.magnitude() <= 1.0) (trusted lemma: normalized_vector)
  ...
```

Tags:
- `[Verified]` — proven by the QF_LRA linear prover
- `[Trusted]` — accepted via a named trusted lemma
- `[Unverified]` — could not be proven (error)

## Writing a custom type alias

Refinement types can have complex predicates:

```
type BoundedPositive = { x: f64 | x > 0.0 && x <= 100.0 };
type NonEmptyArray = { a: Array<f64, 3> | a.len() == 3 };
```

## Conditional branching patterns

Nested conditions for multiple cases:

```
fn classify(x: f64) -> f64 {
    if x > 0.0 {
        return 1.0;
    } else {
        if x < 0.0 {
            return -1.0;
        } else {
            return 0.0;
        }
    }
}
```

## Building a verified crate

```
eidos build examples/calibrate_gyro.eidos --out-dir out/calibrate
```

This produces:
- `out/calibrate/lib.rs` — `#![no_std]` Rust with all proof terms erased
- `out/calibrate/Cargo.toml` — ready to `cargo build --release`

The generated code has zero runtime overhead from verification.
