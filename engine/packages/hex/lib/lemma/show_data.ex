defmodule Lemma.ShowData do
  @moduledoc """
  One input declared in a spec, as returned in `Lemma.Show.data`.

  `type` is the raw `LemmaType` JSON map, a Rust discriminated union tagged by its
  `"kind"` string. It is intentionally left untyped here: pattern-match on it
  directly (e.g. `%{"kind" => "measure", "units" => units} = entry.type`) rather than
  routing through a parallel Elixir struct hierarchy for 12 type kinds.
  """

  @type t :: %__MODULE__{
          type: map(),
          prefilled: map() | nil,
          suggestion: map() | nil,
          needed_by_rules: [String.t()]
        }

  @enforce_keys [:type]
  defstruct type: nil, prefilled: nil, suggestion: nil, needed_by_rules: []

  @doc """
  Builds a `Lemma.ShowData` from one value of the decoded `Lemma.show/4` JSON
  `"data"` map. `prefilled`/`suggestion` are absent (not `null`) from the API JSON
  when unset; `Map.get/3` yields `nil` for both.
  """
  @spec from_map(map()) :: t()
  def from_map(map) when is_map(map) do
    %__MODULE__{
      type: Map.fetch!(map, "type"),
      prefilled: Map.get(map, "prefilled"),
      suggestion: Map.get(map, "suggestion"),
      needed_by_rules: Map.get(map, "needed_by_rules", [])
    }
  end
end
