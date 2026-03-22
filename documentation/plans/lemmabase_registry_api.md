# LemmaBase.com Registry API

## Purpose

LemmaBase.com is the default Registry for the Lemma engine. Its **Registry API** serves Lemma source text for `@...` identifiers so that clients (CLI, LSP, WASM, etc.) can resolve `spec @...` and `type ... from @...` references before loading specs into the engine (`load` / `load_from_paths` after `resolve_registry_references`).

This document specifies the API that LemmaBase.com must expose for Registry resolution. It does not cover the separate **evaluation API** (e.g. `GET /pricing?quantity=10`) described in `lemma-openapi`; that can be implemented and versioned independently.

---

## Status

Draft — for implementation.

---

## Client contract (what the engine does today)

The Lemma engine (see `engine/src/registry.rs`, `LemmaBase`) behaves as follows. LemmaBase.com must satisfy this contract.

- **Base URL:** `https://lemmabase.com`
- **Source request:** One endpoint is used for both spec and type resolution. The client does not distinguish; both call the same URL.
- **Request:** `GET /@{identifier}.lemma`
  - Identifier = the part after `@` in the Lemma reference (e.g. `spec @org/example/helper` → identifier `org/example/helper`).
  - Identifier can contain slashes. Examples: `user/workspace/somespec`, `lemma/std/finance`, `org/team/project/subdir/spec`.
- **Success:** HTTP 200, response body = Lemma source as **plain text** (UTF-8). The client reads the body as a string and parses it as Lemma. No JSON wrapper; raw `.lemma` content.
- **Failure:** The client maps status codes to user-facing errors:
  - **404** → "not found" (spec/type does not exist or is not visible).
  - **401, 403** → "unauthorized" (auth required or forbidden).
  - **500–599** → "server error".
  - Any other non‑success status → generic error.
  - Transport failure (timeout, DNS, etc.) → "network error" (server is not involved).

The client does **not** send Accept headers for content negotiation, nor does it require specific headers. A simple GET with a stable URL is enough.

---

## API specification

### Endpoint: fetch Lemma source

| Item | Specification |
|------|----------------|
| Method | `GET` |
| Path | `/@<identifier>.lemma` |
| Path semantics | `<identifier>` is the Registry identifier and may include `/` (e.g. `org/example/helper`). So the path looks like `/@org/example/helper.lemma`. The server must route so that the entire segment between `@` and `.lemma` is the identifier. |
| Query / body | None required. |
| Success | **200 OK**. Body = Lemma source (UTF-8). Recommended `Content-Type: text/plain; charset=utf-8`. |
| Not found | **404 Not Found** when the identifier has no published spec (or type bundle). |
| Unauthorized | **401 Unauthorized** or **403 Forbidden** when the resource exists but the request is not allowed. (Auth is not in the engine yet; see “Future work”.) |
| Server error | **500–599** for internal errors. |

**Response body (success):** Valid Lemma source: one or more `spec ...` blocks. Spec names in the source use **plain names without** the `@` prefix (e.g. `spec org/example/helper`). The `@` is only used in references, not in declarations.

**Identifier format:** The engine does not impose a schema. Identifiers are opaque strings; slashes are used by convention (e.g. `org/project/spec`). LemmaBase.com can define its own naming and storage layout (e.g. path-like, or keyed by ID).

---

### Optional: human-facing page (navigation URL)

The LSP and other tools use `url_for_id(identifier)` to build “open in browser” links: `https://lemmabase.com/@{identifier}` (no `.lemma`). The engine does not require this URL to return Lemma source; it is for humans. Options:

- **GET /@<identifier>** returns HTML (e.g. a spec page, or a redirect to the canonical page).
- Or return 404 if you do not provide a human UI yet.

This is optional for Registry resolution but improves UX when users click through from the IDE.

---

## Implementation checklist

- [ ] **Routing:** Accept `GET /@...` paths where the identifier may contain slashes (e.g. one catch-all route or equivalent: `/@*identifier.lemma`).
- [ ] **Storage / resolution:** Map identifier → Lemma source (DB, object store, or filesystem). Decide identifier namespace and visibility (public vs private, org boundaries).
- [ ] **Success response:** 200, body = raw Lemma source, `Content-Type: text/plain; charset=utf-8`.
- [ ] **Errors:** 404 (not found), 401/403 (unauthorized/forbidden when auth exists), 5xx (server error). No special error body required for the engine; status code is enough.
- [ ] **Optional:** GET `/@<identifier>` (without `.lemma`) for human-facing page or redirect.
- [ ] **Optional:** Caching (e.g. `Cache-Control`, `ETag`) to reduce load; client does not depend on it.

---

## Out of scope / future work

- **Authentication and authorization:** Not part of the current Registry trait. When added, the server may require tokens or cookies and return 401/403 as above. Design (headers, flows) TBD.
- **API stability:** The Registry API is not yet declared stable (see [registry.md](../registry.md)); expect possible small changes (e.g. optional query params, headers) before a 1.0.
- **Versioning:** The client does not send a version (e.g. `@org/spec@v2`). If LemmaBase.com introduces versions later, the default could be “latest” and version selection could be added via query or path.
- **Evaluation API:** Serving and evaluating specs (e.g. `GET /pricing?quantity=10`) is a separate API; see `lemma-openapi`.

---

## Summary

| Concern | Requirement |
|--------|-------------|
| URL | `GET https://lemmabase.com/@{identifier}.lemma` |
| Identifier | Path segment(s) between `@` and `.lemma`; may contain `/`. |
| Success | 200, body = Lemma source (plain text, UTF-8). |
| Errors | 404, 401/403, 5xx as described. |
| Optional | Human-facing `GET /@{identifier}`; caching headers. |

This is the minimum needed for LemmaBase.com to act as the default Registry and for the engine to resolve `@...` references successfully.
