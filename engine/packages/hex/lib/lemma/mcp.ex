defmodule Lemma.Mcp do
  @moduledoc """
  Pure Lemma MCP tools: engine + JSON args in, catalog or tool result text out.

  Read tools mirror the engine catalog: `run`, `list`, `show`, `source`,
  `check`, `guide`. Admin/write tools stay in the CLI MCP server only.

  `run/2` always includes explanations and returns Engine `Response` JSON
  (same shape as `Lemma.run/3` with `explain: true`). Args match SDK `run`:
  `spec`, optional `repository`, `rules`, `data` (object), `effective`.
  Do not pass `explain`.
  """

  @type engine :: Lemma.engine()
  @type args :: map()

  @spec list_tools() :: {:ok, [map()]} | {:error, term()}
  def list_tools do
    case Lemma.Native.mcp_list_tools() do
      {:ok, binary} -> {:ok, Jason.decode!(binary)}
      err -> err
    end
  end

  @spec list_resources() :: {:ok, [map()]} | {:error, term()}
  def list_resources do
    case Lemma.Native.mcp_list_resources() do
      {:ok, binary} -> {:ok, Jason.decode!(binary)}
      err -> err
    end
  end

  @spec read_resource(String.t()) :: {:ok, String.t()} | {:error, :unknown_uri, String.t()}
  def read_resource(uri) when is_binary(uri) do
    Lemma.Native.mcp_read_resource(uri)
  end

  @spec run(engine(), args()) ::
          {:ok, String.t()}
          | {:error, :invalid_arguments | :not_found | :diagnostics, String.t()}
  def run(engine, args) when is_map(args) do
    Lemma.Native.mcp_run(engine, Jason.encode!(args))
  end

  @doc """
  Deprecated alias of `run/2`. Prefer `Lemma.Mcp.run/2`.
  """
  @spec evaluate(engine(), args()) ::
          {:ok, String.t()}
          | {:error, :invalid_arguments | :not_found | :diagnostics, String.t()}
  def evaluate(engine, args) when is_map(args) do
    run(engine, args)
  end

  @spec list(engine(), args()) ::
          {:ok, String.t()}
          | {:error, :invalid_arguments | :not_found | :diagnostics, String.t()}
  def list(engine, args) when is_map(args) do
    Lemma.Native.mcp_list(engine, Jason.encode!(args))
  end

  @spec show(engine(), args()) ::
          {:ok, String.t()}
          | {:error, :invalid_arguments | :not_found | :diagnostics, String.t()}
  def show(engine, args) when is_map(args) do
    Lemma.Native.mcp_show(engine, Jason.encode!(args))
  end

  @spec source(engine(), args()) ::
          {:ok, String.t()}
          | {:error, :invalid_arguments | :not_found | :diagnostics, String.t()}
  def source(engine, args) when is_map(args) do
    Lemma.Native.mcp_source(engine, Jason.encode!(args))
  end

  @spec check(args()) ::
          {:ok, String.t()}
          | {:error, :invalid_arguments | :not_found | :diagnostics, String.t()}
  def check(args) when is_map(args) do
    Lemma.Native.mcp_check(Jason.encode!(args))
  end

  @spec guide(args()) ::
          {:ok, String.t()}
          | {:error, :invalid_arguments | :not_found | :diagnostics, String.t()}
  def guide(args) when is_map(args) do
    Lemma.Native.mcp_guide(Jason.encode!(args))
  end
end
