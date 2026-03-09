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
lemma run [<spec>] [--rules=rule1,rule2] [facts...] [options]
```

**Syntax:**
- `spec` -- evaluate all rules
- `spec --rules=rule` -- evaluate one rule
- `spec --rules=rule1,rule2` -- evaluate specific rules (comma-separated)
- `spec~hash` -- pin to content hash (like HTTP `?hash=`)
- No arguments with `-i` -- interactive mode

**Options:**
- `-d, --dir <path>` -- workspace root (default: `.`)
- `--rules <rules>` -- comma-separated rule names (omit to evaluate all)
- `-o, --output <format>` -- `table` (default) or `json`
- `-x, --explain` -- show facts and reasoning
- `-i, --interactive` -- guided spec/rule/fact selection
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
lemma run spec~a1b2c3d4
lemma run -i
```

### `lemma show` -- inspect spec structure

Shows facts, rules, and content hash. Use the hash with `spec~hash` to pin evaluation.

```bash
lemma show <spec> [-d <path>] [--effective <datetime>] [--hash]
```

**Options:**
- `--hash` -- output only the content hash (for piping, e.g. `lemma run spec~$(lemma show spec --hash)`)

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
lemma server [-d <path>] [--host <host>] [-p <port>] [--watch] [--proofs]
```

**Options:**
- `-d, --dir <path>` -- workspace root (default: `.`)
- `--host <host>` -- bind address (default: `127.0.0.1`)
- `-p, --port <port>` -- port (default: `8012`)
- `--watch` -- live-reload on `.lemma` file changes
- `--proofs` -- enable proof generation (clients send `x-proofs` header)

**Routes:**

| Method | Route | Description |
|--------|-------|-------------|
| GET | `/{spec}?fact=value` | Evaluate all rules (facts as query params) |
| POST | `/{spec}` | Evaluate all rules (facts as JSON body) |
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
lemma mcp [-d <path>] [--admin]
```

**Options:**
- `-d, --dir <path>` -- workspace root (default: `.`)
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
