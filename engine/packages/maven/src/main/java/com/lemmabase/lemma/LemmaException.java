package com.lemmabase.lemma;

import java.util.List;

/** User/planning error from Lemma. Carries WASM-shaped {@link EngineError} entries. */
public final class LemmaException extends RuntimeException {
  private static final long serialVersionUID = 1L;

  private final transient List<EngineError> errors;

  public LemmaException(String message, String errorsJson) {
    super(message);
    this.errors = JsonSupport.parseEngineErrors(errorsJson);
  }

  public List<EngineError> errors() {
    return errors;
  }
}
