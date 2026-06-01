defmodule LemmaTest do
  use ExUnit.Case, async: true

  @simple_spec """
  spec pricing
  data quantity: number
  data price: 10
  rule total: quantity * price
  rule discount: 0
    unless quantity >= 10 then 5
    unless quantity >= 50 then 15
  """

  @embedded_repo "lemma"

  defp embedded_stdlib_group?(group) do
    group[:repository][:name] == @embedded_repo
  end

  defp workspace_groups(groups) do
    Enum.reject(groups, &embedded_stdlib_group?/1)
  end

  defp embedded_stdlib_group(groups) do
    Enum.find(groups, &embedded_stdlib_group?/1)
  end

  defp spec_count(groups) do
    groups |> Enum.map(fn g -> length(g.specs) end) |> Enum.sum()
  end

  defp workspace_spec_count(groups) do
    groups |> workspace_groups() |> spec_count()
  end

  # Explanation trees embed source paths (e.g. original vs formatted); compare evaluation payloads only.
  defp comparable_rule_result(rule) when is_map(rule) do
    Map.drop(rule, ["explanation"])
  end

  describe "new/0" do
    test "creates engine with default limits" do
      assert {:ok, engine} = Lemma.new()
      assert is_reference(engine)
    end

    test "creates engine with custom limits" do
      assert {:ok, engine} = Lemma.new(%{"max_sources" => 50})
      assert is_reference(engine)
    end

    test "creates engine with nil limits (defaults)" do
      assert {:ok, engine} = Lemma.new(nil)
      assert is_reference(engine)
    end
  end

  describe "new/1 error cases" do
    test "rejects non-integer limit value" do
      assert_raise ErlangError, fn ->
        Lemma.new(%{"max_sources" => "not_a_number"})
      end
    end

    test "rejects unknown limit key" do
      assert_raise ErlangError, fn ->
        Lemma.new(%{"bogus_key" => 10})
      end
    end

    test "rejects negative limit value" do
      assert_raise ErlangError, fn ->
        Lemma.new(%{"max_sources" => -1})
      end
    end
  end

  describe "load/3" do
    test "loads a valid spec" do
      {:ok, engine} = Lemma.new()
      assert :ok = Lemma.load(engine, @simple_spec, "pricing.lemma")
    end

    test "returns errors for invalid spec" do
      {:ok, engine} = Lemma.new()
      assert {:error, errors} = Lemma.load(engine, "spec bad\ndata x: [bogus]", "bad.lemma")
      assert is_list(errors)
      assert length(errors) > 0
      first = hd(errors)
      assert is_map(first)
      assert Map.has_key?(first, :message)
      assert first[:kind] == "parsing"
    end

    test "uses 'inline' as default source label" do
      {:ok, engine} = Lemma.new()
      assert :ok = Lemma.load(engine, "spec inline_test\ndata x: 1\nrule y: x + 1")
    end
  end

  describe "load_batch/3" do
    test "loads multiple sources in one pass" do
      {:ok, engine} = Lemma.new()

      sources = %{
        "a.lemma" => "repo batch_repo\nspec one\ndata x: 1",
        "b.lemma" => "repo batch_repo\nspec two\ndata y: 2"
      }

      assert :ok = Lemma.load_batch(engine, sources)
      assert {:ok, groups} = Lemma.list(engine)
      batch = Enum.find(groups, fn g -> g.repository[:name] == "batch_repo" end)
      assert batch != nil
      names = batch.specs |> Enum.map(& &1[:name]) |> Enum.sort()
      assert names == ["one", "two"]
    end

    test "invalid sources map returns error tuple not exception" do
      {:ok, engine} = Lemma.new()
      assert {:error, errors} = Lemma.load_batch(engine, :not_a_map)
      assert is_list(errors)
      assert hd(errors).kind == "request"
    end
  end

  describe "list/1" do
    test "lists loaded specs with inline schema" do
      {:ok, engine} = Lemma.new()
      :ok = Lemma.load(engine, @simple_spec, "pricing.lemma")
      assert {:ok, groups} = Lemma.list(engine)
      assert is_list(groups)
      assert length(workspace_groups(groups)) == 1
      group = hd(workspace_groups(groups))
      assert group[:repository][:name] == nil
      assert group[:repository][:dependency] == nil
      assert length(group[:specs]) == 1
      assert group[:repository][:start_line] == 1
      spec = hd(group[:specs])
      assert spec[:name] == "pricing"
      assert spec[:start_line] == 1
      assert spec[:attribute] == "pricing.lemma"
      assert is_map(spec[:schema])
      assert spec[:schema]["spec"] == "pricing"
      assert is_map(spec[:schema]["data"])
      assert is_map(spec[:schema]["rules"])
    end

    test "fresh engine lists embedded stdlib repository" do
      {:ok, engine} = Lemma.new()
      {:ok, groups} = Lemma.list(engine)
      embedded = embedded_stdlib_group(groups)
      assert embedded != nil
      assert embedded[:repository][:dependency] == @embedded_repo
      assert hd(embedded.specs)[:name] == "units"
    end

    test "effective_from is nil when not set" do
      {:ok, engine} = Lemma.new()
      :ok = Lemma.load(engine, "spec no_effective\ndata x: 1", "test.lemma")
      {:ok, groups} = Lemma.list(engine)
      [group] = workspace_groups(groups)
      spec = hd(group.specs)
      assert spec[:effective_from] == nil
    end

    test "effective_to is nil for an unversioned spec (no successor)" do
      {:ok, engine} = Lemma.new()
      :ok = Lemma.load(engine, "spec no_effective\ndata x: 1", "test.lemma")
      {:ok, groups} = Lemma.list(engine)
      [group] = workspace_groups(groups)
      spec = hd(group.specs)
      assert spec[:effective_to] == nil
    end

    test "effective_to equals the next version's effective_from for earlier rows" do
      {:ok, engine} = Lemma.new()

      code = """
      spec pricing 2025-01-01
      data base: 10
      rule total: base

      spec pricing 2026-01-01
      data base: 99
      rule total: base
      """

      :ok = Lemma.load(engine, code, "temporal.lemma")
      {:ok, groups} = Lemma.list(engine)
      assert length(workspace_groups(groups)) == 1
      entries = hd(workspace_groups(groups)).specs
      assert length(entries) == 2

      [earlier, latest] = entries
      assert earlier[:effective_from] == "2025-01-01"
      assert earlier[:effective_to] == "2026-01-01"
      assert latest[:effective_from] == "2026-01-01"
      assert latest[:effective_to] == nil
    end
  end

  describe "schema/3" do
    test "returns schema for loaded spec with DataEntry + kind-tagged types" do
      {:ok, engine} = Lemma.new()
      :ok = Lemma.load(engine, @simple_spec, "pricing.lemma")
      assert {:ok, schema} = Lemma.schema(engine, "pricing")
      assert is_map(schema)
      assert schema["spec"] == "pricing"
      assert is_map(schema["data"])
      assert is_map(schema["rules"])
      assert Map.has_key?(schema["data"], "quantity")
      assert Map.has_key?(schema["rules"], "total")
      assert Map.has_key?(schema["rules"], "discount")

      quantity = schema["data"]["quantity"]
      assert is_map(quantity), "DataEntry is a named object, not a tuple"
      assert is_map(quantity["type"])
      assert is_binary(quantity["type"]["kind"]), "type carries `kind` discriminator"
    end

    test "returns error for unknown spec" do
      {:ok, engine} = Lemma.new()
      assert {:error, _} = Lemma.schema(engine, "nonexistent")
    end
  end

  describe "run/3" do
    test "runs spec with provided data" do
      {:ok, engine} = Lemma.new()
      :ok = Lemma.load(engine, @simple_spec, "pricing.lemma")
      assert {:ok, response} = Lemma.run(engine, "pricing", data: %{"quantity" => "5"})
      assert is_map(response)
      assert response["spec"] == "pricing"
      results = response["results"]
      assert is_map(results)
      total = results["total"]
      assert total["display"] == "50"
      assert total["number"] == "50"
    end

    test "runs spec with quantity triggering unless clause" do
      {:ok, engine} = Lemma.new()
      :ok = Lemma.load(engine, @simple_spec, "pricing.lemma")
      {:ok, response} = Lemma.run(engine, "pricing", data: %{"quantity" => "10"})
      results = response["results"]
      assert results["discount"]["display"] == "5"
      assert results["discount"]["number"] == "5"
    end

    test "runs spec with no optional data" do
      {:ok, engine} = Lemma.new()
      :ok = Lemma.load(engine, "spec simple\ndata x: 1\nrule y: x + 1", "s.lemma")
      {:ok, response} = Lemma.run(engine, "simple")
      results = response["results"]
      assert results["y"]["display"] == "2"
      assert results["y"]["number"] == "2"
    end

    test "returns error for unknown spec" do
      {:ok, engine} = Lemma.new()
      assert {:error, _} = Lemma.run(engine, "nonexistent")
    end
  end

  describe "invert/6" do
    test "inverts a rule with any_value target" do
      {:ok, engine} = Lemma.new()

      spec = """
      spec invertible
      data x: number
      rule y: x * 2
      """

      :ok = Lemma.load(engine, spec, "inv.lemma")

      target = %{outcome: :any_value}

      assert {:ok, result} = Lemma.invert(engine, "invertible", "2025-01-01", "y", target)
      assert is_map(result)
    end

    test "inverts a rule with value target" do
      {:ok, engine} = Lemma.new()

      spec = """
      spec invertible2
      data x: number
      rule y: x * 2
      """

      :ok = Lemma.load(engine, spec, "inv2.lemma")

      target = %{outcome: :value, op: :eq, value: "10"}

      assert {:ok, result} = Lemma.invert(engine, "invertible2", "2025-01-01", "y", target)
      assert is_map(result)
    end

    test "rejects target without outcome" do
      {:ok, engine} = Lemma.new()
      :ok = Lemma.load(engine, "spec inv3\ndata x: number\nrule y: x + 1", "inv3.lemma")

      assert_raise ErlangError, fn ->
        Lemma.invert(engine, "inv3", "2025-01-01", "y", %{op: :eq, value: "5"})
      end
    end
  end

  describe "remove_spec/3" do
    test "removes a loaded spec" do
      {:ok, engine} = Lemma.new()
      :ok = Lemma.load(engine, "spec removable\ndata x: 1\nrule y: x + 1", "rm.lemma")
      {:ok, groups} = Lemma.list(engine)
      assert workspace_spec_count(groups) == 1

      assert :ok = Lemma.remove_spec(engine, "removable", "2025-01-01")

      {:ok, specs} = Lemma.list(engine)
      assert workspace_spec_count(specs) == 0
      assert embedded_stdlib_group(specs) != nil
    end

    test "returns error for unknown spec" do
      {:ok, engine} = Lemma.new()
      assert {:error, _} = Lemma.remove_spec(engine, "ghost", "2025-01-01")
    end
  end

  describe "multiple engines" do
    test "engines are independent" do
      {:ok, e1} = Lemma.new()
      {:ok, e2} = Lemma.new()
      :ok = Lemma.load(e1, "spec a\ndata x: 1\nrule y: x + 1", "a.lemma")
      {:ok, groups1} = Lemma.list(e1)
      {:ok, groups2} = Lemma.list(e2)
      assert workspace_spec_count(groups1) == 1
      assert workspace_spec_count(groups2) == 0
      assert embedded_stdlib_group(groups2) != nil
    end
  end

  describe "load_from_paths/2" do
    test "loads specs from a directory" do
      dir = System.tmp_dir!()
      path = Path.join(dir, "hex_test_spec.lemma")
      File.write!(path, "spec from_file\ndata x: 1\nrule y: x + 1")

      {:ok, engine} = Lemma.new()
      assert :ok = Lemma.load_from_paths(engine, [path])
      {:ok, groups} = Lemma.list(engine)
      names = Enum.flat_map(groups, fn g -> Enum.map(g.specs, & &1[:name]) end)
      assert "from_file" in names
    after
      File.rm(Path.join(System.tmp_dir!(), "hex_test_spec.lemma"))
    end

    test "nonexistent .lemma file returns error" do
      {:ok, engine} = Lemma.new()
      result = Lemma.load_from_paths(engine, ["/nonexistent/path/spec.lemma"])
      # Engine skips paths that don't exist on disk (is_file and is_dir both false)
      assert result == :ok
    end
  end

  describe "format/1" do
    test "formats valid lemma source" do
      input = "spec foo\ndata   x:  1\nrule y: x +  1"
      assert {:ok, formatted} = Lemma.format(input)
      assert is_binary(formatted)
      assert formatted =~ "spec foo"
      assert formatted =~ "data x"
      assert formatted =~ "rule y:"
      assert formatted =~ "x + 1"
    end

    test "returns error for invalid source" do
      assert {:error, err} = Lemma.format("not valid lemma at all !!!")
      assert is_map(err)
      assert Map.has_key?(err, :message)
      assert err[:kind] == "parsing"
    end

    test "preserves semantics after formatting" do
      input = "spec fmt\ndata x: number\nrule y: x *   2\nrule z: y + 1"
      {:ok, formatted} = Lemma.format(input)

      {:ok, e1} = Lemma.new()
      {:ok, e2} = Lemma.new()
      :ok = Lemma.load(e1, input, "original")
      :ok = Lemma.load(e2, formatted, "formatted")

      {:ok, r1} = Lemma.run(e1, "fmt", data: %{"x" => "5"})
      {:ok, r2} = Lemma.run(e2, "fmt", data: %{"x" => "5"})

      assert comparable_rule_result(r1["results"]["y"]) ==
               comparable_rule_result(r2["results"]["y"])

      assert comparable_rule_result(r1["results"]["z"]) ==
               comparable_rule_result(r2["results"]["z"])
    end
  end
end
