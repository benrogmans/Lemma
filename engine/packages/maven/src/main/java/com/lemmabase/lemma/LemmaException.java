package com.lemmabase.lemma;

import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/** User/planning error from Lemma. Carries WASM-shaped {@link EngineError} entries. */
public final class LemmaException extends RuntimeException {
  private static final long serialVersionUID = 1L;

  private final String errorsJson;
  private volatile ArrayList<EngineError> parsedErrors;

  public LemmaException(String message, String errorsJson) {
    super(message);
    this.errorsJson = errorsJson;
  }

  private ArrayList<EngineError> parse() {
    if (parsedErrors == null) {
      synchronized (this) {
        if (parsedErrors == null) {
          parsedErrors = new ArrayList<>(JsonSupport.parseEngineErrors(errorsJson));
        }
      }
    }
    return parsedErrors;
  }

  public List<EngineError> errors() {
    return Collections.unmodifiableList(parse());
  }

  /**
   * Groups errors by {@link EngineError#relatedData()}. Errors with a null {@code relatedData} are
   * excluded.
   */
  public Map<String, List<EngineError>> errorsByData() {
    Map<String, List<EngineError>> byData = new LinkedHashMap<>();
    for (EngineError error : parse()) {
      String key = error.relatedData();
      if (key == null) {
        continue;
      }
      byData.computeIfAbsent(key, k -> new ArrayList<>()).add(error);
    }
    Map<String, List<EngineError>> unmodifiable = new LinkedHashMap<>();
    for (Map.Entry<String, List<EngineError>> entry : byData.entrySet()) {
      unmodifiable.put(entry.getKey(), Collections.unmodifiableList(entry.getValue()));
    }
    return Collections.unmodifiableMap(unmodifiable);
  }
}
