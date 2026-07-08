---
nav_title: Lemma CLI
nav_order: 20
---

# Lemma CLI

See [Installation](../installation.md) for install options.

## Commands

### `lemma run`: evaluate a spec

```bash
lemma run [[repo] spec] [name=value ...] [--prefix PATH] [--rules=RULES] [options]
```

**Syntax:**
- Positionals: optional repository qualifier (e.g. `@iso/countries`), then spec name (see `lemma run --help`)
- `spec --rules=rule`: evaluate one rule
- `spec --rules=rule1,rule2`: evaluate specific rules (comma-separated)
- No arguments with `-i`: interactive mode

**Options:**
- `--prefix <path>`: workspace directory or `.lemma` file (default: current directory)
- `--rules <rules>`: comma-separated rule names (omit to evaluate all)
- `--json`: output results as JSON (default: human-readable table)
- `-x, --explain`: show data and reasoning
- `-i, --interactive`: guided spec/rule/data selection
- `--effective <datetime>`: evaluate at effective datetime (e.g. `2025`, `2025-03`, `2025-03-04`)

**Examples:**

```bash
lemma run pricing
lemma run pricing --rules=total,tax
lemma run --prefix ./policies nl/tax/net_salary --rules=net_salary -x
lemma run pricing quantity=10 is_vip=true
lemma run pricing --json
lemma run pricing -x
lemma run pricing --effective 2025-01-01
lemma run -i
lemma run '@iso/countries' alpha2
```

### `lemma schema`: spec schema (data types, constraints, and rules)

Shows data inputs with types and constraints (minimum, maximum, units, decimals, text options), prefilled values, defaults, supplied caller values (when overlay is applied), and rule result types.

```bash
lemma schema [[repo] spec] [--prefix PATH] [--effective <datetime>] [--json]
```

**Options:**
- `[repo]`: optional repository qualifier (e.g. `@iso/countries`)
- `[spec]`: spec name (omit when workspace has a single spec)
- `--prefix <path>`: workspace directory or `.lemma` file (default: current directory)
- `--effective <datetime>`: effective datetime for temporal specs
- `--json`: output schema as JSON (default: human-readable table)

**Examples:**

```bash
lemma schema pricing
lemma schema --prefix ./policies net_salary
lemma schema --prefix tax.lemma calculator
lemma schema '@iso/countries' alpha2
lemma schema pricing --json
```

### `lemma list`: list loaded specs by repository

Lists every loaded spec, grouped by repository. Local specs (no repository qualifier) are printed unindented; named repositories (including embedded `lemma`) appear as headers with indented spec names.

```bash
lemma list [--prefix PATH] [--json]
```

**Options:**
- `--prefix <path>`: workspace directory or `.lemma` file (default: current directory)
- `--json`: output listing as JSON array of `{ "repository", "specs" }` (default: human-readable text)

**Examples:**

```bash
lemma list
lemma list --prefix ./project
lemma list --json
```

### `lemma fetch`: fetch registry dependencies

Resolves `@...` references and downloads specs from the registry.

```bash
lemma fetch [--prefix PATH] --all
lemma fetch [--prefix PATH] <dependency> -f
```

**Options:**
- `--prefix <path>`: workspace directory or `.lemma` file (default: current directory)
- `-a, --all`: fetch all @... references in the workspace
- `-f, --force`: overwrite existing specs when content has changed on the registry

### `lemma format`: format .lemma files

```bash
lemma format [paths...] [--check] [--stdout]
```

**Options:**
- `--check`: check formatting without modifying (exit 1 if any file would change)
- `--stdout`: write formatted output to stdout

### `lemma server`: start HTTP server

```bash
lemma server [--prefix PATH] [--host <host>] [-p <port>] [--watch] [--explanations]
```

**Options:**
- `--prefix <path>`: workspace directory or `.lemma` file (default: current directory)
- `--host <host>`: bind address (default: `127.0.0.1`)
- `-p, --port <port>`: port (default: `8012`)
- `--watch`: live-reload on `.lemma` file changes
- `--explanations`: enable explanation generation (clients send `x-explanations` header)

**Routes:**

| Method | Route | Description |
|--------|-------|-------------|
| GET | `/{spec}?data=value` | Evaluate all rules (data as query params) |
| POST | `/{spec}` | Evaluate all rules (data as JSON body) |
| GET/POST | `/{spec}/{rules}` | Evaluate specific rules (comma-separated) |
| GET | `/` | List all specs with schemas |
| GET | `/openapi.json` | OpenAPI 3.1 specification |
| GET | `/docs` | Interactive API documentation (Scalar) |
| GET | `/health` | Health check |

**Example:**

```bash
lemma server --prefix ./policies --watch

curl "http://localhost:8012/pricing?quantity=10&is_member=true"

curl -X POST http://localhost:8012/pricing \
  -H "Content-Type: application/json" \
  -d '{"quantity": 10, "is_member": true}'
```

### `lemma lsp`: start language server

Starts the Language Server Protocol server over stdio for editor integration (diagnostics, formatting, semantic tokens). The VS Code/Cursor extension invokes this automatically; a globally installed `lemma` CLI is the only requirement.

```bash
lemma lsp
```

### `lemma mcp`: start MCP server

AI assistant integration via [Model Context Protocol](https://modelcontextprotocol.io) over stdio.

```bash
lemma mcp [--prefix PATH] [--admin]
```

**Options:**
- `--prefix <path>`: workspace directory or `.lemma` file; omit to start without loading from disk
- `--admin`: enable admin tools: `add_spec`, `get_spec_source` (read-only by default)

## Workspace

A workspace is a directory containing `.lemma` files. Commands that load specs use `--prefix` to select the workspace (default: current directory). Every `.lemma` file is loaded recursively from that directory, plus any registry deps from the global cache.

```
policies/
  pricing.lemma
  shipping.lemma
  tax.lemma
```

## Resource Limits

Resource limits control parse-time and planning-time budgets. These are security boundaries that prevent unbounded resource consumption from untrusted input.

| Limit | Default | Purpose |
|-------|---------|---------|
| `max_sources` | 4096 | Maximum source files in one engine |
| `max_loaded_bytes` | 50 MB | Total source text across all files |
| `max_source_size_bytes` | 10 MB | Single source file size |
| `max_expression_depth` | 256 | AST nesting depth |
| `max_expression_count` | 10,000 | Expressions per rule |
| `max_total_expression_count` | 1,000,000 | Expressions across all rules |
| `max_normalized_expression_nodes` | 30,000 | Post-normalization node count per rule |
| `max_data_value_bytes` | 1 KB | Single data value size |
| `max_spec_dependency_depth` | 32 | `uses` chain depth |
| `max_dag_specs` | 4096 | Total specs in dependency DAG |

`max_expression_depth` and `max_spec_dependency_depth` bound recursion during parsing and planning. Raising these beyond the defaults requires stack analysis for your deployment.

## API Defaults

**Effective datetime**: when no `--effective` flag or `Accept-Datetime` header is provided, the engine uses the current instant to select the temporal version of the root spec.

**Explanations**: disabled by default in CLI (`lemma run`) and HTTP; enabled by default in WASM. Use `--explain` (CLI) or `--explanations` (server) to opt in.

## See Also

- [Learn guide](../learn/readme.md)
- [Installation](../installation.md)
- [Language reference](readme.md)
- [LemmaBase](registry.md)
- [CLI benchmarks](benchmarks/cli.md)
- [Engine benchmarks](benchmarks/engine.md)
