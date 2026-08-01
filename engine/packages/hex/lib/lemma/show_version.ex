defmodule Lemma.ShowVersion do
  @moduledoc """
  Half-open `[effective_from, effective_to)` for one loaded temporal row, as returned
  in `Lemma.Show.versions`.
  """

  @type t :: %__MODULE__{
          effective_from: String.t() | nil,
          effective_to: String.t() | nil
        }

  defstruct effective_from: nil, effective_to: nil

  @doc """
  Builds a `Lemma.ShowVersion` from one entry of the decoded `Lemma.show/4` JSON
  `"versions"` array. Both fields are absent (not `null`) from the API JSON when
  unbounded; `Map.get/3` yields `nil` for either case.
  """
  @spec from_map(map()) :: t()
  def from_map(map) when is_map(map) do
    %__MODULE__{
      effective_from: Map.get(map, "effective_from"),
      effective_to: Map.get(map, "effective_to")
    }
  end
end
