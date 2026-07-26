# Language Tour

A 10-minute walkthrough of the eidos language. By the end, you'll understand
the core concepts and be able to write your own verified functions.

## 1. Refinement types

Refinement types let you attach predicates to base types. A value of type
`{ x: f64 | x > 0.0 }` is a `f64` that is guaranteed to be positive — the
compiler verifies this at compile time.

```
type Positive = { x: f64 | x > 0.0 };
```

The kernel checks that every value introduced as `Positive` satisfies `x > 0.0`.
If it can't prove this, the program is rejected.

## 2. Division safety

The most fundamental guarantee: division by zero is impossible. The kernel
requires a proof that the denominator is non-zero before allowing division.

```
fn safe_div(a: f64, b: f64) -> f64
    requires b > 0.0
{
    return a / b;
}
```

The `requires b > 0.0` clause tells the kernel that `b` is positive. The
kernel then proves `b != 0` from this, and accepts the division.

Without the guard, the kernel rejects:

```
fn unsafe_div(a: f64, b: f64) -> f64 {
    return a / b;  // REJECTED: no proof that b != 0
}
```

## 3. `if/else` branching

Branches propagate path constraints. Inside `if b > 0.0`, the kernel knows
`b > 0.0` is true. Inside `else`, it knows `b <= 0.0`.

```
fn guarded_div(a: f64, b: f64) -> f64 {
    if b > 0.0 {
        return a / b;  // OK: b > 0.0 in this branch
    } else {
        return 0.0;
    }
}
```

This is how `calibrate_gyro` works: the `if mag > 0.0` guard proves
the division is safe in the true branch.

## 4. `requires` and `ensures`

Preconditions and postconditions constrain function behavior:

```
fn clamp(x: f64) -> f64
    ensures |result| result >= -1.0 && result <= 1.0
{
    if x > 1.0 {
        return 1.0;
    } else {
        if x < -1.0 {
            return -1.0;
        } else {
            return x;
        }
    }
}
```

The `ensures` clause is a lambda: `|result|` is the return value. The kernel
verifies that every `return` path satisfies the postcondition.

## 5. Type aliases

Give names to complex refinement types for readability:

```
type NormalizedVector3 = { v: Array<f64, 3> | v.magnitude() <= 1.0 };
type PositiveFloat = { x: f64 | x > 0.0 };
```

## 6. The `as` cast

To return a refinement type, you must wrap the value with `as`:

```
fn make_positive(x: f64) -> PositiveFloat
    requires x > 0.0
{
    return { x: x } as PositiveFloat;
}
```

The `as` cast triggers a proof obligation: the kernel verifies that the
value satisfies the refinement predicate.

## 7. `let` bindings

`let` bindings introduce values into the proof context. A `let`-bound
manifest nonzero literal proves the value is non-zero:

```
fn let_example(a: f64) -> f64 {
    let x = 5.0;
    return a / x;  // OK: x == 5.0, so x != 0
}
```

## 8. Array operations

Eidos supports array literals, `.len()`, `.map()`, `.zip()`, and
`.magnitude()`:

```
type UnitVec = { v: Array<f64, 3> | v.magnitude() <= 1.0 };

fn normalize(raw: Array<f64, 3>, mag: f64) -> UnitVec
    requires mag > 0.0
{
    if mag > 0.0 {
        return { v: raw.map(|x| x / mag) } as UnitVec;
    } else {
        return { v: [0.0, 0.0, 0.0] } as UnitVec;
    }
}
```

`.magnitude()` is the Euclidean norm. The kernel admits
`v.magnitude() <= 1.0` via the `normalized_vector` trusted lemma when the
array is formed by dividing each element by its magnitude.

## 9. Effects

Effects are metadata labels on functions:

```
fn pure_add(a: f64, b: f64) -> f64 effects [Pure] {
    return a + b;
}
```

In the MVK, effects are descriptive only — they don't trigger runtime checks.
The `effects [RealTime<2ms>]` annotation enables WCET budget checking (the
kernel rejects functions whose structural cost exceeds the budget).

## 10. Recursive functions

Recursive functions must have a structurally decreasing argument:

```
fn countdown(n: f64) -> f64 {
    if n > 0.0 {
        return countdown(n - 1.0);  // OK: n - 1.0 < n
    } else {
        return 0.0;
    }
}
```

The kernel verifies that `n - 1.0` is strictly less than `n`. Non-decreasing
recursion is rejected.

## 11. Linear types

Mark a parameter as `linear` to require exactly one use on every code path:

```
fn consume(x: linear f64) -> f64 {
    return x;  // OK: used exactly once
}
```

Linear parameters cannot be:
- Unused (rejected: "never used")
- Used more than once (rejected: "used more than once")
- Captured in a lambda (rejected: "captured inside a lambda")

## Next steps

- See `examples/calibrate_gyro.eidos` for a complete worked example
- Read `docs/cookbook.md` for practical patterns
- Check `crates/tpt-eidos-flight-math/` for a real domain library
