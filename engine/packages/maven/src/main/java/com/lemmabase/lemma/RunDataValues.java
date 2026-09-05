package com.lemmabase.lemma;

import java.math.BigDecimal;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;

/** Coerce Java data values to engine run strings. Rejects float/double. */
final class RunDataValues {
  private RunDataValues() {}

  /**
   * Converts every entry to its engine run string. Invalid entries do not fail fast:
   * all of them are collected and surfaced together in one {@link LemmaException}, in the
   * caller's iteration order — the caller sees every bad key at once, not one at a time.
   */
  static Map<String, String> toEngineStrings(Map<String, ?> data) {
    Objects.requireNonNull(data, "data");
    Map<String, String> out = new LinkedHashMap<>();
    List<Invalid> failures = new ArrayList<>();
    for (Map.Entry<String, ?> entry : data.entrySet()) {
      String key = Objects.requireNonNull(entry.getKey(), "data key");
      try {
        out.put(key, coerce(key, entry.getValue()));
      } catch (Invalid invalid) {
        failures.add(invalid);
      }
    }
    if (!failures.isEmpty()) {
      throw asLemmaException(failures);
    }
    return out;
  }

  private static String coerce(String key, Object value) {
    if (value == null) {
      throw new Invalid(key, "data value for '" + key + "' must not be null");
    }
    if (value instanceof String s) {
      return s;
    }
    if (value instanceof Boolean b) {
      return b.toString();
    }
    if (value instanceof Integer i) {
      return Integer.toString(i);
    }
    if (value instanceof Long l) {
      return Long.toString(l);
    }
    if (value instanceof Short s) {
      return Short.toString(s);
    }
    if (value instanceof Byte b) {
      return Byte.toString(b);
    }
    if (value instanceof BigDecimal bd) {
      return bd.toPlainString();
    }
    if (value instanceof Float || value instanceof Double) {
      throw new Invalid(
          key, "decimal values must be passed as BigDecimal (or integer) to preserve exactness");
    }
    if (value instanceof Map<?, ?> map) {
      return unitMapToRunString(key, map);
    }
    throw new Invalid(
        key, "unsupported data value type for '" + key + "': " + value.getClass().getName());
  }

  private static String unitMapToRunString(String key, Map<?, ?> map) {
    if (map.isEmpty()) {
      throw new Invalid(key, "data value object must not be empty");
    }
    if (map.size() != 1) {
      throw new Invalid(key, "data value '" + key + "' must be a run string for JNI run");
    }
    Map.Entry<?, ?> only = map.entrySet().iterator().next();
    String unit = String.valueOf(only.getKey());
    return coerce(key + "." + unit, only.getValue()) + " " + unit;
  }

  private static LemmaException asLemmaException(List<Invalid> failures) {
    String message =
        failures.size() == 1
            ? failures.get(0).getMessage()
            : failures.stream()
                .map(Invalid::getMessage)
                .reduce((a, b) -> a + "; " + b)
                .orElseThrow();
    List<EngineError> errors = new ArrayList<>(failures.size());
    for (Invalid failure : failures) {
      errors.add(EngineError.request(failure.getMessage(), failure.relatedData));
    }
    return new LemmaException(message, errors);
  }

  private static final class Invalid extends RuntimeException {
    private static final long serialVersionUID = 1L;
    private final String relatedData;

    Invalid(String relatedData, String message) {
      super(message);
      this.relatedData = relatedData;
    }
  }
}
