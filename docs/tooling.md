# Editor Support and Tooling

## VS Code Extension

The `editors/vscode/` directory contains a VS Code extension with:

- **Syntax highlighting** -- TextMate grammar for `.axis` files
- **LSP integration** -- completions, go-to-definition, references, hover, rename, diagnostics
- **Workspace symbol search** -- find constructs by name
- **Watch file events** -- automatic revalidation on file changes

### Installation

```bash
cd editors/vscode
npm install
npx tsc -p .
```

Then either:
- Press F5 in VS Code to "Run Extension" (development mode)
- Package with `npx vsce package` and install the `.vsix` file

### Configuration

The extension expects the `axis` binary on your PATH. Build it first:

```bash
cargo build --release
export PATH="$PWD/target/release:$PATH"
```

## Language Server (LSP)

The language server runs over stdio and supports:

### Features

| Feature | Description |
|---------|-------------|
| **Completions** | Context-aware keyword and identifier suggestions |
| **Go-to-definition** | Jump to shape, source, realm, flow, or service definitions |
| **Find references** | Find all uses of a shape, source, or flow |
| **Hover** | Type information and documentation on hover |
| **Rename** | Rename symbols across the project |
| **Diagnostics** | Real-time parse and verification errors |
| **Workspace symbols** | Search constructs by name |

### Starting Manually

```bash
axis --lsp
```

The LSP communicates over stdin/stdout using the LSP protocol.

## Formatter

Format `.axis` source code with consistent style:

```bash
axis --fmt app.axis
```

The formatter:
- Normalizes indentation to 2 spaces
- Aligns field types and modifiers
- Preserves comments
- Outputs the formatted program to stdout

Pipe back to file:

```bash
axis --fmt app.axis > app.formatted.axis
mv app.formatted.axis app.axis
```

## Incremental Parser

The incremental parser (`--completions`) provides:

- Parser state at the cursor position
- Set of valid next tokens
- Constructs parsed so far
- Parse errors with line numbers

```bash
axis --completions partial.axis
```

This is used by the LSP for real-time editing support and by constrained decoders for LLM guidance.

## Structural Diff

Compare two axis files structurally:

```bash
axis --diff old.axis new.axis
```

Reports:
- Added, removed, and modified constructs
- Field-level changes within shapes
- Route changes in flows
- Migration suggestions for shape changes

## Constrained Decoding

The LL(1) grammar enables constrained decoding for LLM code generation.

### Grammar Export

```bash
axis --constrain
```

Exports the grammar state machine as JSON. Each state lists valid transitions and the token types accepted.

### Logit Masks

```bash
axis --logit-masks
```

Exports:
- `vocabulary` -- the token vocabulary with names
- `masks` -- per-state binary masks over the vocabulary

At each token position during generation, mask the LLM's logits to only allow valid tokens. This guarantees syntactically valid output and eliminates retry for syntax errors.

## Watch Mode

Recompile automatically when files change:

```bash
axis --watch my-api/
```

The watcher:
- Monitors all `.axis` files in the directory
- Debounces rapid changes
- Recompiles and re-runs verification
- Regenerates codegen output on successful compilation
- Reports errors inline

## Multi-File Projects

Split large programs across multiple files:

```
my-api/
  src/
    models.axis       # SHAPE definitions
    sources.axis      # SOURCE, REALM definitions
    auth.axis         # AUTH-related flows
    bookings.axis     # Booking flows
    admin.axis        # Admin flows
    policies.axis     # POLICY definitions
```

Compile the entire project:

```bash
axis --project my-api/
```

The compiler:
1. Discovers all `.axis` files in the source directory (`<dir>/src/` if it exists, otherwise `<dir>/`)
2. Parses each file independently
3. Merges all ASTs into a single program
4. Runs linking and verification across the entire program
5. Reports errors with file paths
