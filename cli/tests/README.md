# CLI integration tests

Black-box tests for `lemma`. Entry: [integration.rs](integration.rs) → `integrations/`.

Run:

```bash
cargo nextest run -p lemma --tests
```

Run LSP tests only:

```bash
cargo nextest run -p lemma --test integration integrations::lsp
```

## Modules

| File | Focus | Mechanism |
|------|--------|-----------|
| [integrations/run.rs](integrations/run.rs) | `lemma run`, formatter flags, temp specs | `assert_cmd` + `tempfile` |
| [integrations/mcp.rs](integrations/mcp.rs) | MCP tools (`list_specs`, `run`, `add_spec`, …) | JSON-RPC over stdio |
| [integrations/lsp.rs](integrations/lsp.rs) | `lemma lsp` over stdio (initialize, diagnostics, formatting, semantic tokens) | Content-Length framed JSON-RPC via [lsp_session.rs](integrations/lsp_session.rs) |
| [integrations/server.rs](integrations/server.rs) | HTTP evaluate/list endpoints | `reqwest` against local server |
| [integrations/examples.rs](integrations/examples.rs) | Fixture `.lemma` under `integrations/examples/` | Same as run; golden paths |

Unit tests in `cli/src/formatter.rs` and `cli/src/mcp/server.rs` cover private formatting/MCP helpers.

## Overlap with engine tests

| CLI | Engine |
|-----|--------|
| `integrations/examples/*.lemma` | [engine/tests/integration_examples.rs](../../engine/tests/integration_examples.rs) loads the same files via `Engine` |
| MCP `list_specs` / `run` | [engine/tests/](../../engine/tests/) exercise the same engine APIs in-process |

CLI tests assert process boundaries (binary exit codes, JSON shapes, HTTP). Engine tests assert semantics. Change engine behavior in both places when user-visible output changes.

## Ignored / bench

Criterion benches: `cli/benches/http_evaluate.rs`, `engine_profile.rs`. Regenerate numbers with `cargo benchmarks cli` (writes `documentation/reference/benchmarks/cli.md`). CI also runs them via `cargo nextest run --run-ignored all`.
