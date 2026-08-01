package com.lemmabase.lemma;

import com.fasterxml.jackson.core.JsonParser;
import com.fasterxml.jackson.core.JsonToken;
import java.io.IOException;
import java.math.BigDecimal;

/** Spec {@code meta} field value. Externally tagged. */
public sealed interface MetaValue {

  record Literal(LiteralValue value) implements MetaValue {}

  record Unquoted(String value) implements MetaValue {}

  /** Parsed literal value. Externally tagged. */
  public sealed interface LiteralValue {
    record Number(BigDecimal value) implements LiteralValue {}

    record NumberWithUnit(BigDecimal number, String unit) implements LiteralValue {}

    record Text(String value) implements LiteralValue {}

    record Date(String value) implements LiteralValue {}

    record Time(String value) implements LiteralValue {}

    record BooleanLit(String value) implements LiteralValue {}

    record Range(LiteralValue from, LiteralValue to) implements LiteralValue {}

    static LiteralValue read(JsonParser p) throws IOException {
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
            case "date" -> new Date(JsonReading.readString(p));
            case "time" -> new Time(JsonReading.readString(p));
            case "boolean" -> {
              String v = JsonReading.readString(p);
              if (!("true".equals(v) || "false".equals(v) || "yes".equals(v) || "no".equals(v))) {
                throw new LemmaBugError("BUG: invalid boolean literal '" + v + "'");
              }
              yield new BooleanLit(v);
            }
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
  }

  static MetaValue read(JsonParser p) throws IOException {
    JsonReading.expectStartObject(p, "MetaValue");
    p.nextToken();
    if (p.currentToken() == JsonToken.END_OBJECT) {
      throw new LemmaBugError("BUG: empty object for MetaValue");
    }
    String tag = p.currentName();
    p.nextToken();
    MetaValue result =
        switch (tag) {
          case "literal" -> new Literal(LiteralValue.read(p));
          case "unquoted" -> new Unquoted(JsonReading.readString(p));
          default -> throw new LemmaBugError("BUG: unknown tag '" + tag + "' in MetaValue");
        };
    p.nextToken();
    if (p.currentToken() != JsonToken.END_OBJECT) {
      throw new LemmaBugError("BUG: expected END_OBJECT for MetaValue");
    }
    return result;
  }
}
