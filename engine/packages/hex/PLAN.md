# Plan: Hex package (Rustler) for Lemma engine

## Goal
Elixir/Erlang library on Hex that wraps the Lemma engine via Rustler NIFs. **Slim API** aligned with a small set of NIFs: create engine (optional limits), load (string or paths), list loaded specs, schema, run, invert, remove_spec. **Elixir names mirror NIFs** (drop `lemma_` prefix): `lemma_list` → `Lemma.list`, `lemma_load` → `Lemma.load`, `lemma_schema` → `Lemma.schema`, etc.; `lemma_new` → `Lemma.new`. `add_dependency_files`, `run_json` stay out of scope.

## 1. Layout

```
engine/packages/hex/
  mix.exs                 # Elixir project, deps: rustler, jason
  lib/
    lemma.ex               # Public API
    lemma/native.ex        # Rustler module (load NIF)
  native/
    lemma_hex/
      Cargo.toml           # rustler, lemma-engine (path = "../../../../")
      src/
        lib.rs             # NIF definitions + engine resource
        error_encoding.rs  # Error → Elixir term conversion
  test/
    lemma_test.exs         # ExUnit tests
  README.md
```

- **Cargo workspace**: Add `engine/packages/hex/native/lemma_hex` to repo root `Cargo.toml` `members` so the NIF crate can depend on `engine` (lemma-engine). Path dep in lemma_hex: `lemma-engine = { path = "../../../../" }` (native/lemma_hex → engine).

## 2. Engine state

- Hold one `Engine` per NIF resource (Rustler `Resource`). Elixir gets an opaque ref; all NIFs that take that ref receive the resource and call engine methods.
- **No** global engine: allow multiple engines (e.g. per process or per tenant). Elixir devs can wrap the ref in a GenServer for a shared "global" rules service if they like.

## 3. NIFs (Rust)

| NIF | Args | Returns |
|-----|------|--------|
| `lemma_new` | optional `limits_map`: omit / nil / `%{}` ⇒ default `ResourceLimits`; if a map is passed, keys are optional (e.g. `max_files`, `max_loaded_bytes`, `max_file_size_bytes`, `max_total_expression_count`, …) and merge onto defaults | `{:ok, resource}` \| `{:error, reason}` |
| `lemma_load` | resource, code_binary, source_label_binary | `:ok` \| `{:error, errors}` |
| `lemma_load_from_paths` | resource, paths (list of path binaries) | `:ok` \| `{:error, errors}` |
| `lemma_list` | resource | `{:ok, list}` (each item e.g. `%{name, effective_from}`) |
| `lemma_schema` | resource, spec_binary, effective_opt | `{:ok, schema_json_binary}` \| `{:error, term}` |
| `lemma_run` | resource, spec_binary, effective_opt, data_values (map) | `{:ok, response_json_binary}` \| `{:error, term}` |
| `lemma_invert` | resource, spec_name_binary, effective_binary, rule_name_binary, target_term, values (map) | `{:ok, inversion_response_json_binary}` \| `{:error, term}` |
| `lemma_remove_spec` | resource, spec_name_binary, effective_binary | `:ok` \| `{:error, term}` |
| `lemma_format` | code_binary | `{:ok, formatted_binary}` \| `{:error, term}` |

- **`lemma_new`**: No argument, nil, or empty map ⇒ `Engine::new()`. Non-empty map ⇒ `Engine::with_limits(...)` after filling defaults for omitted keys. Malformed limits map ⇒ `{:error, reason}` (do not silently fall back to defaults).
- **Error encoding**: Convert `lemma::Error` (and `Vec<Error>`) to Elixir terms: map/list with `:message`, `:location` (file/line/column), `:suggestion` where present.
- **effective_opt**: If nil, use `DateTimeValue::now()`; else parse datetime string (engine format) to `DateTimeValue`.
- **Response / Schema / InversionResponse**: Serialize in Rust with `serde_json::to_vec`; Elixir decodes to map via Jason.
- **Target (invert)**: Elixir passes a map with **atom keys** (`:outcome`, `:op`, `:value`, `:message`). NIF looks up atom keys and builds `lemma::inversion::Target`.
- **load_from_paths**: Passes `recursive: false` to engine. Only on non-wasm (native NIF); engine's `load_from_paths` is `#[cfg(not(target_arch = "wasm32"))]`. Enforces engine limits.
- **`term_to_string`**: Must return `Err` for unsupported term types instead of silently returning an empty string.

## 4. Type conversions (Rust)

- **Data**: Elixir map string → string. NIF builds `HashMap<String, String>` for `run` / `invert`.
- **effective**: Elixir passes `nil` or datetime string; Rust parses to `DateTimeValue` or uses `DateTimeValue::now()`.
- **Source type**: `source_label_binary`: use `SourceType::Labeled(str)` if non-empty, else `SourceType::Inline` (Elixir can pass `"inline"` when no label).
- **Paths**: List of binaries → `Vec<PathBuf>` for `load_from_paths`.
- **ResourceLimits** (`lemma_new`): Entire limits argument optional. When provided, Elixir map with string keys and integer values. Partial map ⇒ merge onto defaults. Invalid key/value types ⇒ error (not silent fallback).
- **List** (`lemma_list` / Elixir `Lemma.list/1`): Wrap `Engine::list_specs()`; return list of maps `%{name: binary, effective_from: binary | nil}` (nil when no effective_from, not empty string).
- **Target (invert)**: Decode from Elixir term with **atom keys** (`:op`, `:outcome`, optional `:value`/`:message`) into `lemma::inversion::Target`; `LiteralValue` for value outcome parsed from string.

