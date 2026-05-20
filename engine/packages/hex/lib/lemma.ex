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

  - `max_sources` - max sources per load_from_paths (after expanding paths)
  - `max_loaded_bytes` - max total bytes to load
  - `max_source_size_bytes` - max single source text size in bytes
  - `max_total_expression_count` - max expression nodes
  - `max_expression_depth` - max nesting depth
  - `max_expression_count` - max expressions per source (parser)
  - `max_data_value_bytes` - max data value size

  ## Examples

      {:ok, engine} = Lemma.new()
      {:ok, engine} = Lemma.new(%{max_sources: 100})
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
  Loads multiple Lemma sources in one planning pass.

  `sources` is a map of path label (for errors) to source text. Use `""` as key for volatile/inline.

  `dependency` is optional; when non-empty, repositories in this batch are tagged with that dependency id (same as the Rust `Engine.load_batch/2` second argument).
  """
  @spec load_batch(engine(), %{String.t() => String.t()}, String.t() | nil) ::
          :ok | {:error, [map()]}
  def load_batch(engine, sources, dependency \\ nil) do
    Lemma.Native.lemma_load_batch(engine, sources, dependency)
  end

  @doc """
  Lists loaded specs grouped by repository (same order as the engine:
  workspace first, then dependencies).

  Each element is `%{repository: %{name: ..., dependency: ..., start_line: ..., attribute: ...}, specs: [...]}`
  where `:name` / `:dependency` are strings or `nil` (workspace has `:name` nil),
  `:start_line` is a non-negative integer, and `:attribute` is the load-source label string
  or `nil` (same display as `SourceType` in Rust — path, `volatile`, …).
  Each entry in `:specs` has `:name`, `:effective_from`, `:effective_to`, `:start_line`,
  `:attribute`, and `:schema` (decoded JSON object).

  Temporal versions form a half-open `[effective_from, effective_to)` range:

  - `:effective_from` is `nil` when the spec has no declared start date (the
    first version is unbounded at the start).
  - `:effective_to` is `nil` when the spec has no later version (this row is
    the latest and stays valid forward indefinitely).
  - Otherwise `:effective_to` equals the next version's `:effective_from`
    (exclusive end of this row's validity).

  `:schema` is the decoded [`Lemma.schema/3`] envelope for this version so
  callers never need a second round-trip.

  Always includes the embedded `lemma` repository (`spec si`) on a fresh engine.
  Returns `{:ok, []}` only when no repositories have specs (should not happen on `Lemma.new/1`).
  """
  @spec list(engine()) :: {:ok, [map()]} | {:error, term()}
  def list(engine) do
    case Lemma.Native.lemma_list(engine) do
      {:ok, groups} ->
        {:ok,
         Enum.map(groups, fn group ->
           specs =
             Enum.map(group.specs, fn item ->
               Map.update!(item, :schema, &Jason.decode!/1)
             end)

           %{repository: group.repository, specs: specs}
         end)}

      err ->
        err
    end
  end

  @doc """
  Returns canonical Lemma source for a loaded repository (formatted from the in-engine AST).

  Use `"lemma"` for the embedded SI stdlib (`spec si`).
  """
  @spec format_repository(engine(), String.t()) :: {:ok, String.t()} | {:error, term()}
  def format_repository(engine, repository) when is_binary(repository) do
    Lemma.Native.lemma_format_repository(engine, repository)
  end

  @doc """
  Returns the schema for a spec.

  Options: `:effective` (datetime string or nil).
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
  Returns the serialized execution plan for a spec as a map.

  Options: `:effective` (datetime string or nil).
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
  Runs a spec. Options: `:effective` (datetime string or nil), `:data` (map).

  Returns decoded JSON response.
  """
  @spec run(engine(), spec_name(), keyword()) :: {:ok, map()} | {:error, term()}
  def run(engine, spec, opts \\ []) do
    effective = Keyword.get(opts, :effective)
    data = Keyword.get(opts, :data, %{})

    case Lemma.Native.lemma_run(engine, spec, effective, data) do
      {:ok, binary} -> {:ok, Jason.decode!(binary)}
      err -> err
    end
  end

  @doc """
  Inverts a rule to find input domains that produce a desired outcome.

  `effective` is a datetime string or nil.

  Target is a map with `:outcome` ("value" | "veto" | "any_value" | "any_veto"),
  optionally `:op` ("eq" | "neq" | "lt" | etc.), and for "value"/"veto": `:value` or `:message`.
  """
  @spec invert(engine(), spec_name(), String.t() | nil, String.t(), map(), map()) ::
          {:ok, map()} | {:error, term()}
  def invert(engine, spec_name, effective, rule_name, target, values \\ %{}) do
    case Lemma.Native.lemma_invert(
           engine,
           spec_name,
           effective,
           rule_name,
           target,
           values
         ) do
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
  Loaded repositories (workspace and dependencies). Each map has string keys `"name"` and `"dependency"`.
  """
  @spec repositories(engine()) :: {:ok, [map()]} | {:error, term()}
  def repositories(engine) do
    case Lemma.Native.lemma_repositories(engine) do
      {:ok, binary} -> {:ok, Jason.decode!(binary)}
      err -> err
    end
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
