# scribecraft-automation (L4)

Every user-facing feature as a JSON tool (`TOOLS`), used by `scribecraft-cli run --script`
and, later, the opt-in MCP server.

- Paths are confined to `--root` (no `..`, no absolute paths outside it, symlinks resolved);
  results and errors show root-relative paths only.
- Errors name the tool, the step and what is allowed.

## Adding a tool
1. Engine API first. 2. A row in `src/tools.rs`. 3. An end-to-end test in
`tests/automation.rs`. 4. Cite tool and test in `parity/express-scribe-features.toml`.

Script format: `{"steps": [{"tool": "dictation_open", "args": {"id": 1}}, ...]}`.
