defmodule Lemma.Native do
  @moduledoc false
  version = Mix.Project.config()[:version]

  use RustlerPrecompiled,
    otp_app: :lemma_engine,
    crate: "lemma_hex",
    base_url: "https://github.com/benrogmans/lemma/releases/download/cli-v#{version}",
    force_build: not File.exists?("checksum-Elixir.Lemma.Native.exs"),
    version: version,
    targets: ~w(
      aarch64-apple-darwin
      x86_64-apple-darwin
      aarch64-unknown-linux-gnu
      x86_64-unknown-linux-gnu
      x86_64-unknown-linux-musl
      x86_64-pc-windows-msvc
    )

  def lemma_new(_limits), do: :erlang.nif_error(:nif_not_loaded)
  def lemma_load(_resource, _code, _source_label), do: :erlang.nif_error(:nif_not_loaded)
  def lemma_load_from_paths(_resource, _paths), do: :erlang.nif_error(:nif_not_loaded)
  def lemma_list(_resource), do: :erlang.nif_error(:nif_not_loaded)
  def lemma_schema(_resource, _spec, _effective_opt), do: :erlang.nif_error(:nif_not_loaded)

  def lemma_execution_plan(_resource, _spec, _effective_opt),
    do: :erlang.nif_error(:nif_not_loaded)

  def lemma_run(_resource, _spec, _effective_opt, _fact_values),
    do: :erlang.nif_error(:nif_not_loaded)

  def lemma_invert(_resource, _spec_name, _effective, _rule_name, _target_term, _values),
    do: :erlang.nif_error(:nif_not_loaded)

  def lemma_remove_spec(_resource, _spec_name, _effective), do: :erlang.nif_error(:nif_not_loaded)
  def lemma_format(_code), do: :erlang.nif_error(:nif_not_loaded)
end
