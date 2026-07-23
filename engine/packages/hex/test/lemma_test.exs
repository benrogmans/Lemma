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
    group["repository"] == @embedded_repo
  end

  defp workspace_groups(groups) do
    Enum.reject(groups, &embedded_stdlib_group?/1)
  end

  defp embedded_stdlib_group(groups) do
    Enum.find(groups, &embedded_stdlib_group?/1)
  end

  defp spec_count(groups) do
    groups |> Enum.map(fn g -> length(g["specs"]) end) |> Enum.sum()
  end

  defp workspace_spec_count(groups) do
    groups |> workspace_groups() |> spec_count()
  end

  defp date_iso(nil), do: nil

  defp date_iso(%{"year" => year, "month" => month, "day" => day}) do
    :io_lib.format("~4..0w-~2..0w-~2..0w", [year, month, day]) |> List.to_string()
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

  describe "new/1 max_normalized_expression_nodes" do
    test "accepts max_normalized_expression_nodes limit" do
      assert {:ok, engine} = Lemma.new(%{"max_normalized_expression_nodes" => 1000})
      assert is_reference(engine)
    end

    test "enforces max_normalized_expression_nodes during planning" do
      # Wide unless over distinct data — many unique NormalForm cells, no Rule-overlay
      # sharing. Shared self-doubling chains stay linear and no longer trip this limit.
      arm_count = 40

      data =
        Enum.map_join(0..(arm_count - 1), "\n", fn i -> "data d#{i}: boolean" end)

      arms =
        Enum.map_join(0..(arm_count - 1), "\n", fn i -> "  unless d#{i} then #{i}" end)

      blowup = """
      spec blowup
      #{data}
      rule r: 0
      #{arms}
      """

      {:ok, engine} = Lemma.new(%{"max_normalized_expression_nodes" => 50})
      result = Lemma.load(engine, %{"blowup.lemma" => blowup})
      assert {:error, errors} = result
      assert is_list(errors)

      error = hd(errors)
      assert error[:kind] == "resource_limit"
      assert error[:message] =~ "expression nodes" or error[:message] =~ "normal-form"
    end
  end

  describe "load/2 binary" do
    test "loads inline volatile source" do
      {:ok, engine} = Lemma.new()
      assert :ok = Lemma.load(engine, @simple_spec)
    end

    test "returns errors for invalid inline source" do
      {:ok, engine} = Lemma.new()
      assert {:error, errors} = Lemma.load(engine, "spec bad\ndata x: [bogus]")
      assert is_list(errors)
      assert length(errors) > 0
      first = hd(errors)
      assert is_map(first)
      assert Map.has_key?(first, :message)
      assert first[:kind] == "parsing"
    end
  end

  describe "load/2 labeled" do
    test "loads a labeled spec from a map" do
      {:ok, engine} = Lemma.new()
      assert :ok = Lemma.load(engine, %{"pricing.lemma" => @simple_spec})
    end

    test "loads from a list of label-code tuples" do
      {:ok, engine} = Lemma.new()

      assert :ok =
               Lemma.load(engine, [
                 {"pricing.lemma", @simple_spec}
               ])
    end

    test "path label volatile loads as Path not Volatile" do
      {:ok, engine} = Lemma.new()

      assert :ok =
               Lemma.load(engine, %{
                 "volatile" => "spec inline_test\ndata x: 1\nrule y: x + 1"
               })

      {:ok, show} = Lemma.show(engine, nil, "inline_test")
      assert show["source_type"] == %{"path" => "volatile"}
    end

    test "rejects empty source label" do
      {:ok, engine} = Lemma.new()

      assert {:error, errors} =
               Lemma.load(engine, %{
                 "" => "spec inline_test\ndata x: 1\nrule y: x + 1"
               })

      assert hd(errors)[:kind] == "request"
    end
  end

  describe "list/1" do
    test "lists loaded specs with metadata" do
      {:ok, engine} = Lemma.new()
      :ok = Lemma.load(engine, %{"pricing.lemma" => @simple_spec})
      assert {:ok, groups} = Lemma.list(engine)
      assert is_list(groups)
      assert length(workspace_groups(groups)) == 1
      group = hd(workspace_groups(groups))
      assert group["repository"] == nil
      assert length(group["specs"]) == 1
      spec = hd(group["specs"])
      assert spec["name"] == "pricing"
      refute Map.has_key?(spec, "start_line")
      refute Map.has_key?(spec, "source_type")

      {:ok, show} = Lemma.show(engine, nil, "pricing")
      assert show["start_line"] == 1
      assert show["source_type"] == %{"path" => "pricing.lemma"}
    end

    test "fresh engine lists embedded stdlib repository" do
      {:ok, engine} = Lemma.new()
      {:ok, groups} = Lemma.list(engine)
      embedded = embedded_stdlib_group(groups)
      assert embedded != nil
      assert embedded["repository"] == @embedded_repo
      assert hd(embedded["specs"])["name"] == "units"
    end

    test "effective_from is nil when not set" do
      {:ok, engine} = Lemma.new()
      :ok = Lemma.load(engine, %{"test.lemma" => "spec no_effective\ndata x: 1"})
      {:ok, groups} = Lemma.list(engine)
      [group] = workspace_groups(groups)
      spec = hd(group["specs"])
      assert spec["effective_from"] == nil
    end

    test "effective_to is nil for an unversioned spec (no successor)" do
      {:ok, engine} = Lemma.new()
      :ok = Lemma.load(engine, %{"test.lemma" => "spec no_effective\ndata x: 1"})
      {:ok, groups} = Lemma.list(engine)
      [group] = workspace_groups(groups)
      spec = hd(group["specs"])
      assert spec["effective_to"] == nil
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

      :ok = Lemma.load(engine, %{"temporal.lemma" => code})
      {:ok, groups} = Lemma.list(engine)
      assert length(workspace_groups(groups)) == 1
      entries = hd(workspace_groups(groups))["specs"]
      assert length(entries) == 2

      [earlier, latest] = entries
      assert date_iso(earlier["effective_from"]) == "2025-01-01"
      assert date_iso(earlier["effective_to"]) == "2026-01-01"
      assert date_iso(latest["effective_from"]) == "2026-01-01"
      assert latest["effective_to"] == nil
    end
  end

  describe "source/4" do
    test "returns embedded lemma repo source" do
      {:ok, engine} = Lemma.new()
      assert {:ok, source} = Lemma.source(engine, @embedded_repo, nil, nil)
      assert source =~ "spec units"
      assert source =~ "trait duration"
    end

    test "nil repository returns workspace source after load" do
      {:ok, engine} = Lemma.new()
      :ok = Lemma.load(engine, %{"ws.lemma" => @simple_spec})
      assert {:ok, source} = Lemma.source(engine, nil, nil, nil)
      assert source =~ "spec pricing"
    end

    test "spec slice source" do
      {:ok, engine} = Lemma.new()
      :ok = Lemma.load(engine, %{"ws.lemma" => @simple_spec})
      assert {:ok, source} = Lemma.source(engine, nil, "pricing", nil)
      assert source =~ "spec pricing"
    end

    test "unknown qualifier returns error" do
      {:ok, engine} = Lemma.new()
      assert {:error, _} = Lemma.source(engine, "workspace", nil, nil)
    end
  end

  describe "show/4" do
    test "returns show for loaded spec with DataEntry + kind-tagged types" do
      {:ok, engine} = Lemma.new()
      :ok = Lemma.load(engine, %{"pricing.lemma" => @simple_spec})
      assert {:ok, show} = Lemma.show(engine, nil, "pricing")
      assert is_map(show)
      assert show["spec"] == "pricing"
      assert is_map(show["data"])
      assert is_map(show["rules"])
      assert Map.has_key?(show["data"], "quantity")
      assert Map.has_key?(show["rules"], "total")
      assert Map.has_key?(show["rules"], "discount")

      quantity = show["data"]["quantity"]
      assert is_map(quantity), "DataEntry is a named object, not a tuple"
      assert is_map(quantity["type"])
      assert is_binary(quantity["type"]["kind"]), "type carries `kind` discriminator"
    end

    test "returns error for unknown spec" do
      {:ok, engine} = Lemma.new()
      assert {:error, _} = Lemma.show(engine, nil, "nonexistent")
    end
  end

  describe "run/3" do
    test "runs spec with provided data" do
      {:ok, engine} = Lemma.new()
      :ok = Lemma.load(engine, %{"pricing.lemma" => @simple_spec})

      assert {:ok, response} =
               Lemma.run(engine, %{spec: "pricing"}, %{data: %{"quantity" => "5"}})

      assert is_map(response)
      assert response["spec"] == "pricing"
      refute Map.has_key?(response, "data")
      results = response["results"]
      assert is_map(results)
      total = results["total"]
      assert total["display"] == "50"
      assert total["number"] == "50"
      refute Map.has_key?(total, "missing_data")
    end

    test "exposes per-rule missing_data when inputs unbound" do
      {:ok, engine} = Lemma.new()
      :ok = Lemma.load(engine, %{"pricing.lemma" => @simple_spec})

      assert {:ok, response} = Lemma.run(engine, %{spec: "pricing"}, %{})

      refute Map.has_key?(response, "data")
      total = response["results"]["total"]
      assert is_list(total["missing_data"])
      assert "quantity" in total["missing_data"]
    end

    test "runs spec with measure triggering unless clause" do
      {:ok, engine} = Lemma.new()
      :ok = Lemma.load(engine, %{"pricing.lemma" => @simple_spec})

      {:ok, response} =
        Lemma.run(engine, %{spec: "pricing"}, %{data: %{"quantity" => "10"}})

      results = response["results"]
      assert results["discount"]["display"] == "5"
      assert results["discount"]["number"] == "5"
    end

    test "runs spec with no optional data" do
      {:ok, engine} = Lemma.new()
      :ok = Lemma.load(engine, %{"s.lemma" => "spec simple\ndata x: 1\nrule y: x + 1"})
      {:ok, response} = Lemma.run(engine, %{spec: "simple"})
      results = response["results"]
      assert results["y"]["display"] == "2"
      assert results["y"]["number"] == "2"
    end

    test "returns error for unknown spec" do
      {:ok, engine} = Lemma.new()
      assert {:error, _} = Lemma.run(engine, %{spec: "nonexistent"})
    end
  end

  describe "remove/3" do
    test "removes a loaded spec" do
      {:ok, engine} = Lemma.new()
      :ok = Lemma.load(engine, %{"rm.lemma" => "spec removable\ndata x: 1\nrule y: x + 1"})
      {:ok, groups} = Lemma.list(engine)
      assert workspace_spec_count(groups) == 1

      assert :ok = Lemma.remove(engine, nil, "removable", "2025-01-01")

      {:ok, specs} = Lemma.list(engine)
      assert workspace_spec_count(specs) == 0
      assert embedded_stdlib_group(specs) != nil
    end

    test "returns error for unknown spec" do
      {:ok, engine} = Lemma.new()
      assert {:error, _} = Lemma.remove(engine, nil, "ghost", "2025-01-01")
    end
  end

  describe "multiple engines" do
    test "engines are independent" do
      {:ok, e1} = Lemma.new()
      {:ok, e2} = Lemma.new()
      :ok = Lemma.load(e1, %{"a.lemma" => "spec a\ndata x: 1\nrule y: x + 1"})
      {:ok, groups1} = Lemma.list(e1)
      {:ok, groups2} = Lemma.list(e2)
      assert workspace_spec_count(groups1) == 1
      assert workspace_spec_count(groups2) == 0
      assert embedded_stdlib_group(groups2) != nil
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
      :ok = Lemma.load(e1, %{"original" => input})
      :ok = Lemma.load(e2, %{"formatted" => formatted})

      {:ok, r1} = Lemma.run(e1, %{spec: "fmt"}, %{data: %{"x" => "5"}})
      {:ok, r2} = Lemma.run(e2, %{spec: "fmt"}, %{data: %{"x" => "5"}})

      assert comparable_rule_result(r1["results"]["y"]) ==
               comparable_rule_result(r2["results"]["y"])

      assert comparable_rule_result(r1["results"]["z"]) ==
               comparable_rule_result(r2["results"]["z"])
    end
  end
end
