package com.lemmabase.lemma;

/** Invariant / bug surfaced from the native engine. Not a domain result. */
public final class LemmaBugError extends Error {
  private static final long serialVersionUID = 1L;

  public LemmaBugError(String message) {
    super(message);
  }
}
