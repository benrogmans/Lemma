---
nav_title: Installation
nav_order: 20
---

# Installation

Lemma is designed to run wherever your rules need to live. Install the CLI to evaluate specs from the terminal, embed the engine directly in Rust, JavaScript/TypeScript, Elixir, or Java/Kotlin, serve specs over HTTP or MCP, or pull a Docker image for containerized workloads. For language bindings and the full SDK documentation, see [Tools & SDKs](tools/readme.md).

## Lemma CLI

From [crates.io/crates/lemma](https://crates.io/crates/lemma):

```bash
cargo install lemma
```

Or via npm:

```bash
npm install -g lemma
```

See [Lemma CLI](reference/cli.md) for all commands.

## Embed Lemma in your application

Lemma can be embedded directly in Rust, JavaScript/TypeScript, Elixir, and Java/Kotlin. See [Tools & SDKs](tools/readme.md) for the full SDK documentation for each language. More languages are coming soon.

## Editor support

Install the CLI, then use the VS Code/Cursor extension or point any LSP client at `lemma lsp` (stdio). The extension invokes `lemma lsp` automatically.

## Docker

```bash
docker pull ghcr.io/lemma/lemma:latest
```

Supports `linux/amd64` and `linux/arm64`.
