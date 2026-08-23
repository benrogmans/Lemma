package com.lemmabase.lemma;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.util.List;
import java.util.Map;
import org.junit.jupiter.api.Test;

final class EngineContractTest {

  @Test
  void updateReplacesSpecSlice() {
    try (Engine engine = Engine.create()) {
      engine.load(
          """
          spec pricing
          data quantity: 1
          rule total: quantity * 10
          """);
      engine.update(
          null,
          "pricing",
          null,
          """
          spec pricing
          data quantity: 1
          rule total: quantity * 20
          """,
          null);
      Response response = engine.run(RunRequest.of("pricing"));
      assertEquals("20", response.results().get("total").number().toPlainString());
    }
  }

  @Test
  void qualityReportsMissingHelp() {
    try (Engine engine = Engine.create()) {
      engine.load(
          """
          spec pricing 2026-01-01
          \"\"\"
          Bulk pricing.
          \"\"\"

          data qty: number
          rule total: qty
          """);
      List<Recommendation> recs = engine.quality();
      assertFalse(recs.isEmpty());
      Recommendation hit =
          recs.stream()
              .filter(r -> r.message().contains("no `-> help`"))
              .findFirst()
              .orElseThrow();
      assertEquals("pricing", hit.spec());
      assertEquals("2026-01-01", hit.effectiveFrom());
      assertTrue(hit.source().attribute().length() > 0);
      assertTrue(hit.source().line() > 0);
    }
  }

  @Test
  void qualityEmptyForCleanSpec() {
    try (Engine engine = Engine.create()) {
      engine.load(
          """
          spec pricing 2026-01-01
          \"\"\"
          Bulk pricing.
          \"\"\"

          data qty: number
            -> minimum 0
            -> maximum 1000000
            -> help "Order quantity."

          rule total: qty
          """);
      assertTrue(engine.quality().isEmpty());
    }
  }

  @Test
  void rejectsDoubleRunDataValues() {
    try (Engine engine = Engine.create()) {
      engine.load(
          """
          spec pricing
          data amount: number
          rule doubled: amount * 2
          """);
      LemmaException thrown =
          assertThrows(
              LemmaException.class,
              () ->
                  engine.run(
                      RunRequest.of("pricing").data(Map.of("amount", 1.5))));
      assertTrue(
          thrown.getMessage().contains("decimal values must be passed as strings"),
          thrown.getMessage());
      assertFalse(thrown.errors().isEmpty());
      assertEquals("request", thrown.errors().get(0).kind());
    }
  }

  @Test
  void rejectsFloatRunDataValues() {
    try (Engine engine = Engine.create()) {
      engine.load(
          """
          spec pricing
          data amount: number
          rule doubled: amount * 2
          """);
      assertThrows(
          LemmaException.class,
          () ->
              engine.run(
                  RunRequest.of("pricing").data(Map.of("amount", 1.5f))));
    }
  }

  @Test
  void vetoRemainsInRuleResult() {
    try (Engine engine = Engine.create()) {
      engine.load(
          """
          spec deny
          rule outcome: veto "not allowed"
          """);
      Response response = engine.run(RunRequest.of("deny"));
      RuleResult outcome = response.results().get("outcome");
      assertTrue(outcome.vetoed());
      assertEquals("not allowed", outcome.vetoReason());
    }
  }

  @Test
  void emptyRulesListFails() {
    try (Engine engine = Engine.create()) {
      engine.load(
          """
          spec sample
          rule value: 1
          """);
      LemmaException thrown =
          assertThrows(
              LemmaException.class,
              () -> engine.run(RunRequest.of("sample").rules(List.of())));
      assertTrue(thrown.getMessage().contains("run failed"), thrown.getMessage());
      assertTrue(
          thrown.errors().stream().anyMatch(e -> e.message().contains("rules must not be empty")),
          thrown.errors().toString());
    }
  }

  @Test
  void invalidLoadThrowsLemmaException() {
    try (Engine engine = Engine.create()) {
      LemmaException thrown =
          assertThrows(LemmaException.class, () -> engine.load("this is not lemma"));
      assertFalse(thrown.errors().isEmpty());
    }
  }

  @Test
  void useAfterCloseThrowsLemmaBugError() {
    Engine engine = Engine.create();
    engine.close();
    assertThrows(LemmaBugError.class, () -> engine.load("spec x\nrule y: 1"));
  }

  @Test
  void convenienceUnitStringAndDecimalResult() {
    try (Engine engine = Engine.create()) {
      engine.load(
          """
          spec shipping
          data weight: measure
            -> unit kilogram: 1
          rule heavy: weight > 10 kilogram
          """);
      Response response =
          engine.run(
              RunRequest.of("shipping")
                  .data(Map.of("weight", "12 kilogram"))
                  .rules(List.of("heavy")));
      RuleResult heavy = response.results().get("heavy");
      assertFalse(heavy.vetoed());
      assertEquals(Boolean.TRUE, heavy.booleanValue());
    }
  }

  @Test
  void formatReturnsNormalizedSource() {
    String formatted =
        Lemma.format(
            """
            spec demo
            rule x:1
            """);
    assertTrue(formatted.contains("spec demo"));
    assertTrue(formatted.contains("rule x"));
  }

