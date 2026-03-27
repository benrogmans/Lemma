defmodule Lemma.MixProject do
  use Mix.Project

  @version "0.8.5"
  @source_url "https://github.com/benrogmans/lemma"

  def project do
    [
      app: :lemma,
      version: @version,
      elixir: "~> 1.14",
      compilers: Mix.compilers(),
      start_permanent: Mix.env() == :prod,
      aliases: aliases(),
      deps: deps(),
      description: "Lemma rules engine for Elixir",
      package: package(),
      docs: docs()
    ]
  end

  def application do
    []
  end

  defp aliases do
    [
      precommit: ["format --check-formatted", "deps.get --check-locked", "compile"]
    ]
  end

  defp deps do
    [
      {:jason, "~> 1.4"},
      {:rustler, "~> 0.37", runtime: false},
      {:ex_doc, "~> 0.40.1", only: :dev, runtime: false}
    ]
  end

  defp package do
    [
      files: ["lib", "native", "mix.exs", "README.md"],
      licenses: ["Apache-2.0"],
      links: %{"GitHub" => @source_url}
    ]
  end

  defp docs do
    [
      main: "Lemma",
      source_url: @source_url,
      extras: ["README.md"]
    ]
  end
end
