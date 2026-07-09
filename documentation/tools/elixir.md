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
  [{:lemma_engine, "~> 0.8"}]
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

{:ok, response} = Lemma.run(engine, "pricing", data: %{"quantity" => "25"})
```

Introspect loaded Specs:

```elixir
{:ok, groups} = Lemma.list(engine)
{:ok, schema} = Lemma.schema(engine, "pricing")
```

Format source code (no engine needed):

```elixir
{:ok, formatted} = Lemma.format("spec foo\ndata x: 1\nrule y: x + 1")
```

## API

| Function | Description |
|----------|-------------|
| `Lemma.new/1` | Create engine (optional limits map) |
| `Lemma.load/3` | Load Spec from string |
| `Lemma.load_from_paths/2` | Load Specs from file paths |
| `Lemma.list/1` | List loaded Specs (includes embedded `lemma` / `spec units`) |
| `Lemma.format_repository/2` | Formatted Lemma source for a repository |
| `Lemma.schema/3` | Get Spec schema (Data, Rules, types) |
| `Lemma.run/3` | Evaluate a Spec with Data |
| `Lemma.remove_spec/3` | Remove a Spec from the engine |
| `Lemma.format/1` | Format Lemma source code (no engine needed) |

## Engine lifecycle

Each `Lemma.new/1` call creates an independent engine. The engine reference is safe to use from a single process. For shared access across processes, wrap it in a GenServer.
