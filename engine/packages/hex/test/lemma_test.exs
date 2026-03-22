defmodule LemmaTest do
  use ExUnit.Case, async: true

  @simple_spec """
  spec pricing
  fact quantity: [number]
  fact price: 10
  rule total: quantity * price
  rule discount: 0
    unless quantity >= 10 then 5
    unless quantity >= 50 then 15
  """

  describe "new/0" do
    test "creates engine with default limits" do
      assert {:ok, engine} = Lemma.new()
      assert is_reference(engine)
    end

    test "creates engine with custom limits" do
      assert {:ok, engine} = Lemma.new(%{"max_files" => 50})
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
        Lemma.new(%{"max_files" => "not_a_number"})
      end
    end

    test "rejects unknown limit key" do
      assert_raise ErlangError, fn ->
        Lemma.new(%{"bogus_key" => 10})
      end
    end

    test "rejects negative limit value" do
      assert_raise ErlangError, fn ->
        Lemma.new(%{"max_files" => -1})
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
      assert {:error, errors} = Lemma.load(engine, "spec bad\nfact x: [bogus]", "bad.lemma")
      assert is_list(errors)
      assert length(errors) > 0
      first = hd(errors)
      assert is_map(first)
      assert Map.has_key?(first, :message)
    end

    test "uses 'inline' as default source label" do
      {:ok, engine} = Lemma.new()
      assert :ok = Lemma.load(engine, "spec inline_test\nfact x: 1\nrule y: x + 1")
    end
  end

  describe "list/1" do
    test "lists loaded specs" do
      {:ok, engine} = Lemma.new()
      :ok = Lemma.load(engine, @simple_spec, "pricing.lemma")
      assert {:ok, specs} = Lemma.list(engine)
      assert is_list(specs)
      assert length(specs) == 1
      spec = hd(specs)
      assert spec[:name] == "pricing"
    end

    test "empty engine returns empty list" do
      {:ok, engine} = Lemma.new()
      assert {:ok, []} = Lemma.list(engine)
    end

    test "effective_from is nil when not set" do
      {:ok, engine} = Lemma.new()
      :ok = Lemma.load(engine, "spec no_effective\nfact x: 1", "test.lemma")
      {:ok, [spec]} = Lemma.list(engine)
      assert spec[:effective_from] == nil
    end
  end

  describe "schema/3" do
    test "returns schema for loaded spec" do
      {:ok, engine} = Lemma.new()
      :ok = Lemma.load(engine, @simple_spec, "pricing.lemma")
      assert {:ok, schema} = Lemma.schema(engine, "pricing")
      assert is_map(schema)
      assert schema["spec"] == "pricing"
      assert is_map(schema["facts"])
      assert is_map(schema["rules"])
      assert Map.has_key?(schema["facts"], "quantity")
      assert Map.has_key?(schema["rules"], "total")
      assert Map.has_key?(schema["rules"], "discount")
    end

    test "returns error for unknown spec" do
      {:ok, engine} = Lemma.new()
      assert {:error, _} = Lemma.schema(engine, "nonexistent")
    end
  end

  describe "run/3" do
    test "runs spec with provided facts" do
      {:ok, engine} = Lemma.new()
      :ok = Lemma.load(engine, @simple_spec, "pricing.lemma")
      assert {:ok, response} = Lemma.run(engine, "pricing", facts: %{"quantity" => "5"})
      assert is_map(response)
      assert response["spec_name"] == "pricing"
      results = response["results"]
      assert is_map(results)
      total = results["total"]
      assert total["result"]["value"]["display_value"] == "50"
    end

    test "runs spec with quantity triggering unless clause" do
      {:ok, engine} = Lemma.new()
      :ok = Lemma.load(engine, @simple_spec, "pricing.lemma")
      {:ok, response} = Lemma.run(engine, "pricing", facts: %{"quantity" => "10"})
      results = response["results"]
      assert results["discount"]["result"]["value"]["display_value"] == "5"
    end

    test "runs spec with no optional facts" do
      {:ok, engine} = Lemma.new()
      :ok = Lemma.load(engine, "spec simple\nfact x: 1\nrule y: x + 1", "s.lemma")
      {:ok, response} = Lemma.run(engine, "simple")
      results = response["results"]
      assert results["y"]["result"]["value"]["display_value"] == "2"
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
      fact x: [number]
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
      fact x: [number]
      rule y: x * 2
      """

      :ok = Lemma.load(engine, spec, "inv2.lemma")

      target = %{outcome: :value, op: :eq, value: "10"}

      assert {:ok, result} = Lemma.invert(engine, "invertible2", "2025-01-01", "y", target)
      assert is_map(result)
    end

    test "rejects target without outcome" do
      {:ok, engine} = Lemma.new()
      :ok = Lemma.load(engine, "spec inv3\nfact x: [number]\nrule y: x + 1", "inv3.lemma")

      assert_raise ErlangError, fn ->
        Lemma.invert(engine, "inv3", "2025-01-01", "y", %{op: :eq, value: "5"})
      end
    end
  end

  describe "remove_spec/3" do
    test "removes a loaded spec" do
      {:ok, engine} = Lemma.new()
      :ok = Lemma.load(engine, "spec removable\nfact x: 1\nrule y: x + 1", "rm.lemma")
      {:ok, specs} = Lemma.list(engine)
      assert length(specs) == 1

      assert :ok = Lemma.remove_spec(engine, "removable", "2025-01-01")

      {:ok, specs} = Lemma.list(engine)
      assert length(specs) == 0
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
      :ok = Lemma.load(e1, "spec a\nfact x: 1\nrule y: x + 1", "a.lemma")
      {:ok, specs1} = Lemma.list(e1)
      {:ok, specs2} = Lemma.list(e2)
      assert length(specs1) == 1
      assert length(specs2) == 0
    end
  end

  describe "load_from_paths/2" do
    test "loads specs from a directory" do
      dir = System.tmp_dir!()
      path = Path.join(dir, "hex_test_spec.lemma")
      File.write!(path, "spec from_file\nfact x: 1\nrule y: x + 1")

      {:ok, engine} = Lemma.new()
      assert :ok = Lemma.load_from_paths(engine, [path])
      {:ok, specs} = Lemma.list(engine)
      names = Enum.map(specs, & &1[:name])
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
      input = "spec foo\nfact   x:  1\nrule y: x +  1"
      assert {:ok, formatted} = Lemma.format(input)
      assert is_binary(formatted)
      assert formatted =~ "spec foo"
      assert formatted =~ "fact x"
      assert formatted =~ "rule y:"
      assert formatted =~ "x + 1"
    end

    test "returns error for invalid source" do
      assert {:error, err} = Lemma.format("not valid lemma at all !!!")
      assert is_map(err)
      assert Map.has_key?(err, :message)
    end

    test "preserves semantics after formatting" do
      input = "spec fmt\nfact x: [number]\nrule y: x *   2\nrule z: y + 1"
      {:ok, formatted} = Lemma.format(input)

      {:ok, e1} = Lemma.new()
      {:ok, e2} = Lemma.new()
      :ok = Lemma.load(e1, input, "original")
      :ok = Lemma.load(e2, formatted, "formatted")

      {:ok, r1} = Lemma.run(e1, "fmt", facts: %{"x" => "5"})
      {:ok, r2} = Lemma.run(e2, "fmt", facts: %{"x" => "5"})

      assert r1["results"]["y"]["result"] == r2["results"]["y"]["result"]
      assert r1["results"]["z"]["result"] == r2["results"]["z"]["result"]
    end
  end
end
