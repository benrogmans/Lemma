# Feature Request: History Checksum

## Summary

A temporal checksum primitive for the Lemma engine that hashes all spec content effective at or before a given datetime boundary. Enables consumers to verify that upstream dependencies haven't altered historical content while still accepting forward extensions.

## Motivation

When a spec depends on a floating dependency (e.g., `uses @demo/finance units`), the dependency can change at any time. Consumers need a way to detect whether the upstream altered content they already committed to, without freezing or duplicating the dependency content.

Current workarounds (frozen copies, publish-time guards, cascading republishes) all push temporal logic into the registry (LemmaBase), making it complex. The engine should own this instead.

## Design

### New engine function

```
history_checksum(engine, repository, through: DateTime) -> String
```

Computes a deterministic hash over all spec content in `repository` where `effective_from <= through`, using canonical formatted source in stable sort order by `(name, effective_from)`.

### Properties

- Adding specs with `effective_from > through` does NOT change the checksum
- Modifying, adding, or removing any spec with `effective_from <= through` DOES change the checksum
- The hash algorithm and spec ordering must be stable across engine versions
- `through` can be past, present, or future

### Behavior table

| Upstream action | `effective_from` vs `through` | Checksum |
|---|---|---|
| Add new spec | after `through` | unchanged |
| Add new spec | at or before `through` | changed |
| Modify spec content | at or before `through` | changed |
| Remove spec | at or before `through` | changed |
| Modify spec content | after `through` | unchanged |

## Usage

### At publish time (LemmaBase or CLI)

When a consumer publishes, for each dependency:

1. Load the dep into the engine
2. Compute `history_checksum(engine, dep_id, through: locked_through)`
3. Store `{dep_id, history_checksum, locked_through}` on the publication

### At load/evaluation time (engine verification)

When loading a dependency:

1. Load the live dep content
2. Compute `history_checksum(engine, dep_id, through: locked_through)`
3. Compare against stored checksum
4. Match = content within the locked range is intact; extensions beyond it are welcome
5. Mismatch = historical content was altered; consumer decides how to handle

## `locked_through` as a business boundary

The `locked_through` datetime is consumer-configurable and represents a temporal commitment:

- `locked_through: now` -- standard: protect everything that was effective when you published. Accept future extensions freely.
- `locked_through: 2026-12-31` -- fiscal year lock: "I built my annual budget on these rates. Lock all of 2026. 2027 rates can change freely."
- `locked_through: 2025-12-31` -- loose lock: only protect last year, accept everything from this year onward.

## Consumer behavior by context

| Context | On checksum mismatch |
|---|---|
| CLI (`lemma fetch`) | Refuse update by default. `--accept` to re-lock with new checksum. |
| LemmaBase editor | Show "dependency updated within locked range" in UI. Republishing accepts the new checksum. |
| Server / evaluation API | Configurable policy (reject, warn, accept). |

## Implications for LemmaBase

LemmaBase becomes a dumb content store:

- Publishes freely, no temporal guard
- Serves live content from the current (unsuperseded) publication
- Stores `{dep_id, history_checksum, locked_through}` per publication dependency (audit + verification data)
- No frozen dependency copies in the resolution path
- No cascade, no subscriptions, no publish-time content comparison

The engine owns all temporal integrity verification.

## Implications for the CLI

`lemma fetch`:

- Fetches live content from the registry
- Computes history checksum of fetched content at the lockfile's `locked_through`
- Compares against lockfile checksum
- Mismatch + `effective_from <= locked_through` changed = refuse (default) or accept (`--accept`)
- Match or only changes beyond `locked_through` = update proceeds

## Implementation notes

- The checksum should be computed from the canonical formatted output (`format_repository` or per-spec formatted source), not raw file bytes, to be whitespace/formatting-invariant
- Spec identity for ordering: `(name, effective_from)` tuple, sorted lexicographically
- Hash algorithm: SHA-256 (matches existing `content_hash` convention in LemmaBase)
- The function must be deterministic: same content + same `through` = same checksum, always
- Consider exposing via both the Elixir NIF (`Lemma.history_checksum/3`) and the WASM/npm package for browser-side verification
