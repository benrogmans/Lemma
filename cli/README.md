# Lemma CLI

> **A command-line interface for the Lemma language.**

This package provides the `lemma` CLI for running, inspecting, and serving Lemma specs. It ships alongside the `lemma-engine` crate and exposes the same deterministic, auditable evaluation pipeline from the terminal.

## Status

Lemma is still early-stage and **not yet recommended for production use**. Expect breaking changes and evolving commands while the toolchain stabilizes.

## Installation

```bash
cargo install lemma-cli
```

After installation the `lemma` binary is available on your PATH.

## Common commands

```bash
# Evaluate a spec (all rules)
lemma run shipping

# Evaluate specific rules
lemma run tax_calculation --rules=tax_owed

# Provide fact values
lemma run tax_calculation income=75000 filing_status="married"

# Explore specs interactively
lemma run --interactive

# Show spec structure
lemma show pricing

# List available specs
lemma list ./documentation/examples

# Start the HTTP server
lemma server --port 8012 --dir ./documentation/examples

# Start the MCP server (AI assistant integration)
lemma mcp --dir ./documentation/examples
```

Each command supports `--help` for full usage details.

## Features

- **Deterministic evaluations** – same audit trail as the engine library
- **Interactive mode** – select specs, rules, and facts without typing full paths
- **HTTP server** – evaluate specs over REST, perfect for integration tests and dashboards
- **MCP server** – expose Lemma to AI assistants via the Model Context Protocol
- **Machine-readable output** – `--raw` flag for tooling and pipelines

## Example session

```bash
lemma run shipping

# Output:
# ┌───────────────┬──────────────────────────────────────────────────────┐
# │ Rule          ┆ Evaluation                                           │
# ╞═══════════════╪══════════════════════════════════════════════════════╡
# │ express_fee   ┆ 4.99                                                 │
# │               ┆    ...                                               │
# └───────────────┴──────────────────────────────────────────────────────┘
```

Enable raw mode to pipe results:

```bash
lemma run shipping --raw > output.json
```

## Documentation

- CLI reference: <https://github.com/benrogmans/lemma/blob/main/documentation/CLI.md>
- Language guide: <https://benrogmans.github.io/lemma/>
- API docs (engine): <https://docs.rs/lemma-engine>
- Examples: <https://github.com/benrogmans/lemma/tree/main/documentation/examples>
- Roadmap: <https://github.com/benrogmans/lemma/blob/main/documentation/roadmap.md>

## Contributing

Contributions are very welcome! See [documentation/contributing.md](https://github.com/benrogmans/lemma/blob/main/documentation/contributing.md) and the [project roadmap](https://github.com/benrogmans/lemma/blob/main/documentation/roadmap.md) for ideas.

## License

Apache 2.0
