defmodule Lemma.EngineError do
  @moduledoc """
  Structured engine error as returned by NIF error paths (`load`, `show`, `run`,
  `format`, …). Keys are atoms (Rustler encoding), not JSON strings.

  Call `from_map/1` on an error map (or each element of a load-error list) to get
  a typed struct.
  """

  @type source :: %{
          attribute: String.t(),
          line: non_neg_integer(),
          column: non_neg_integer(),
          length: non_neg_integer()
        }

  @type t :: %__MODULE__{
          kind: String.t(),
          message: String.t(),
          source: source() | nil,
          suggestion: String.t() | nil,
          repository: String.t() | nil,
          related_data: String.t() | nil,
          spec: String.t() | nil,
          related_spec: String.t() | nil,
          registry_kind: String.t() | nil,
          request_kind: String.t() | nil,
          limit_name: String.t() | nil,
          limit_value: String.t() | nil,
          actual_value: String.t() | nil
        }

  @enforce_keys [:kind, :message]
  defstruct [
    :kind,
    :message,
    :source,
    :suggestion,
    :repository,
    :related_data,
    :spec,
    :related_spec,
    :registry_kind,
    :request_kind,
    :limit_name,
    :limit_value,
    :actual_value
  ]

  @doc """
  Builds a `Lemma.EngineError` from an atom-keyed error map returned by a NIF.

  Raises `KeyError` if `:kind` or `:message` is missing.
  """
  @spec from_map(map()) :: t()
  def from_map(map) when is_map(map) do
    %__MODULE__{
      kind: Map.fetch!(map, :kind),
      message: Map.fetch!(map, :message),
      source: source_from_map(Map.get(map, :source)),
      suggestion: Map.get(map, :suggestion),
      repository: Map.get(map, :repository),
      related_data: Map.get(map, :related_data),
      spec: Map.get(map, :spec),
      related_spec: Map.get(map, :related_spec),
      registry_kind: Map.get(map, :registry_kind),
      request_kind: Map.get(map, :request_kind),
      limit_name: Map.get(map, :limit_name),
      limit_value: Map.get(map, :limit_value),
      actual_value: Map.get(map, :actual_value)
    }
  end

  defp source_from_map(nil), do: nil

  defp source_from_map(source) when is_map(source) do
    %{
      attribute: Map.fetch!(source, :attribute),
      line: Map.fetch!(source, :line),
      column: Map.fetch!(source, :column),
      length: Map.fetch!(source, :length)
    }
  end
end
