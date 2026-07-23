package com.lemmabase.lemma;

import org.jspecify.annotations.Nullable;

/** Listed spec metadata row. */
public record ListedSpec(
    String name, @Nullable String effectiveFrom, @Nullable String effectiveTo) {}
