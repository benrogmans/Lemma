---
layout: default
title: CLI Guide
---

# Lemma CLI

The Lemma CLI provides everything you need to work with Lemma: document evaluation, workspace management, and HTTP/MCP servers.

## Installation

```bash
cargo install lemma
# or
cargo build --release
```

## Commands

### `lemma run` - Evaluate a document

Run rules in a workspace and see the results.

```bash
lemma run [<document>[:<rules>]] [facts...] [-d <path>] [-r|--raw] [-i|--interactive]
```

**Syntax:**
- `document` - evaluates all rules in the document
- `document:rule` - evaluates only the specified rule
- `document:rule1,rule2,rule3` - evaluates multiple specific rules (comma-separated)
- No arguments with `-i` - launches interactive mode

**Options:**
- `-d, --dir <path>` - Workspace root directory (default: `.`)
- `-r, --raw` - Output raw values only (for piping to other tools)
- `-i, --interactive` - Enable interactive mode with:
  - Fuzzy-searchable document selection
  - Multi-select rule picker
  - Type-aware fact input (calendar picker for dates, examples for other types)

**Examples:**
```bash
# Evaluate all rules in a document
lemma run pricing -d ./policies

# Evaluate only the total rule
lemma run pricing:total base_price=200 quantity=10

# Evaluate multiple specific rules
lemma run pricing:subtotal,tax,total base_price=200

# Get raw values for piping to other tools
lemma run pricing:total -r base_price=200

# Pipe result to jq or other tools
lemma run pricing:total -r base_price=200 | xargs echo "Total:"

# Interactive mode (guided prompts for document, rules, and facts)
lemma run -i

# Interactive fact entry for specific document
lemma run pricing -i

# Interactive fact entry for specific rules
lemma run pricing:total,tax -i

# Interactive mode with date picker for date fields
lemma run examples/date_handling -i -d docs/examples

# Using long form flags
lemma run pricing --dir ./policies --raw
```

**Output Format:**

Default output shows a table with evaluation steps:
```
┌───────────┬─────────────────────────────────────────┐
│ Rule      │ Evaluation                              │
├───────────┼─────────────────────────────────────────┤
│ total     │ 242.00 USD                              │
│           │                                         │
│           │   0. fact base_price = 200.00 USD       │
│           │   1. rule subtotal = 200.00 USD         │
│           │   2. rule tax = 42.00 USD               │
│           │   3. add(200.00, 42.00) → 242.00        │
│           │   4. result = 242.00 USD                │
└───────────┴─────────────────────────────────────────┘
```

Raw output (`--raw`) shows only values (perfect for piping):
```
242.00 USD
```

**Note:** When evaluating specific rules, their dependencies are still computed but only the requested rules appear in the output.

### `lemma show` - Show document structure

View the structure of a document including facts, rules, and required inputs.

```bash
lemma show <document> [-d <path>]
```

**Example:**
```bash
lemma show pricing -d ./policies
```

### `lemma list` - List all documents

Load and display information about all documents in a workspace.

```bash
lemma list [path]
```

**Example:**
```bash
lemma list ./policies
```

### `lemma serve` - Start HTTP server

Start an HTTP REST API server with a pre-loaded workspace.

```bash
lemma server [-d <path>] [--host <host>] [-p <port>]
```

**Options:**
- `-d, --dir` - Workspace root directory (default: `.`)
- `--host` - Host to bind to (default: `127.0.0.1`)
- `-p, --port` - Port to bind to (default: `3000`)

**Example:**
```bash
lemma server -d ./policies -p 8080
```

**API Endpoints:**

```bash
# Health check
GET /health

# Evaluate pre-loaded document with facts as query params
GET /evaluate/{document}?fact1=value1&fact2=value2

# Evaluate inline code
POST /evaluate
Content-Type: application/json
{
  "code": "doc example\nfact x = 5\nrule y = x * 2",
  "facts": {
    "x": 100
  }
}
```

**Response Format:**
```json
{
  "results": [
    {
      "name": "rule_name",
      "value": "computed_value",
      "veto_reason": null
    }
  ],
  "warnings": []
}
```

### `lemma mcp` - Start MCP server

Start a Model Context Protocol server for AI assistant integration.

```bash
lemma mcp [-d <path>]
```

**Options:**
- `-d, --dir` - Workspace root directory (default: `.`)

The MCP server provides AI assistants with tools to:
- Add and evaluate Lemma documents
- Inspect document structure
- Query rules with fact overrides

## Workspace Structure

A workspace is a directory containing `.lemma` files:

```
policies/
├── pricing.lemma
├── shipping.lemma
└── tax.lemma
```

The CLI automatically loads all `.lemma` files and makes their documents available for evaluation.

## Configuration Files

### Built-in Documents

## Examples

### Basic Workflow

```bash
# 1. Create a workspace
mkdir policies
cd policies

# 2. Create a Lemma file
cat > pricing.lemma << 'EOF'
doc pricing
fact base_price = 100
fact quantity = 1
rule total = base_price * quantity * 1.1
EOF

# 3. Evaluate it
lemma run pricing

# 4. Override facts
lemma run pricing base_price=200 quantity=5

# 5. Show document structure
lemma show pricing

# 6. Start HTTP server
lemma server -p 3000
```

### HTTP Server Usage

```bash
# Start server
lemma server --workdir ./policies &

# Evaluate with inline code
curl -X POST http://localhost:3000/evaluate \
  -H "Content-Type: application/json" \
  -d '{
    "code": "doc calc\nfact x = 10\nrule double = x * 2",
    "facts": {"x": 25}
  }'

# Response:
# {
#   "results": [{"name": "double", "value": "50"}],
#   "warnings": []
# }
```

## Features

- **Document Listing** - Load and list all documents from a directory
- **CLI Evaluation** - Run documents from command line with operation records
- **Interactive Mode** - Guided prompts with fuzzy search, multi-select, and calendar date picker
- **Raw Output Mode** - Extract values for piping to other Unix tools
- **HTTP Server** - REST API with both stateful and stateless evaluation
- **Document Inspection** - View document structure and requirements
- **MCP Server** - Model Context Protocol for AI assistant integration

## Performance

- Document loading: ~1ms per document
- Rule evaluation: <1ms simple, <10ms complex
- Server startup: instant with pre-loaded workspace
- Memory: ~1KB per fact, ~2KB per rule

## Troubleshooting

### "Document not found"
Make sure your `.lemma` files are in the workspace directory and contain valid `doc` declarations.

### "Address already in use"
Another process is using the port. Try a different port:
```bash
lemma server --port 8080
```

### Parse errors
Check your Lemma syntax. Use `lemma show` to verify the document loads correctly.

## See Also

- [Language Reference](index.md)
- [Examples](examples/)
- [API Documentation](servers.md)

