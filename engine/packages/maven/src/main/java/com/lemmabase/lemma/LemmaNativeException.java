package com.lemmabase.lemma;

/** Deploy-time native library failure. Catchable; not a JVM invariant. */
public final class LemmaNativeException extends RuntimeException {
  /** Serialization version. */
  private static final long serialVersionUID = 1L;

  /**
   * Creates a native-load failure.
   *
   * @param message failure description
   */
  public LemmaNativeException(String message) {
    super(message);
  }

  /**
   * Creates a native-load failure with a cause.
   *
   * @param message failure description
   * @param cause underlying cause
   */
  public LemmaNativeException(String message, Throwable cause) {
    super(message, cause);
  }
}
