package com.lemmabase.lemma;

/** Invariant / bug surfaced from the native engine. Not a domain result. */
public final class LemmaBugError extends Error {
  /** Serialization version. */
  private static final long serialVersionUID = 1L;

  /**
   * Creates a bug error.
   *
   * @param message invariant violation description
   */
  public LemmaBugError(String message) {
    super(message);
  }
}
