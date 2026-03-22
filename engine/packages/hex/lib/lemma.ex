defmodule Lemma do
  @moduledoc """
  Lemma rules engine for Elixir.

  Wraps the Lemma engine (Rust) via NIFs. Create an engine, load specs from
  string or paths, run evaluations, and introspect schemas.

  ## Example

      {:ok, engine} = Lemma.new()
      :ok = Lemma.load(engine, "spec foo\\nfact x: 1\\nrule y: x + 1", "my_spec.lemma")
      {:ok, response} = Lemma.run(engine, "foo", [])
      # response is a map from decoded JSON

  ## Engine lifecycle

  Each engine is an opaque resource. Do not share the same engine ref across
  processes unless you serialize access (e.g. via a GenServer).
  """

  @type engine :: reference()
  @type spec_name :: String.t()
  @type limits_map :: %{String.t() => pos_integer()} | nil

  @doc """
  Creates a new engine. Optionally pass a map of resource limits; omitted keys use defaults.

  ## Options (limits map keys)

  - `max_files` - max .lemma files per load_from_paths
  - `max_loaded_bytes` - max total bytes to load
  - `max_file_size_bytes` - max single file size
  - `max_total_expression_count` - max expression nodes
  - `max_expression_depth` - max nesting depth
  - `max_expression_count` - max expressions per file
  - `max_fact_value_bytes` - max fact value size

  ## Examples

      {:ok, engine} = Lemma.new()
      {:ok, engine} = Lemma.new(%{max_files: 100})
  """
  @spec new(limits_map) :: {:ok, engine()} | {:error, term()}
  def new(limits \\ nil) do
    Lemma.Native.lemma_new(limits)
  end

  @doc """
  Loads a spec from a string. Source label is used for error reporting (e.g. "my_spec.lemma").
  Use "inline" when no path.
  """
  @spec load(engine(), String.t(), String.t()) :: :ok | {:error, [map()]}
  def load(engine, code, source_label \\ "inline") do
    Lemma.Native.lemma_load(engine, code, source_label)
  end

  @doc """
  Loads specs from paths (files and/or directories). Directories are expanded one level;
  only .lemma files are loaded.
  """
  @spec load_from_paths(engine(), [String.t()]) :: :ok | {:error, [map()]}
  def load_from_paths(engine, paths) do
    Lemma.Native.lemma_load_from_paths(engine, paths)
  end

  @doc """
  Lists all loaded specs. Each item is a map with `:name` and `:effective_from`.
  """
  @spec list(engine()) :: {:ok, [map()]} | {:error, term()}
  def list(engine) do
    Lemma.Native.lemma_list(engine)
  end

  @doc """
  Returns the schema for a spec. Accepts spec name or "name~hash". Options: `:effective` (datetime string or nil).
  """
  @spec schema(engine(), spec_name(), keyword()) :: {:ok, map()} | {:error, term()}
  def schema(engine, spec, opts \\ []) do
    effective = Keyword.get(opts, :effective)
    case Lemma.Native.lemma_schema(engine, spec, effective) do
      {:ok, binary} -> {:ok, Jason.decode!(binary)}
      err -> err
    end
  end

  @doc """
  Returns the serialized execution plan for a spec as a map. Options: `:effective` (datetime string or nil).
  """
  @spec execution_plan(engine(), spec_name(), keyword()) :: {:ok, map()} | {:error, term()}
  def execution_plan(engine, spec, opts \\ []) do
    effective = Keyword.get(opts, :effective)
    case Lemma.Native.lemma_execution_plan(engine, spec, effective) do
      {:ok, binary} -> {:ok, Jason.decode!(binary)}
      err -> err
    end
  end

  @doc """
  Runs a spec. Options: `:effective` (datetime string), `:facts` (map of string keys/values).
  Returns decoded JSON response.
  """
  @spec run(engine(), spec_name(), keyword()) :: {:ok, map()} | {:error, term()}
  def run(engine, spec, opts \\ []) do
    effective = Keyword.get(opts, :effective)
    facts = Keyword.get(opts, :facts, %{})
    case Lemma.Native.lemma_run(engine, spec, effective, facts) do
      {:ok, binary} -> {:ok, Jason.decode!(binary)}
      err -> err
    end
  end

  @doc """
  Inverts a rule to find input domains that produce a desired outcome.

  Target is a map with `:outcome` ("value" | "veto" | "any_value" | "any_veto"),
  optionally `:op` ("eq" | "neq" | "lt" | etc.), and for "value"/"veto": `:value` or `:message`.
  """
  @spec invert(engine(), spec_name(), String.t(), String.t(), map(), map()) :: {:ok, map()} | {:error, term()}
  def invert(engine, spec_name, effective, rule_name, target, values \\ %{}) do
    case Lemma.Native.lemma_invert(engine, spec_name, effective, rule_name, target, values) do
      {:ok, binary} -> {:ok, Jason.decode!(binary)}
      err -> err
    end
  end

  @doc """
  Removes a spec from the engine by name and effective datetime.
  """
  @spec remove_spec(engine(), spec_name(), String.t()) :: :ok | {:error, term()}
  def remove_spec(engine, spec_name, effective) do
    Lemma.Native.lemma_remove_spec(engine, spec_name, effective)
  end

  @doc """
  Formats Lemma source code. Does not require an engine instance.

  ## Example

      {:ok, formatted} = Lemma.format("spec foo\\nfact   x:  1\\nrule y: x +  1")
  """
  @spec format(String.t()) :: {:ok, String.t()} | {:error, term()}
  def format(code) do
    Lemma.Native.lemma_format(code)
  end
end
