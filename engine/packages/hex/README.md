# Lemma

Elixir client for the [Lemma](https://github.com/benrogmans/lemma) rules engine, via Rustler NIFs.

## Requirements

- Elixir >= 1.14
- Rust toolchain (stable) — Rustler compiles the NIF from source

## Installation

Add to `mix.exs`:

```elixir
def deps do
  [
    {:lemma, "~> 0.8"}
  ]
end
```

Or from git:

```elixir
{:lemma, git: "https://github.com/benrogmans/lemma", sparse: "engine/packages/hex"}
```

## Usage

```elixir
# Create an engine
{:ok, engine} = Lemma.new()

# Load a spec
:ok = Lemma.load(engine, """
spec pricing
fact quantity: [number]
fact price: 10
rule total: quantity * price
rule discount: 0
  unless quantity >= 10 then 5
  unless quantity >= 50 then 15
""")

# Run with facts
{:ok, response} = Lemma.run(engine, "pricing", facts: %{"quantity" => "25"})

# Introspect
{:ok, specs} = Lemma.list(engine)
{:ok, schema} = Lemma.schema(engine, "pricing")

# Format source code (no engine needed)
{:ok, formatted} = Lemma.format("spec foo\nfact x: 1\nrule y: x + 1")

# Clean up
:ok = Lemma.remove_spec(engine, "pricing", "2025-01-01")
```

## API

| Function | Description |
|----------|-------------|
| `Lemma.new/1` | Create engine (optional limits map) |
| `Lemma.load/3` | Load spec from string |
| `Lemma.load_from_paths/2` | Load specs from file paths |
| `Lemma.list/1` | List loaded specs |
| `Lemma.schema/3` | Get spec schema (facts, rules, types) |
| `Lemma.run/3` | Evaluate a spec with facts |
| `Lemma.remove_spec/3` | Remove a spec from the engine |
| `Lemma.format/1` | Format Lemma source code (no engine needed) |

See `Lemma` module docs for full typespecs and options.

## Engine lifecycle

Each `Lemma.new/1` call creates an independent engine. The engine reference is safe to use from a single process. For shared access across processes, wrap it in a GenServer.

## License

Apache-2.0
