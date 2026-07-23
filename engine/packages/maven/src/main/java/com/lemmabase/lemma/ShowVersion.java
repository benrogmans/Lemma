package com.lemmabase.lemma;

import org.jspecify.annotations.Nullable;

/** Temporal version window on {@link Show}. */
public record ShowVersion(@Nullable String effectiveFrom, @Nullable String effectiveTo) {}
