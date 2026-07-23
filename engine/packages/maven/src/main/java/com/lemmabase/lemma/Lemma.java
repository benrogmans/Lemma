package com.lemmabase.lemma;

import java.util.Objects;

/** Static helpers that do not require an {@link Engine} instance. */
public final class Lemma {
  private Lemma() {}

  public static String format(String code) {
    Objects.requireNonNull(code, "code");
    return Native.format(code);
  }
}
