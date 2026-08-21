defmodule Lemma.Mcp do
  @moduledoc """
  Pure Lemma MCP tools: engine + arguments in, catalog or text out.
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

  @spec evaluate(engine(), args()) ::
          {:ok, String.t()}
          | {:error, :invalid_arguments | :not_found | :diagnostics, String.t()}
  def evaluate(engine, args) when is_map(args) do
    Lemma.Native.mcp_evaluate(engine, Jason.encode!(args))
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
