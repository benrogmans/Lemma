package com.lemmabase.lemma;

import java.util.Objects;

/** Static helpers that do not require an {@link Engine} instance. */
public final class Lemma {
  /** Prevents instantiation. */
  private Lemma() {}

  /**
   * Formats Lemma source text.
   *
   * @param code source to format
   * @return formatted source
   */
  public static String format(String code) {
    Objects.requireNonNull(code, "code");
    return Native.format(code);
  }
}