  @Test
  void versionedSpecShowListAndRunSucceedWithIsoStrings() {
    try (Engine engine = Engine.create()) {
      engine.load(
          """
          spec policy 2024-01-01
          data amount: number
          rule ok: amount

          spec policy 2025-06-01
          data amount: number
          rule ok: amount * 2
          """);

      Show show = engine.show(null, "policy", "2024-06-01");
      assertEquals("2024-01-01", show.effectiveFrom());
      assertEquals("2025-06-01", show.effectiveTo());
      assertFalse(show.versions().isEmpty());
      Show.ShowVersion first = show.versions().get(0);
      assertEquals("2024-01-01", first.effectiveFrom());

      List<String> policyEffectiveFroms = new java.util.ArrayList<>();
      for (ResolvedRepository repo : engine.list()) {
        for (ResolvedRepository.ListedSpec spec : repo.specs()) {
          if ("policy".equals(spec.name())) {
            policyEffectiveFroms.add(spec.effectiveFrom());
          }
        }
      }
      assertEquals(List.of("2024-01-01", "2025-06-01"), policyEffectiveFroms);

      Response response =
          engine.run(
              RunRequest.of("policy")
                  .effective("2024-06-01")
                  .data(Map.of("amount", "3")));
      assertFalse(response.results().get("ok").vetoed());
      assertEquals("3", response.results().get("ok").display());
    }
  }

  @Test
  void singleVersionShowOmitsEffectiveToAsNull() {
    try (Engine engine = Engine.create()) {
      engine.load(
          """
          spec solo 2024-01-01
          data amount: number
          rule ok: amount
          """);
      Show show = engine.show(null, "solo", "2024-06-01");
      assertEquals("2024-01-01", show.effectiveFrom());
      assertEquals(null, show.effectiveTo());
    }
  }

  @Test
  void showDataAndResponseResultsPreserveDeclarationOrder() {
    try (Engine engine = Engine.create()) {
      engine.load(
          """
          spec topo
          data zebra: number
          data yankee: number
          data alpha: number
          rule z: zebra
          rule a: z + yankee + alpha
          """);
      Show show = engine.show(null, "topo", null);
      assertEquals(
          java.util.List.of("zebra", "yankee", "alpha"),
          java.util.List.copyOf(show.data().keySet()));
      assertEquals(java.util.List.of("z", "a"), java.util.List.copyOf(show.rules().keySet()));

      Response response =
          engine.run(
              RunRequest.of("topo")
                  .data(
                      java.util.Map.of(
                          "zebra", "1",
                          "yankee", "2",
                          "alpha", "3")));
      assertEquals(
          java.util.List.copyOf(show.rules().keySet()),
          java.util.List.copyOf(response.results().keySet()));
    }
  }

  @Test
  void nullDataValueRaisesAttributedLemmaExceptionNotNpe() {
    try (Engine engine = Engine.create()) {
      engine.load(
          """
          spec pricing
          data amount: number
          rule ok: amount
          """);
      java.util.Map<String, Object> data = new java.util.LinkedHashMap<>();
      data.put("amount", null);
      LemmaException thrown =
          assertThrows(
              LemmaException.class,
              () -> engine.run(RunRequest.of("pricing").data(data)));
      assertFalse(
          thrown.getCause() instanceof NullPointerException
              || thrown.getMessage().contains("NullPointerException"),
          "must not surface bare NPE from Map.copyOf");
      assertFalse(thrown.errors().isEmpty());
      assertEquals("amount", thrown.errors().get(0).relatedData());
      assertTrue(
          thrown.errors().get(0).message().contains("must not be null"),
          thrown.errors().get(0).message());
    }
  }

  @Test
  void engineErrorKindIsStringType() throws Exception {
    var errorRecord = EngineError.class;
    var kindMethod = errorRecord.getMethod("kind");
    assertEquals(String.class, kindMethod.getReturnType(), "EngineError.kind() should return String");
  }

  @Test
  void unrecognizedErrorKindReadsAsString() {
    String json =
        "[{\"kind\":\"not_a_real_kind\",\"message\":\"x\",\"related_data\":null,\"spec\":null,\"related_spec\":null,\"source\":null,\"suggestion\":null,\"repository\":null,\"registry_kind\":null,\"request_kind\":null,\"limit_name\":null,\"limit_value\":null,\"actual_value\":null}]";
    var errors = new LemmaException("x", json).errors();
    assertEquals(1, errors.size());
    assertEquals("not_a_real_kind", errors.get(0).kind());
  }

  @Test
  void multipleInvalidRunDataValuesReportInInputOrder() {
    try (Engine engine = Engine.create()) {
      engine.load(
          """
          spec multi
          data a: number
          data b: number
          data c: number
          data d: number
          data e: number
          rule r: a
          """);
      java.util.Map<String, Object> data = new java.util.LinkedHashMap<>();
      data.put("a", null);
      data.put("b", "1");
      data.put("c", null);
      data.put("d", "2");
      data.put("e", null);
      LemmaException thrown =
          assertThrows(
              LemmaException.class,
              () -> engine.run(RunRequest.of("multi").data(data)));
      assertFalse(
          thrown.getCause() instanceof NullPointerException,
          "must not surface bare NPE from Map.copyOf");
      java.util.List<String> related =
          thrown.errors().stream()
              .map(EngineError::relatedData)
              .filter(java.util.Objects::nonNull)
              .toList();
      assertEquals(java.util.List.of("a", "c", "e"), related);
    }
  }
}
