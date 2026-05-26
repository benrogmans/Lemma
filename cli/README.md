# Lemma CLI

> **A command-line interface for the Lemma language.**

This package provides the `lemma` CLI for running, inspecting, and serving Lemma specs. It ships alongside the `lemma-engine` crate and exposes the same deterministic, auditable evaluation pipeline from the terminal.

## Status

Lemma is pre-1.0. The CLI is stable for most use cases, but breaking changes may occur between minor versions. Pin your dependency version and review the [changelog](https://github.com/lemma/lemma/blob/main/CHANGELOG.md) before upgrading.

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

# Provide data values
lemma run tax_calculation income=75000 filing_status="married"

# Explore specs interactively
lemma run --interactive

# Show spec structure
lemma schema pricing

# List workspace entry-point specs (+ loaded repos); from cwd, drill into a repo:
lemma list
lemma list '@lemma/std'
lemma list ./my_project spec_composition

# Start the HTTP server
lemma server ./documentation/examples --port 8012

# Start the MCP server (AI assistant integration)
lemma mcp ./documentation/examples
```

Each command supports `--help` for full usage details.

## Features

- **Deterministic evaluations** – same audit trail as the engine library
- **Interactive mode** – select specs, rules, and data without typing full paths
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

- CLI reference: <https://github.com/lemma/lemma/blob/main/documentation/CLI.md>
- Language guide: <https://github.com/lemma/lemma/blob/main/documentation/index.md>
- API docs (engine): <https://docs.rs/lemma-engine>
- Examples: <https://github.com/lemma/lemma/tree/main/documentation/examples>

## Contributing

Contributions are very welcome!

## License

Apache 2.0
