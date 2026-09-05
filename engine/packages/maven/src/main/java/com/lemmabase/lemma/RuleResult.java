package com.lemmabase.lemma;

import com.fasterxml.jackson.core.JsonParser;
import com.fasterxml.jackson.core.JsonToken;
import com.lemmabase.lemma.schema.ExplanationNode;
import java.io.IOException;
import java.math.BigDecimal;
import java.time.LocalDate;
import java.time.LocalTime;
import java.util.List;
import java.util.Map;
import org.jspecify.annotations.Nullable;

/**
 * Result of evaluating one rule. Pattern-match on the variant: veto, missing data, or a typed
 * value.
 */
public sealed interface RuleResult {
  /**
   * Declared rule type name.
   *
   * @return rule type
   */
  String ruleType();

  /**
   * Explanation tree when {@code explain} was requested; otherwise null.
   *
   * @return explanation or null
   */
  ExplanationNode.@Nullable Rule explanation();

  /**
   * Domain or engine veto (not unbound inputs).
   *
   * @param vetoReason veto text; may be null
   * @param ruleType rule type
   * @param explanation explanation or null
   */
  record Veto(
      @Nullable String vetoReason,
      String ruleType,
      ExplanationNode.@Nullable Rule explanation)
      implements RuleResult {}

  /**
   * Rule still waits on unbound inputs.
   *
   * @param missingData unbound data keys in evaluation order
   * @param ruleType rule type
   * @param explanation explanation or null
   */
  record MissingData(
      List<String> missingData,
      String ruleType,
      ExplanationNode.@Nullable Rule explanation)
      implements RuleResult {}

  /**
   * Number result.
   *
   * @param display engine display string
   * @param number magnitude
   * @param ruleType rule type
   * @param explanation explanation or null
   */
  record Number(
      String display,
      BigDecimal number,
      String ruleType,
      ExplanationNode.@Nullable Rule explanation)
      implements RuleResult {}

  /**
   * Text result.
   *
   * @param display engine display string
   * @param text text value
   * @param ruleType rule type
   * @param explanation explanation or null
   */
  record Text(
      String display,
      String text,
      String ruleType,
      ExplanationNode.@Nullable Rule explanation)
      implements RuleResult {}

  /**
   * Boolean result.
   *
   * @param display engine display string
   * @param booleanValue boolean value
   * @param ruleType rule type
   * @param explanation explanation or null
   */
  record BooleanValue(
      String display,
      boolean booleanValue,
      String ruleType,
      ExplanationNode.@Nullable Rule explanation)
      implements RuleResult {}

  /**
   * Date result.
   *
   * @param display engine display string
   * @param date calendar date
   * @param ruleType rule type
   * @param explanation explanation or null
   */
  record Date(
      String display,
      LocalDate date,
      String ruleType,
      ExplanationNode.@Nullable Rule explanation)
      implements RuleResult {}

  /**
   * Time result.
   *
   * @param display engine display string
   * @param time time of day
   * @param ruleType rule type
   * @param explanation explanation or null
   */
  record Time(
      String display,
      LocalTime time,
      String ruleType,
      ExplanationNode.@Nullable Rule explanation)
      implements RuleResult {}

  /**
   * Measure result (unit name to magnitude).
   *
   * @param display engine display string
   * @param measure unit map
   * @param ruleType rule type
   * @param explanation explanation or null
   */
  record Measure(
      String display,
      Map<String, BigDecimal> measure,
      String ruleType,
      ExplanationNode.@Nullable Rule explanation)
      implements RuleResult {}

  /**
   * Ratio result (unit name to magnitude).
   *
   * @param display engine display string
   * @param ratio unit map
   * @param ruleType rule type
   * @param explanation explanation or null
   */
  record Ratio(
      String display,
      Map<String, BigDecimal> ratio,
      String ruleType,
      ExplanationNode.@Nullable Rule explanation)
      implements RuleResult {}

  /**
   * Calendar measure result.
   *
   * @param display engine display string
   * @param calendar calendar value
   * @param ruleType rule type
   * @param explanation explanation or null
   */
  record Calendar(
      String display,
      RuleResultValue.CalendarResult calendar,
      String ruleType,
      ExplanationNode.@Nullable Rule explanation)
      implements RuleResult {}

  /**
   * Range result.
   *
   * @param display engine display string
   * @param range endpoints
   * @param ruleType rule type
   * @param explanation explanation or null
   */
  record Range(
      String display,
      RuleResultValue.RangeResult range,
      String ruleType,
      ExplanationNode.@Nullable Rule explanation)
      implements RuleResult {}

  /**
   * Parses JSON.
   *
   * @param p parser at value start
   * @return parsed value
   * @throws IOException if JSON IO fails
   */
  static RuleResult read(JsonParser p) throws IOException {
    JsonReading.expectStartObject(p, "RuleResult");
    String display = null;
    Map<String, BigDecimal> measure = null;
    Map<String, BigDecimal> ratio = null;
    BigDecimal number = null;
    Boolean booleanValue = null;
    String text = null;
    LocalDate date = null;
    LocalTime time = null;
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
        case "date" -> date = JsonReading.readLocalDate(p);
        case "time" -> time = JsonReading.readLocalTime(p);
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
    if (missingData != null && !missingData.isEmpty()) {
      return new MissingData(List.copyOf(missingData), ruleType, explanation);
    }
    if (vetoed) {
      return new Veto(vetoReason, ruleType, explanation);
    }
    if (display == null) {
      JsonReading.missingRequired("display", "RuleResult");
    }
    if (number != null) {
      return new Number(display, number, ruleType, explanation);
    }
    if (text != null) {
      return new Text(display, text, ruleType, explanation);
    }
    if (booleanValue != null) {
      return new BooleanValue(display, booleanValue, ruleType, explanation);
    }
    if (date != null) {
      return new Date(display, date, ruleType, explanation);
    }
    if (time != null) {
      return new Time(display, time, ruleType, explanation);
    }
    if (measure != null) {
      return new Measure(display, measure, ruleType, explanation);
    }
    if (ratio != null) {
      return new Ratio(display, ratio, ruleType, explanation);
    }
    if (calendar != null) {
      return new Calendar(display, calendar, ruleType, explanation);
    }
    if (range != null) {
      return new Range(display, range, ruleType, explanation);
    }
    throw new LemmaBugError("BUG: non-veto RuleResult has no typed value field");
  }
}
