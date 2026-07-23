package com.lemmabase.lemma;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.math.BigDecimal;
import java.util.List;
import java.util.Map;
import org.junit.jupiter.api.Test;

final class EngineContractTest {

  @Test
  void rejectsDoubleDataValues() {
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
  void rejectsFloatDataValues() {
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
  void convenienceUnitStringAndBigDecimalResult() {
    try (Engine engine = Engine.create()) {
      engine.load(
          """
          spec shipping
          data weight: measure
            -> unit kilogram 1
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
}
