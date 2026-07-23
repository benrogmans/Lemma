---
nav_title: Elixir
nav_order: 20
---

# Elixir

`lemma_engine` provides precompiled NIFs for Elixir (>= 1.14). Erlang and Gleam use the same package.

Precompiled binaries are downloaded automatically for macOS (arm64/x86_64), Linux (gnu x86_64 and arm64), and Windows (arm64/x86_64).

## Install

Add to `mix.exs`:

```elixir
def deps do
  [{:lemma_engine, "~> 0.9"}]
end
```

Or from git:

```elixir
{:lemma_engine, git: "https://github.com/lemma/lemma", sparse: "engine/packages/hex"}
```

## Usage

```elixir
{:ok, engine} = Lemma.new()

:ok = Lemma.load(engine, """
spec pricing
data quantity: number
data price: 10
rule total: quantity * price
rule discount: 0
  unless quantity >= 10 then 5
  unless quantity >= 50 then 15
""")

{:ok, response} = Lemma.run(engine, %{spec: "pricing"}, %{data: %{"quantity" => "25"}})
```

Introspect loaded Specs:

```elixir
{:ok, groups} = Lemma.list(engine)
{:ok, show} = Lemma.show(engine, nil, "pricing")
{:ok, workspace_source} = Lemma.source(engine, nil, nil, nil)
{:ok, stdlib} = Lemma.source(engine, "lemma", nil, nil)
```

Format source code (no engine needed):

```elixir
{:ok, formatted} = Lemma.format("spec foo\ndata x: 1\nrule y: x + 1")
```

## API

| Function | Description |
|----------|-------------|
| `Lemma.new/1` | Create engine (optional limits map) |
| `Lemma.load/2` | Load sources: binary (volatile) or map / `[{label, code}, ...]` (labeled) |
| `Lemma.list/1` | List loaded Specs (includes embedded `lemma` / `spec units`) |
| `Lemma.source/4` | Formatted Lemma source (`repository`, `spec`, `effective`; omit `spec` for repo-wide) |
| `Lemma.show/4` | Spec interface + temporal window (`repository`, `spec`, `effective`) |
| `Lemma.run/3` | Evaluate: `target` map (`repo`, `spec`, `effective`), `options` map (`data`, `rules`, `explain`). Each rule result may include `missing_data` (unbound input keys). With `explain: true`, `explanation` matches [explanation.v1.json](../schemas/explanation.v1.json). Types and suggestions are on `Lemma.show/4` only. |
| `Lemma.remove/4` | Remove temporal slice: `repository`, `spec`, `effective` |
| `Lemma.format/1` | Format Lemma source code (no engine needed) |

`Lemma.OpenAPI` is a separate module (HTTP OpenAPI document helpers via `lemma_openapi`). It is not part of the core `Lemma` engine table above.

## Engine lifecycle

Each `Lemma.new/1` call creates an independent engine. The engine reference is safe to use from a single process. For shared access across processes, wrap it in a GenServer.
