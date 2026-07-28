# eidos Language Support for VS Code

Syntax highlighting for `.eidos` source files (the tpt-eidos proof-native systems language).

## Features

- Syntax highlighting for keywords (`fn`, `type`, `requires`, `ensures`, `effects`, `let`, `if`, `else`, `return`, `as`, `linear`)
- Refinement-type primitives (`f64`, `f32`, `i64`, `i32`, `bool`, `Unit`, `Array`)
- Doc comments (`///`) rendered distinctly from regular line comments (`//`)
- Function names and type aliases highlighted at declaration sites
- Numeric literals, boolean constants, operators, and lambda syntax

## Install

### From the marketplace (future)

Search "eidos" in the VS Code Extensions panel.

### Manual install (VSIX)

```sh
cd editors/vscode
npm install -g @vscode/vsce   # one-time
vsce package                  # produces tpt-eidos-0.2.0.vsix
code --install-extension tpt-eidos-0.2.0.vsix
```

### Development (load from source)

Open `editors/vscode/` as the workspace root in VS Code and press **F5** to
launch the Extension Development Host.

## Example

```eidos
/// A normalized 3D direction vector: magnitude ≤ 1.0.
type NormalizedVector3 = { v: Array<f64, 3> | v.magnitude() <= 1.0 };

/// Calibrate and normalise a raw gyroscope reading.
fn calibrate(raw: Array<f64, 3>, bias: Array<f64, 3>) -> NormalizedVector3
requires raw.len() == 3 && bias.len() == 3
ensures |result| result.v.magnitude() <= 1.0
{
    let corrected = raw.zip(bias).map(|(r, b)| r - b);
    let mag = corrected.magnitude();
    if mag > 0.0 {
        return { v: corrected.map(|x| x / mag) } as NormalizedVector3;
    } else {
        return { v: [0.0, 0.0, 0.0] } as NormalizedVector3;
    }
}
```

## LSP integration

For full diagnostics (verify-on-save, error squiggles), run `eidos lsp` as the
language server. Configure your editor to launch it:

```json
{
    "eidos.languageServerPath": "/path/to/eidos"
}
```

A future extension version will wire this automatically.
