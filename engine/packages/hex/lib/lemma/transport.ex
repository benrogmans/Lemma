defmodule Lemma.Transport do
  @moduledoc """
  Default HTTP transport for `Lemma.install/3`. Uses Req without automatic retries so
  registry failures surface promptly to the install step machine.
  """

  @doc """
  GET `url` with the given request headers. Returns a binary body.
  """
  @spec get(String.t(), [{String.t(), String.t()}]) ::
          {:ok, non_neg_integer(), [{String.t(), String.t()}], String.t()} | {:error, String.t()}
  def get(url, headers) when is_binary(url) and is_list(headers) do
    case Req.get(url,
           headers: headers,
           receive_timeout: 30_000,
           decode_body: false,
           retry: false
         ) do
      {:ok, %{status: status, headers: resp_headers, body: body}} when is_binary(body) ->
        {:ok, status, normalize_headers(resp_headers), body}

      {:ok, %{body: body}} ->
        {:error, "response body was not binary: #{inspect(body)}"}

      {:error, exception} ->
        {:error, Exception.message(exception)}
    end
  end

  defp normalize_headers(headers) when is_list(headers) do
    Enum.flat_map(headers, fn
      {name, value} when is_binary(name) and is_binary(value) -> [{name, value}]
      {name, values} when is_binary(name) and is_list(values) -> Enum.map(values, &{name, &1})
      other -> raise "BUG: unexpected Req header shape: #{inspect(other)}"
    end)
  end

  defp normalize_headers(headers) when is_map(headers) do
    Enum.flat_map(headers, fn
      {name, value} when is_binary(value) -> [{to_string(name), value}]
      {name, values} when is_list(values) -> Enum.map(values, &{to_string(name), &1})
      other -> raise "BUG: unexpected Req header shape: #{inspect(other)}"
    end)
  end
end
