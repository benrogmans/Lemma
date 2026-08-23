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
  /** Jackson parser factory. */
  private static final JsonFactory FACTORY = new JsonFactory();

  /** Prevents instantiation. */
  private JsonReading() {}

  /** Reads one JSON value from a parser. */
  @FunctionalInterface
  interface Reader<T> {
    /**
     * Reads one value.
     *
     * @param p parser positioned at the value
     */
    T read(JsonParser p) throws IOException;
  }
  record Tuple2<A, B>(A first, B second) {}

  /**
   * Reads a JSON array with {@code reader} per element.
   *
   * @param p parser at {@code START_ARRAY}
   * @param reader element reader
   */
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

  /**
   * Reads a JSON object map with {@code reader} per value.
   *
   * @param p parser at {@code START_OBJECT}
   * @param reader value reader
   */
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

  /**
   * Reads a JSON number as {@link BigDecimal}.
   *
   * @param p parser at a numeric token
   */
  static BigDecimal readDecimal(JsonParser p) throws IOException {
    String text = p.getText();
    try {
      return new BigDecimal(text);
    } catch (NumberFormatException e) {
      throw new LemmaBugError("BUG: invalid decimal '" + text + "'");
    }
  }

  /**
   * Reads a JSON string or null.
   *
   * @param p parser at string or null token
   */
  static String readString(JsonParser p) throws IOException {
    if (p.currentToken() == JsonToken.VALUE_NULL) {
      return null;
    }
    return p.getText();
  }

  /**
   * Reads a JSON integer or null.
   *
   * @param p parser at integer or null token
   */
  static Integer readInt(JsonParser p) throws IOException {
    if (p.currentToken() == JsonToken.VALUE_NULL) {
      return null;
    }
    return p.getIntValue();
  }

  /**
   * Reads a JSON boolean.
   *
   * @param p parser at boolean token
   */
  static boolean readBoolean(JsonParser p) throws IOException {
    return p.getBooleanValue();
  }

  /**
   * Reads a two-element JSON array tuple.
   *
   * @param p parser at {@code START_ARRAY}
   * @param ra first element reader
   * @param rb second element reader
   */
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

  /**
   * Copies the current JSON object to a string.
   *
   * @param p parser inside an object
   */
  static String bufferObjectAsString(JsonParser p) throws IOException {
    StringWriter sw = new StringWriter();
    try (JsonGenerator g = FACTORY.createGenerator(sw)) {
      g.copyCurrentStructure(p);
    }
    return sw.toString();
  }

  /**
   * Creates a parser for a JSON document.
   *
   * @param json JSON text
   */
  static JsonParser parserFor(String json) throws IOException {
    JsonParser p = FACTORY.createParser(json);
    p.nextToken();
    return p;
  }

  /**
   * Finds a string field value in a shallow JSON object.
   *
   * @param json JSON object text
   * @param tagField field name to read
   */
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

  /**
   * Requires the parser to be at {@code START_OBJECT}.
   *
   * @param p parser to check
   * @param typeName type name for error messages
   */
  static void expectStartObject(JsonParser p, String typeName) throws IOException {
    if (p.currentToken() != JsonToken.START_OBJECT) {
      throw new LemmaBugError("BUG: expected START_OBJECT for " + typeName);
    }
  }

  /**
   * Reports an unknown JSON field.
   *
   * @param field field name
   * @param typeName type being parsed
   */
  static void unknownField(String field, String typeName) {
    throw new LemmaBugError("BUG: unknown field '" + field + "' in " + typeName);
  }

  /**
   * Reports a missing required JSON field.
   *
   * @param field field name
   * @param typeName type being parsed
   */
  static void missingRequired(String field, String typeName) {
    throw new LemmaBugError("BUG: missing required field '" + field + "' in " + typeName);
  }
}
