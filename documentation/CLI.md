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
lemma run [<spec>] [--rules=rule1,rule2] [data...] [options]
```

**Syntax:**
- `spec` -- evaluate all rules
- `spec --rules=rule` -- evaluate one rule
- `spec --rules=rule1,rule2` -- evaluate specific rules (comma-separated)
- No arguments with `-i` -- interactive mode

**Options:**
- `-d, --dir <path>` -- workspace root (default: `.`)
- `--rules <rules>` -- comma-separated rule names (omit to evaluate all)
- `-o, --output <format>` -- `table` (default) or `json`
- `-x, --explain` -- show data and reasoning
- `-i, --interactive` -- guided spec/rule/data selection
- `--effective <datetime>` -- evaluate at effective datetime (e.g. `2025`, `2025-03`, `2025-03-04`)

**Examples:**

```bash
lemma run pricing
lemma run pricing --rules=total,tax
lemma run nl/tax/net_salary --rules=net_salary -x
lemma run pricing quantity=10 is_vip=true
lemma run pricing -o json
lemma run pricing -x
lemma run pricing --effective 2025-01-01
lemma run -i
```

### `lemma schema` -- spec schema (data and rules)

Shows data and rules.

```bash
lemma schema <spec> [-d <path>] [--effective <datetime>]
```

### `lemma list` -- list all specs

```bash
lemma list [path] [--effective <datetime>]
```

### `lemma get` -- fetch registry dependencies

Resolves `@...` references and downloads specs from the registry.

```bash
lemma get [-d <path>] [-f]              # resolve all @... references in workspace
lemma get <spec> [-f]                   # fetch a specific spec (e.g. @lemma/std/finance)
```

**Options:**
- `-f, --force` -- overwrite existing specs when content has changed on the registry

### `lemma format` -- format .lemma files

```bash
lemma format [paths...] [--check] [--stdout]
```

**Options:**
- `--check` -- check formatting without modifying (exit 1 if any file would change)
- `--stdout` -- write formatted output to stdout

### `lemma info` -- show environment

```bash
lemma info
```

Shows version and deps cache path.

### `lemma server` -- start HTTP server

```bash
lemma server [-d <path>] [--host <host>] [-p <port>] [--watch] [--explanations]
```

**Options:**
- `-d, --dir <path>` -- workspace root (default: `.`)
- `--host <host>` -- bind address (default: `127.0.0.1`)
- `-p, --port <port>` -- port (default: `8012`)
- `--watch` -- live-reload on `.lemma` file changes
- `--explanations` -- enable explanation generation (clients send `x-explanations` header)

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
lemma server --dir ./policies --watch

curl "http://localhost:8012/pricing?quantity=10&is_member=true"

curl -X POST http://localhost:8012/pricing \
  -H "Content-Type: application/json" \
  -d '{"quantity": 10, "is_member": true}'
```

### `lemma mcp` -- start MCP server

AI assistant integration via [Model Context Protocol](https://modelcontextprotocol.io) over stdio.

```bash
lemma mcp [path] [--admin]
```

**Options:**
- optional `path` — workspace directory or `.lemma` file; omit to start without loading from disk
- `--admin` -- enable write tools (`add_spec`, `get_spec_source`)

## Workspace

A workspace is a directory containing `.lemma` files. All commands that accept `-d` / `--dir` load every `.lemma` file recursively from that directory, plus any registry deps from the global cache.

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
