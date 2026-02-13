# Server Redesign

## Overview

Redesign the API server so that `lemma server` automatically generates a typed REST API with interactive OpenAPI documentation based on the loaded Lemma documents. Add a `--watch` flag for live-reloading when `.lemma` files change.

## Status: done

## Auto-generated routes

For each loaded Lemma document, the server generates two endpoints. For example, a document called `pricing` with facts `quantity` (number) and `is_member` (boolean), and rules `discount`, `total`, `vat`:

- **`GET /@pricing?quantity=10&is_member=true`** — evaluate all rules, facts as query parameters
- **`POST /@pricing`** with JSON body `{"quantity": 10, "is_member": true}` — evaluate all rules, facts as JSON

Rules can optionally be filtered by appending them as a comma-separated path segment:

- **`GET /@pricing/discount`** — evaluate only the `discount` rule
- **`GET /@pricing/discount,total`** — evaluate the `discount` and `total` rules
- **`POST /@pricing/discount,vat`** — same pattern for POST

Both GET and POST return the same response shape: rule results with values or veto reasons.

The `@` prefix reserves the top-level namespace for server meta routes (`/docs`, `/health`, `/openapi.json`, etc.) and prevents collisions with document names.

## Meta routes

- **`GET /`** — list all available documents (names, fact counts, rule counts)
- **`GET /openapi.json`** — dynamically generated OpenAPI 3.1 specification
- **`GET /docs`** — Scalar interactive documentation UI (static HTML pointing to `/openapi.json`)
- **`GET /health`** — health check

## OpenAPI generation

### Shared crate: `lemma-openapi`

The Lemma-to-OpenAPI mapping logic lives in a small shared crate (`lemma-openapi`) that depends on `lemma-engine`. This crate is used by both `lemma-cli` (for the `lemma server` command) and LemmaBase.com (for its hosted API). Keeping this logic in a shared crate avoids duplication and ensures both produce identical OpenAPI specifications for the same documents.

The crate takes an `Engine` (or its document/plan data) and produces an OpenAPI 3.1 specification as JSON.

### Specification structure

For each document:

- **Path:** `/@{doc_name}` and `/@{doc_name}/{rules}` (comma-separated rule names)
- **GET operation:** facts become query parameters, typed
- **POST operation:** facts become a JSON request body schema
- **Response schema:** rule names as fields, each with value or veto

### Lemma type to OpenAPI type mapping

**GET (query parameters):** all values are strings (inherent to query params). The description for each parameter documents the expected format, available units, and constraints.

- `number` → `string` — Example: `?quantity=10`
- `scale` → `string` — Example: `?price=100+eur`
- `ratio` → `string` — Example: `?tax_rate=21+percent`
- `text` → `string` — with `enum` if options are defined
- `boolean` → `string` — Example: `?is_member=true`
- `date` → `string` — Example: `?deadline=2024-01-15`
- `time` → `string` — Example: `?start=14:30:00`
- `duration` → `string` — Example: `?workweek=40+hours`

**POST (JSON body):** uses native JSON types where possible. Unit-typed values use a structured object with `value` (number) and `unit` (string enum).

- `number` → `number` — Example: `{"quantity": 10}`
- `scale` → `object` with `value` (number) and `unit` (string enum) — Example: `{"price": {"value": 100, "unit": "eur"}}`
- `ratio` → `object` with `value` (number) and optional `unit` (string enum) — Example: `{"tax_rate": {"value": 21, "unit": "percent"}}`
- `text` → `string` — with `enum` if options are defined. Example: `{"name": "Alice"}`
- `boolean` → `boolean` — Example: `{"is_member": true}`
- `date` → `string` (format: date) — Example: `{"deadline": "2024-01-15"}`
- `time` → `string` (format: time) — Example: `{"start": "14:30:00"}`
- `duration` → `object` with `value` (number) and `unit` (string enum) — Example: `{"workweek": {"value": 40, "unit": "hours"}}`

The server converts structured POST input to the engine's `HashMap<String, String>` format (for example `{"value": 100, "unit": "eur"}` becomes `"100 eur"`). In Scalar's "Try it" UI, the structured objects render as a number input with a unit dropdown, providing a good documentation experience.

## Scalar interactive documentation

Served as a static HTML response at `/docs`. The Scalar API reference JavaScript bundle is vendored (embedded at compile time via `include_str!`) so the server has zero external runtime dependencies — no CDN required.

```html
<!doctype html>
<html>
<head><title>Lemma API</title></head>
<body>
  <div id="app"></div>
  <script src="/scalar.js"></script>
  <script>
    Scalar.createApiReference('#app', { url: '/openapi.json' })
  </script>
</body>
</html>
```

Scalar provides a built-in "Try it" feature for making requests directly from the documentation.

## Watch mode (`--watch`)

- New `--watch` flag on the `server` command.
- Uses the `notify` crate for filesystem watching.
- Watches the workspace directory for `.lemma` file changes (create, modify, delete).
- Debounces rapid changes (a few hundred milliseconds).
- On change: creates a new `Engine`, loads all `.lemma` files, swaps into the `Arc<RwLock<Engine>>`.
- The OpenAPI specification regenerates automatically (it is derived from the engine state on each request).

## Dependencies

- `lemma-openapi` — new shared crate for Lemma-to-OpenAPI mapping (depends on `lemma-engine` and `serde_json`)
- `notify` — filesystem watcher, for `--watch` mode
- `serde_json` — already present
- Scalar API reference JS is vendored at compile time (no CDN dependency)

## What gets removed

The current `POST /evaluate` (inline code execution) endpoint is removed. The server becomes purely about serving the loaded workspace documents as typed API endpoints.
