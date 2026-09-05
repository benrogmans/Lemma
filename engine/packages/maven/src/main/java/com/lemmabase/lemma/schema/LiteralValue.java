package com.lemmabase.lemma.schema;

import com.lemmabase.lemma.JsonReading;
import com.lemmabase.lemma.LemmaBugError;

import com.fasterxml.jackson.core.JsonParser;
import com.fasterxml.jackson.core.JsonToken;
import java.io.IOException;
import java.math.BigDecimal;
import java.time.LocalDate;
import java.time.LocalTime;

/** Parsed literal value. Externally tagged. */
public sealed interface LiteralValue {
  /**
   * Number.
   *
   * @param value value
   */
  record Number(BigDecimal value) implements LiteralValue {}

  /**
   * NumberWithUnit.
   *
   * @param number number
   * @param unit unit
   */
  record NumberWithUnit(BigDecimal number, String unit) implements LiteralValue {}

  /**
   * Text.
   *
   * @param value value
   */
  record Text(String value) implements LiteralValue {}

  /**
   * Date.
   *
   * @param value calendar date
   */
  record Date(LocalDate value) implements LiteralValue {}

  /**
   * Time.
   *
   * @param value time of day
   */
  record Time(LocalTime value) implements LiteralValue {}

  /**
   * BooleanLit.
   *
   * @param value boolean value
   */
  record BooleanLit(boolean value) implements LiteralValue {}

  /**
   * Range.
   *
   * @param from from
   * @param to to
   */
  record Range(LiteralValue from, LiteralValue to) implements LiteralValue {}

  /**
   * Parses JSON.
   *
   * @param p parser at value start
   * @return parsed value
   * @throws IOException if JSON IO fails
   */
  public static LiteralValue read(JsonParser p) throws IOException {
    JsonReading.expectStartObject(p, "LiteralValue");
    p.nextToken();
    if (p.currentToken() == JsonToken.END_OBJECT) {
      throw new LemmaBugError("BUG: empty object for LiteralValue");
    }
    String tag = p.currentName();
    p.nextToken();
    LiteralValue result =
        switch (tag) {
          case "number" -> new Number(JsonReading.readDecimal(p));
          case "number_with_unit" -> {
            var tuple =
                JsonReading.readTuple2(p, JsonReading::readDecimal, JsonReading::readString);
            yield new NumberWithUnit(tuple.first(), tuple.second());
          }
          case "text" -> new Text(JsonReading.readString(p));
          case "date" -> new Date(JsonReading.readLocalDate(p));
          case "time" -> new Time(JsonReading.readLocalTime(p));
          case "boolean" -> new BooleanLit(readWireBoolean(p));
          case "range" -> {
            var tuple = JsonReading.readTuple2(p, LiteralValue::read, LiteralValue::read);
            yield new Range(tuple.first(), tuple.second());
          }
          default -> throw new LemmaBugError("BUG: unknown tag '" + tag + "' in LiteralValue");
        };
    p.nextToken();
    if (p.currentToken() != JsonToken.END_OBJECT) {
      throw new LemmaBugError("BUG: expected END_OBJECT for LiteralValue");
    }
    return result;
  }

  private static boolean readWireBoolean(JsonParser p) throws IOException {
    String v = JsonReading.readString(p);
    return switch (v) {
      case "true", "yes" -> true;
      case "false", "no" -> false;
      default -> throw new LemmaBugError("BUG: invalid boolean literal '" + v + "'");
    };
  }
}
