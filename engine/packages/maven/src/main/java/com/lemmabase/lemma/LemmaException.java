package com.lemmabase.lemma;

import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;

/** User/planning error from Lemma. Carries {@link EngineError} entries. */
public final class LemmaException extends RuntimeException {
  /** Serialization version. */
  private static final long serialVersionUID = 1L;

  /** Structured engine errors. */
  private final ArrayList<EngineError> errors;

  /**
   * Creates an exception with typed errors.
   *
   * @param message summary message
   * @param errors structured engine errors
   */
  public LemmaException(String message, List<EngineError> errors) {
    super(message);
    this.errors = new ArrayList<>(Objects.requireNonNull(errors, "errors"));
  }

  /**
   * JNI / LemmaBase wire path: parse errors JSON once into typed entries.
   *
   * @param message summary message
   * @param errorsJson JSON array of engine errors
   */
  public LemmaException(String message, String errorsJson) {
    this(message, JsonSupport.parseEngineErrors(errorsJson));
  }

  /**
   * Structured engine errors.
   *
   * @return unmodifiable error list
   */
  public List<EngineError> errors() {
    return Collections.unmodifiableList(errors);
  }

  /**
   * Groups errors by {@link EngineError#relatedData()}. Errors with a null {@code relatedData} are
   * excluded.
   *
   * @return map from data name to errors for that binding
   */
  public Map<String, List<EngineError>> errorsByData() {
    Map<String, List<EngineError>> byData = new LinkedHashMap<>();
    for (EngineError error : errors) {
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
