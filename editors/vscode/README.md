# Cool VS Code Extension

This extension gives VS Code first-party support for `.cool` files:

- `.cool` language registration
- syntax highlighting and indentation rules
- diagnostics, completions, hover, go-to-definition, document symbols, and workspace symbols through `cool lsp`

## Install From This Repo

1. Build the Cool binary:

   ```bash
   cargo build --release
   ```

2. Package the extension:

   ```bash
   cd editors/vscode
   npm install
   npx @vscode/vsce package
   ```

3. In VS Code, run `Extensions: Install from VSIX...` and pick the generated `.vsix` file.

## Configure The Language Server

If `cool` is already on your `PATH`, the default configuration works:

```json
{
  "cool.lsp.serverCommand": ["cool", "lsp"]
}
```

If you want to point VS Code at the repo-local binary instead:

```json
{
  "cool.lsp.serverCommand": [
    "/absolute/path/to/cool-lang/target/release/cool",
    "lsp"
  ]
}
```

## Development

From `editors/vscode/`:

```bash
npm install
code --extensionDevelopmentPath="$(pwd)"
```

Then open another VS Code window and work with `.cool` files there.
