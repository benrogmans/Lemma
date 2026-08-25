defmodule Lemma.OpenAPI do
  @moduledoc """
  OpenAPI and temporal API docs helpers (`lemma_openapi` crate). Separate from core [`Lemma`] runtime.
  """

  @doc """
  Temporal version options for API docs (same boundaries as `lemma serve` / Scalar).

  Returns a list of maps with string keys `"title"` and `"slug"`. Slug `"now"` means the
  latest interface at request time; other slugs match `effective` strings for `generate_openapi/2`.
  """
  @spec temporal_api_sources(Lemma.engine()) :: {:ok, [map()]} | {:error, term()}
  def temporal_api_sources(engine) do
    case Lemma.Native.lemma_temporal_api_sources(engine) do
      {:ok, binary} -> {:ok, Jason.decode!(binary)}
      err -> err
    end
  end

  @doc """
  Generates an OpenAPI 3.1 JSON document for the loaded specs (Lemma HTTP API shape).

  Options: `:explain` (boolean, default false), `:effective` (datetime string or nil for "now").
  """
  @spec generate_openapi(Lemma.engine(), keyword()) :: {:ok, map()} | {:error, term()}
  def generate_openapi(engine, opts \\ []) do
    explain = Keyword.get(opts, :explain, false)
    effective = Keyword.get(opts, :effective)

    case Lemma.Native.lemma_generate_openapi(engine, explain, effective) do
      {:ok, binary} -> {:ok, Jason.decode!(binary)}
      err -> err
    end
  end
end
