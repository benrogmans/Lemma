package com.lemmabase.lemma;

import java.math.BigDecimal;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Objects;

/** Coerce Java data values to engine convenience strings. Rejects float/double. */
final class DataValues {
  private DataValues() {}

  static Map<String, String> toEngineStrings(Map<String, ?> data) {
    Objects.requireNonNull(data, "data");
    Map<String, String> out = new LinkedHashMap<>();
    for (Map.Entry<String, ?> entry : data.entrySet()) {
      String key = Objects.requireNonNull(entry.getKey(), "data key");
      out.put(key, coerce(key, entry.getValue()));
    }
    return out;
  }

  private static String coerce(String key, Object value) {
    if (value == null) {
      throw request(key, "data value for '" + key + "' must not be null");
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
      throw request(
          key, "decimal values must be passed as strings to preserve exactness");
    }
    if (value instanceof Map<?, ?> map) {
      return unitMapToConvenience(key, map);
    }
    throw request(
        key,
        "unsupported data value type for '" + key + "': " + value.getClass().getName());
  }

  private static String unitMapToConvenience(String key, Map<?, ?> map) {
    if (map.isEmpty()) {
      throw request(key, "data value object must not be empty");
    }
    if (map.size() != 1) {
      throw request(
          key, "data value '" + key + "' must be a convenience string for JNI run");
    }
    Map.Entry<?, ?> only = map.entrySet().iterator().next();
    String unit = String.valueOf(only.getKey());
    String mag = coerce(key + "." + unit, only.getValue());
    return mag + " " + unit;
  }

  private static LemmaException request(String relatedData, String message) {
    String escapedMessage = message.replace("\\", "\\\\").replace("\"", "\\\"");
    String escapedRelated = relatedData.replace("\\", "\\\\").replace("\"", "\\\"");
    String json =
        "[{\"kind\":\"request\",\"message\":\""
            + escapedMessage
            + "\",\"related_data\":\""
            + escapedRelated
            + "\",\"spec\":null,\"related_spec\":null,\"source\":null,\"suggestion\":null,\"repository\":null}]";
    return new LemmaException(message, json);
  }
}
