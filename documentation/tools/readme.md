---
nav_title: Tools & SDKs
nav_order: 50
---

# Tools & SDKs

Ways to run Lemma, from embedding the engine in your app to driving it from the command line.

## Lemma SDKs

Embed Lemma directly in your language:

- [Rust](rust.md)
- [Elixir](elixir.md)
- [JavaScript / TypeScript](javascript.md)
- [Maven](maven.md) (coming soon)
- [Python](python.md) (coming soon)
- [C# / .NET](dotnet.md) (coming soon)

More SDKs are on the way. Precompiled binaries make each new one straightforward to add.

## Command line & servers

- [Lemma CLI](../reference/cli.md): `lemma run`, `format`, `fetch`, plus the HTTP and language servers
- [Lemma MCP](../reference/cli.md#lemma-mcp-start-mcp-server): expose specs to AI assistants over the Model Context Protocol
- [Registry: LemmaBase](../reference/registry.md): share and reuse specs via `@org/path` references

## Notes that apply to every SDK

- Same engine underneath, so results are identical across languages.
- `load` validates: invalid specs are rejected there, never at run time.
- The engine never hits the network. Resolve `@...` [registry](../reference/registry.md) references before loading.
- Editor support (any language): the [VS Code / Cursor extension](../installation.md) drives `lemma lsp`.
