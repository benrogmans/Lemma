---
layout: default
title: CLI Guide
---

# Lemma CLI

## Installation

```bash
cargo install lemma-cli
```

## Commands

### `lemma run` -- evaluate a spec

```bash
lemma run [[repo] spec] [name=value ...] [--prefix PATH] [--rules=RULES] [options]
```

**Syntax:**
- Positionals: optional repository qualifier (e.g. `@lemma/std`), then spec name (see `lemma run --help`)
- `spec --rules=rule` -- evaluate one rule
- `spec --rules=rule1,rule2` -- evaluate specific rules (comma-separated)
- No arguments with `-i` -- interactive mode

**Options:**
- `--prefix <path>` -- workspace directory or `.lemma` file (default: current directory)
- `--rules <rules>` -- comma-separated rule names (omit to evaluate all)
- `--as <rule:unit>` -- convert a **named quantity** rule’s displayed result to another unit declared on that rule’s type (repeatable; e.g. `--as total:usd`). Dependent rules still use the unconverted value; `--explain` still shows the evaluation trace (not a synthetic `rule as unit` root). With `--rules`, every `--as` rule must appear in the rule list.
- `-o, --output <format>` -- `table` (default) or `json`
- `-x, --explain` -- show data and reasoning
- `-i, --interactive` -- guided spec/rule/data selection
- `--effective <datetime>` -- evaluate at effective datetime (e.g. `2025`, `2025-03`, `2025-03-04`)

**Examples:**

```bash
lemma run pricing
lemma run pricing --rules=total,tax
lemma run --prefix ./policies nl/tax/net_salary --rules=net_salary -x
lemma run pricing quantity=10 is_vip=true
lemma run pricing -o json
lemma run pricing -x
lemma run pricing --effective 2025-01-01
lemma run pricing --as total:usd
lemma run -i
lemma run '@lemma/std' finance
```

### `lemma schema` -- spec schema (data and rules)

Shows data and rules.

```bash
lemma schema [source] [spec] [--effective <datetime>]
```

### `lemma list` -- list workspace specs and repositories

Lists specs in the **workspace (main) repository** only, then a **Repositories** section for loaded named/registry repos. Optional first positional is a workspace directory if it exists on disk; otherwise a single positional is a **repository** qualifier (e.g. `@lemma/std`). With an explicit workspace path, pass `[REPO]` as the second positional. (`lemma run` uses `--prefix` for the workspace instead.)

```bash
lemma list [source] [REPO] [--effective <datetime>]
```

Examples:

```bash
lemma list
lemma list '@lemma/std'
lemma list ./project spec_composition
lemma list lemma    # print formatted embedded SI stdlib (repo lemma, spec si)
```

### `lemma fetch` -- fetch registry dependencies

Resolves `@...` references and downloads specs from the registry.

```bash
lemma fetch [source] --all            # resolve all @... references
lemma fetch [source] <dependency> -f  # fetch a specific dependency (e.g. @lemma/std)
```

**Options:**
- `-a, --all` -- fetch all @... references in the workspace
- `-f, --force` -- overwrite existing specs when content has changed on the registry

### `lemma format` -- format .lemma files

```bash
lemma format [paths...] [--check] [--stdout]
```

**Options:**
- `--check` -- check formatting without modifying (exit 1 if any file would change)
- `--stdout` -- write formatted output to stdout

### `lemma server` -- start HTTP server

```bash
lemma server [source] [--host <host>] [-p <port>] [--watch] [--explanations]
```

**Options:**
- `[source]` -- workspace directory or `.lemma` file (default: `.`)
- `--host <host>` -- bind address (default: `127.0.0.1`)
- `-p, --port <port>` -- port (default: `8012`)
- `--watch` -- live-reload on `.lemma` file changes
- `--explanations` -- enable explanation generation (clients send `x-explanations` header)

**Routes:**

| Method | Route | Description |
|--------|-------|-------------|
| GET | `/{spec}?data=value` | Evaluate all rules (data as query params) |
| GET | `/{spec}?as_units=rule:unit,...` | Optional quantity rule-result unit conversion (comma-separated) |
| POST | `/{spec}` | Evaluate all rules (data as JSON body) |
| GET/POST | `/{spec}/{rules}` | Evaluate specific rules (comma-separated) |
| GET | `/` | List all specs with schemas |
| GET | `/openapi.json` | OpenAPI 3.1 specification |
| GET | `/docs` | Interactive API documentation (Scalar) |
| GET | `/health` | Health check |

**Example:**

```bash
lemma server [source] --watch

curl "http://localhost:8012/pricing?quantity=10&is_member=true"
curl "http://localhost:8012/pricing?as_units=total:usd"

curl -X POST http://localhost:8012/pricing \
  -H "Content-Type: application/json" \
  -d '{"quantity": 10, "is_member": true}'
```

### `lemma mcp` -- start MCP server

AI assistant integration via [Model Context Protocol](https://modelcontextprotocol.io) over stdio.

```bash
lemma mcp [source] [--admin]
```

**Options:**
- `[source]` — workspace directory or `.lemma` file; omit to start without loading from disk
- `--admin` — enable admin tools: `add_spec`, `get_spec_source` (read-only by default)

## Workspace

A workspace is a directory containing `.lemma` files. All commands that accept a `[source]` argument (or `--prefix`) load every `.lemma` file recursively from that directory, plus any registry deps from the global cache.

```
policies/
  pricing.lemma
  shipping.lemma
  tax.lemma
```

## See Also

- [Language Guide](index.md)
- [Reference](reference.md)
- [Registry](registry.md)
- [Examples](examples/)
