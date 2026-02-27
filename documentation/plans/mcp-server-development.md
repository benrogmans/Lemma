# MCP Server Development Plan

## Business Objective

Organisations in regulated and high-stakes domains (finance, insurance, healthcare, compliance) need LLM-powered applications they can trust. Today, LLMs hallucinate numbers and invent rules. The Lemma MCP (Model Context Protocol) server turns the LLM into a reliable interface to business logic: the model queries Lemma for every domain-specific answer, and Lemma returns deterministic results with step-by-step reasoning. This means:

- **No hallucinated values.** Every number, condition, and outcome comes from evaluated Lemma rules, not the model's weights.
- **Auditable answers.** Reasoning traces provide a paper trail that satisfies compliance, legal review, and customer trust.
- **Safe deployment.** Read-only by default; the LLM cannot modify business rules unless the deployer explicitly allows it.

The MCP server is the product surface that makes Lemma usable inside any MCP-compatible AI assistant (Claude, Cursor, custom agents) without writing integration code.

---

## Principles

1. **Determinism** — All domain logic (numbers, conditions, eligibility) comes from Lemma evaluation. The LLM does not invent values.
2. **Transparency** — Users (and the LLM) can see which rules and facts were used. Reasoning traces provide an audit trail.
3. **Sandbox** — Lemma evaluation is pure and side-effect free; the MCP server only exposes evaluation and document introspection, not arbitrary execution.
4. **Plain text to the LLM** — All tool responses are plain text. No JSON structures, no nested trees. Models handle linear text well; nested structures are error-prone.

---

## User Journey

A typical interaction looks like this:

1. User asks: "Am I eligible for the premium discount?"
2. LLM calls `list_documents` — sees document `pricing` with facts (`quantity`, `is_vip`) and rules (`discount`, `total`) including their types.
3. LLM calls `evaluate` for `pricing.discount` with `quantity=25, is_vip=false`.
4. Response: `discount = 10 percent` with reasoning trace:
   - `quantity = 25`
   - `unless clause 2: quantity >= 50 is false, skipped`
   - `unless clause 1: quantity >= 10 is true, matched`
5. LLM tells the user: "You qualify for a 10% discount because your quantity of 25 meets the >= 10 threshold."

The LLM never invented the 10% — it came from Lemma. The reasoning trace lets the LLM explain exactly why.

---

## Current Capabilities

All core functionality is implemented.

### Tools

| Tool | Description |
|------|-------------|
| **list_documents** | Lists all loaded documents with full schemas: fact names, types, defaults, and rule names with return types. Uses `DocumentSchema` from the engine (shared with the HTTP server and CLI). |
| **evaluate** | Evaluates a single rule with optional fact values. Returns the result (value or veto) and a plain-text reasoning trace showing which facts were used, which unless clauses matched or were skipped, and how the result was derived. |
| **add_document** | Adds Lemma code to the engine. Returns the document schema on success. Admin only. |
| **get_document_source** | Returns the canonical Lemma source code for a document. Useful for inspecting or debugging the rules behind an evaluation result. Admin only. |

### Read-only by Default; Admin Opt-in

The server is read-only by default. Only `list_documents` and `evaluate` are advertised. Admin tools (`add_document`, `get_document_source`) are not listed and return an error if called.

To enable admin tools, the deployer starts the server with `--admin`:

```
lemma mcp --admin
```

No other auth model. Capability is determined by the flag.

### Workspace Loading

The server loads `.lemma` files from a directory at startup via `--dir`:

```
lemma mcp --dir ./rules
```

This is the same workspace loading logic used by the CLI and HTTP server.

---

## Design Decisions

### Schema lives in list_documents, not a separate tool

Instead of a `get_document_schema` tool, `list_documents` returns full schemas inline. The LLM's flow is: discover what's available → evaluate a rule. Collapsing discovery and schema into one call eliminates a round-trip. If document count grows large enough to be a problem, a per-document schema tool can be added later.

### Proofs are flat plain-text reasoning traces

The engine computes structured `Proof` trees with nested nodes (Value, RuleReference, Computation, Branches, Condition, Veto). (RuleReference here is the proof-tree node for evaluating a rule; in Lemma source, references are written by name without a `?` suffix.) The MCP server linearises these into a flat list of one-line-per-step reasoning traces. No JSON proof trees are sent to the LLM.

The lineariser walks the proof tree depth-first:
- Facts are emitted as `name = value` (deduplicated across branches).
- Unless branches are emitted as `unless clause N: condition is true/false, matched/skipped`.
- Computations are emitted as `expression = result`.
- Rule references are expanded recursively, then summarised as `rule_name = result`.
- Veto is emitted as `veto: reason`.

### DocumentSchema has a Display impl in the engine

`DocumentSchema::fmt` produces the plain-text format used by the MCP server. This avoids MCP-specific formatting code; any consumer (MCP, CLI, future tools) can call `.to_string()`.

### add_document returns the schema

After a successful add, the response includes the schema of the newly added document(s). This lets the LLM immediately know what facts and rules are available without an extra `list_documents` call.

---

## Remaining Work

### MCP resources for documents (optional, low priority)

Expose documents as MCP resources (e.g. `lemma://document/<doc_name>`) so the LLM can read document source or schema via the resources protocol. The current tools may be sufficient; this is only needed if clients prefer the resources interface.

---

## Out of Scope

- **Inversion.** The engine supports inversion (finding input domains that produce a target outcome) but it is not exposed via MCP. It adds complexity and the use case for LLM-driven inversion is unclear.
- **Streaming.** Evaluation is fast enough that streaming results is unnecessary.
- **Multi-rule evaluate.** One document, one rule per call. Keeps the interface simple and the reasoning traces focused.

---

## Summary

The MCP server gives LLMs deterministic access to Lemma business rules. It exposes `list_documents` (with schemas) and `evaluate` (with reasoning traces) by default. Admin tools (`add_document`, `get_document_source`) are available via `--admin`. All responses are plain text. The server is read-only by default. Workspace loading happens at startup.