## 5. Build integration

- **Rustler**: In `mix.exs`, use `rustler` with local compile: `mix compile` triggers Rustler to compile `native/lemma_hex`.
- **Rust target**: Same as default target (no wasm).
- **Workspace**: Root `Cargo.toml`: add `engine/packages/hex/native/lemma_hex` to `members`.

## 6. Rust crate deps (lemma_hex)

- `rustler = "0.37"`
- `lemma-engine = { path = "../../../../" }` (engine crate)
- `serde_json` (workspace)
- `rust_decimal = "1"` (for parsing numeric values in target/data)

## 7. Elixir API

Public functions on `Lemma` match the NIF stem (drop `lemma_`): `load`, `load_from_paths`, `list`, `schema`, `run`, `invert`, `remove_spec`, `new`, `format`. Exact arities follow NIF args + optional keyword sugar (e.g. `run(engine, spec, opts)`).

```elixir
# Create engine; limits optional (%{} = defaults)
{:ok, engine} = Lemma.new()
{:ok, engine} = Lemma.new(%{"max_files" => 100, "max_loaded_bytes" => 10_000_000})

# Load
:ok = Lemma.load(engine, "spec foo\ndata x: 1\nrule y: x + 1", "my_spec.lemma")
:ok = Lemma.load_from_paths(engine, ["/app/priv/specs", "/app/other.lemma"])
{:error, errors} = Lemma.load(engine, "spec foo\ndata x: bad", "spec.lemma")

# Introspection
{:ok, specs} = Lemma.list(engine)
{:ok, schema} = Lemma.schema(engine, "foo", effective: nil)   # or "foo^hash", effective: "2024-01-15"

# Run (data: string map)
{:ok, response} = Lemma.run(engine, "foo", [])
{:ok, response} = Lemma.run(engine, "foo", effective: "2024-01-15", data: %{"x" => "2"})
{:error, reason} = Lemma.run(engine, "foo", [])

# Invert
{:ok, inversion} = Lemma.invert(engine, "foo", "2024-01-15", "my_rule", target, %{"x" => "1"})

# Remove spec
:ok = Lemma.remove_spec(engine, "foo", "2024-01-15")
```

- `response` / `schema` / `inversion`: maps from decoded JSON. Document shapes in module docs.

## 8. Error handling

- **Load**: Returns `{:error, list}` where each element is a map (e.g. `%{message: "...", location: %{file: "...", line: 1}, suggestion: "..."}`). No exception for load.
- **schema / run / invert / remove_spec**: Returns `{:error, term}` (single error map or similar). No panic in NIF; map engine `Error` to term.
- **new (limits)**: Malformed limits map (non-string key, non-integer value) returns `{:error, reason}` — never silently ignores bad input.
- **term_to_string**: Unsupported term types return error, not empty string.

## 9. Testing

- **ExUnit**: Engine lifecycle (new, load, run, schema, list, remove_spec); invalid spec → `{:error, _}`; invert with a minimal spec. Prefer string load; optional `load_from_paths` with a temp dir.

## 10. Out of scope (this plan)

- `Engine::run_json`, `add_dependency_files`, `list_specs_effective`, `get_spec_rules` — not exposed; extend later if needed.
- Remote registry fetch in Elixir, not in NIF.
- No LSP.

## 11. Documentation

- README: how to add dep (Hex + git), that Rust toolchain is required to compile the NIF, and minimal example (new, load, run).
- Module docs for `Lemma` with typespecs and examples.

## 12. Checklist (implementation order)

1. ~~Add `engine/packages/hex/native/lemma_hex` to workspace `members` (root `Cargo.toml`).~~
2. ~~Create `native/lemma_hex/Cargo.toml` (rustler, lemma-engine path `../../../../`).~~
3. ~~Implement NIFs: `lemma_new` (limits map), `lemma_load`, `lemma_load_from_paths`; error encoding.~~
4. ~~Implement `lemma_list` → `Engine::list_specs`, `lemma_schema`, `lemma_run`, `lemma_remove_spec`.~~
5. ~~Implement `lemma_invert` (target term ↔ `Target`, JSON response).~~
6. ~~Create mix project `engine/packages/hex/mix.exs` (rustler, compile config for lemma_hex).~~
7. ~~Implement `Lemma` and `Lemma.Native` in Elixir (functions matching §3).~~
8. Fix NIF robustness issues (silent fallbacks in `limits_from_term`, `term_to_string`, `decode_target` atom-vs-string keys, `lemma_list` nil for missing effective_from).
9. ExUnit tests (lifecycle, load/run/schema/list/remove_spec, invert, errors).
10. README and module docs.
