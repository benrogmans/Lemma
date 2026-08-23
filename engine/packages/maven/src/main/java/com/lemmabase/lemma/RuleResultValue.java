package com.lemmabase.lemma;

import com.fasterxml.jackson.core.JsonParser;
import com.fasterxml.jackson.core.JsonToken;
import java.io.IOException;
import java.math.BigDecimal;
import java.util.Map;
import org.jspecify.annotations.Nullable;
/**
 * RuleResultValue.
 * @param display display
 * @param measure measure
 * @param ratio ratio
 * @param number number
 * @param booleanValue booleanValue
 * @param text text
 * @param date date
 * @param time time
 * @param calendar calendar
 * @param range range
 */
public record RuleResultValue(
    @Nullable String display,
    @Nullable Map<String, BigDecimal> measure,
    @Nullable Map<String, BigDecimal> ratio,
    @Nullable BigDecimal number,
    @Nullable Boolean booleanValue,
    @Nullable String text,
    @Nullable String date,
    @Nullable String time,
    @Nullable CalendarResult calendar,
    @Nullable RangeResult range) {
  /**
   * CalendarResult.
   * @param value value
   * @param unit unit
   */
  public record CalendarResult(BigDecimal value, String unit) {
    /**
     * Parses JSON.
     *
     * @param p parser at value start
     * @return parsed value
     * @throws IOException if JSON IO fails
     */
    static CalendarResult read(JsonParser p) throws IOException {
      JsonReading.expectStartObject(p, "CalendarResult");
      BigDecimal value = null;
      String unit = null;
      while (p.nextToken() != JsonToken.END_OBJECT) {
        String field = p.currentName();
        p.nextToken();
        switch (field) {
          case "value" -> value = JsonReading.readDecimal(p);
          case "unit" -> unit = JsonReading.readString(p);
          default -> JsonReading.unknownField(field, "CalendarResult");
        }
      }
      if (value == null) {
        JsonReading.missingRequired("value", "CalendarResult");
      }
      if (unit == null) {
        JsonReading.missingRequired("unit", "CalendarResult");
      }
      return new CalendarResult(value, unit);
    }
  }
  /**
   * Endpoint.
   * @param display display
   * @param measure measure
   * @param ratio ratio
   * @param number number
   * @param booleanValue booleanValue
   * @param text text
   * @param date date
   * @param time time
   * @param calendar calendar
   */
  public record Endpoint(
      @Nullable String display,
      @Nullable Map<String, BigDecimal> measure,
      @Nullable Map<String, BigDecimal> ratio,
      @Nullable BigDecimal number,
      @Nullable Boolean booleanValue,
      @Nullable String text,
      @Nullable String date,
      @Nullable String time,
      @Nullable CalendarResult calendar) {
    /**
     * Parses JSON.
     *
     * @param p parser at value start
     * @return parsed value
     * @throws IOException if JSON IO fails
     */
    static Endpoint read(JsonParser p) throws IOException {
      JsonReading.expectStartObject(p, "RuleResultValueEndpoint");
      String display = null;
      Map<String, BigDecimal> measure = null;
      Map<String, BigDecimal> ratio = null;
      BigDecimal number = null;
      Boolean booleanValue = null;
      String text = null;
      String date = null;
      String time = null;
      CalendarResult calendar = null;
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
          case "calendar" -> calendar = CalendarResult.read(p);
          default -> JsonReading.unknownField(field, "RuleResultValueEndpoint");
        }
      }
      return new Endpoint(
          display, measure, ratio, number, booleanValue, text, date, time, calendar);
    }
  }
  /**
   * RangeResult.
   * @param from from
   * @param to to
   */
  public record RangeResult(Endpoint from, Endpoint to) {
    /**
     * Parses JSON.
     *
     * @param p parser at value start
     * @return parsed value
     * @throws IOException if JSON IO fails
     */
    static RangeResult read(JsonParser p) throws IOException {
      JsonReading.expectStartObject(p, "RangeResult");
      Endpoint from = null;
      Endpoint to = null;
      while (p.nextToken() != JsonToken.END_OBJECT) {
        String field = p.currentName();
        p.nextToken();
        switch (field) {
          case "from" -> from = Endpoint.read(p);
          case "to" -> to = Endpoint.read(p);
          default -> JsonReading.unknownField(field, "RangeResult");
        }
      }
      if (from == null) {
        JsonReading.missingRequired("from", "RangeResult");
      }
      if (to == null) {
        JsonReading.missingRequired("to", "RangeResult");
      }
      return new RangeResult(from, to);
    }
  }

  /**
   * Parses JSON.
   *
   * @param p parser at value start
   * @return parsed value
   * @throws IOException if JSON IO fails
   */
  public static RuleResultValue read(JsonParser p) throws IOException {
    JsonReading.expectStartObject(p, "RuleResultValue");
    String display = null;
    Map<String, BigDecimal> measure = null;
    Map<String, BigDecimal> ratio = null;
    BigDecimal number = null;
    Boolean booleanValue = null;
    String text = null;
    String date = null;
    String time = null;
    CalendarResult calendar = null;
    RangeResult range = null;
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
        case "calendar" -> calendar = CalendarResult.read(p);
        case "range" -> range = RangeResult.read(p);
        default -> JsonReading.unknownField(field, "RuleResultValue");
      }
    }
    return new RuleResultValue(
        display, measure, ratio, number, booleanValue, text, date, time, calendar, range);
  }
}
