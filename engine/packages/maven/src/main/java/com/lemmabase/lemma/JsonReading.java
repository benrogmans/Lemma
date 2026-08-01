package com.lemmabase.lemma;

import com.fasterxml.jackson.core.JsonFactory;
import com.fasterxml.jackson.core.JsonGenerator;
import com.fasterxml.jackson.core.JsonParser;
import com.fasterxml.jackson.core.JsonToken;
import java.io.IOException;
import java.io.StringWriter;
import java.math.BigDecimal;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/** Package-private JSON parse primitives for SDK API types. */
final class JsonReading {
  private static final JsonFactory FACTORY = new JsonFactory();

  private JsonReading() {}

  @FunctionalInterface
  interface Reader<T> {
    T read(JsonParser p) throws IOException;
  }

  record Tuple2<A, B>(A first, B second) {}

  static <T> List<T> readList(JsonParser p, Reader<T> reader) throws IOException {
    if (p.currentToken() != JsonToken.START_ARRAY) {
      throw new LemmaBugError("BUG: expected START_ARRAY");
    }
    List<T> list = new ArrayList<>();
    while (p.nextToken() != JsonToken.END_ARRAY) {
      list.add(reader.read(p));
    }
    return list;
  }

  static <T> Map<String, T> readMap(JsonParser p, Reader<T> reader) throws IOException {
    if (p.currentToken() != JsonToken.START_OBJECT) {
      throw new LemmaBugError("BUG: expected START_OBJECT");
    }
    Map<String, T> map = new LinkedHashMap<>();
    while (p.nextToken() != JsonToken.END_OBJECT) {
      String key = p.currentName();
      p.nextToken();
      map.put(key, reader.read(p));
    }
    return map;
  }

  static BigDecimal readDecimal(JsonParser p) throws IOException {
    String text = p.getText();
    try {
      return new BigDecimal(text);
    } catch (NumberFormatException e) {
      throw new LemmaBugError("BUG: invalid decimal '" + text + "'");
    }
  }

  static String readString(JsonParser p) throws IOException {
    if (p.currentToken() == JsonToken.VALUE_NULL) {
      return null;
    }
    return p.getText();
  }

  static Integer readInt(JsonParser p) throws IOException {
    if (p.currentToken() == JsonToken.VALUE_NULL) {
      return null;
    }
    return p.getIntValue();
  }

  static boolean readBoolean(JsonParser p) throws IOException {
    return p.getBooleanValue();
  }

  static <A, B> Tuple2<A, B> readTuple2(JsonParser p, Reader<A> ra, Reader<B> rb)
      throws IOException {
    if (p.currentToken() != JsonToken.START_ARRAY) {
      throw new LemmaBugError("BUG: expected START_ARRAY for tuple");
    }
    p.nextToken();
    A a = ra.read(p);
    p.nextToken();
    B b = rb.read(p);
    p.nextToken();
    if (p.currentToken() != JsonToken.END_ARRAY) {
      throw new LemmaBugError("BUG: expected END_ARRAY after tuple");
    }
    return new Tuple2<>(a, b);
  }

  static String bufferObjectAsString(JsonParser p) throws IOException {
    StringWriter sw = new StringWriter();
    try (JsonGenerator g = FACTORY.createGenerator(sw)) {
      g.copyCurrentStructure(p);
    }
    return sw.toString();
  }

  static JsonParser parserFor(String json) throws IOException {
    JsonParser p = FACTORY.createParser(json);
    p.nextToken();
    return p;
  }

  static String findTag(String json, String tagField) throws IOException {
    try (JsonParser p = parserFor(json)) {
      if (p.currentToken() != JsonToken.START_OBJECT) {
        throw new LemmaBugError("BUG: expected START_OBJECT for findTag");
      }
      while (p.nextToken() != JsonToken.END_OBJECT) {
        String field = p.currentName();
        p.nextToken();
        if (tagField.equals(field)) {
          return p.getText();
        }
        p.skipChildren();
      }
    }
    return null;
  }

  static void expectStartObject(JsonParser p, String typeName) throws IOException {
    if (p.currentToken() != JsonToken.START_OBJECT) {
      throw new LemmaBugError("BUG: expected START_OBJECT for " + typeName);
    }
  }

  static void unknownField(String field, String typeName) {
    throw new LemmaBugError("BUG: unknown field '" + field + "' in " + typeName);
  }

  static void missingRequired(String field, String typeName) {
    throw new LemmaBugError("BUG: missing required field '" + field + "' in " + typeName);
  }
}
