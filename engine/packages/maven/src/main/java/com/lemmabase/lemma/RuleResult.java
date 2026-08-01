package com.lemmabase.lemma;

import com.fasterxml.jackson.core.JsonParser;
import com.fasterxml.jackson.core.JsonToken;
import java.io.IOException;
import java.math.BigDecimal;
import java.util.List;
import java.util.Map;
import org.jspecify.annotations.Nullable;

/**
 * Result of evaluating one rule. {@link RuleResultValue} fields are flattened onto this object when
 * the rule is not vetoed.
 */
public record RuleResult(
    @Nullable String display,
    @Nullable Map<String, BigDecimal> measure,
    @Nullable Map<String, BigDecimal> ratio,
    @Nullable BigDecimal number,
    @Nullable Boolean booleanValue,
    @Nullable String text,
    @Nullable String date,
    @Nullable String time,
    RuleResultValue.@Nullable CalendarResult calendar,
    RuleResultValue.@Nullable RangeResult range,
    boolean vetoed,
    @Nullable String vetoReason,
    String ruleType,
    @Nullable List<String> missingData,
    ExplanationNode.@Nullable Rule explanation) {

  public static RuleResult read(JsonParser p) throws IOException {
    JsonReading.expectStartObject(p, "RuleResult");
    String display = null;
    Map<String, BigDecimal> measure = null;
    Map<String, BigDecimal> ratio = null;
    BigDecimal number = null;
    Boolean booleanValue = null;
    String text = null;
    String date = null;
    String time = null;
    RuleResultValue.CalendarResult calendar = null;
    RuleResultValue.RangeResult range = null;
    Boolean vetoed = null;
    String vetoReason = null;
    String ruleType = null;
    List<String> missingData = null;
    ExplanationNode.Rule explanation = null;
    while (p.nextToken() != JsonToken.END_OBJECT) {
      String field = p.currentName();
      p.nextToken();
      switch (field) {
        case "display" -> display = JsonReading.readString(p);
        case "measure" -> measure = JsonReading.readMap(p, JsonReading::readDecimal);
        case "ratio" -> ratio = JsonReading.readMap(p, JsonReading::readDecimal);
        case "number" -> number = JsonReading.readDecimal(p);
        case "boolean" -> booleanValue = JsonReading.readBoolean(p);
        case "text" -> text = JsonReading.readString(p);
        case "date" -> date = JsonReading.readString(p);
        case "time" -> time = JsonReading.readString(p);
        case "calendar" -> calendar = RuleResultValue.CalendarResult.read(p);
        case "range" -> range = RuleResultValue.RangeResult.read(p);
        case "vetoed" -> vetoed = JsonReading.readBoolean(p);
        case "veto_reason" -> vetoReason = JsonReading.readString(p);
        case "rule_type" -> ruleType = JsonReading.readString(p);
        case "missing_data" -> missingData = JsonReading.readList(p, JsonReading::readString);
        case "explanation" -> explanation = ExplanationNode.Rule.read(p);
        default -> JsonReading.unknownField(field, "RuleResult");
      }
    }
    if (vetoed == null) {
      JsonReading.missingRequired("vetoed", "RuleResult");
    }
    if (ruleType == null) {
      JsonReading.missingRequired("rule_type", "RuleResult");
    }
    return new RuleResult(
        display,
        measure,
        ratio,
        number,
        booleanValue,
        text,
        date,
        time,
        calendar,
        range,
        vetoed,
        vetoReason,
        ruleType,
        missingData,
        explanation);
  }
}
