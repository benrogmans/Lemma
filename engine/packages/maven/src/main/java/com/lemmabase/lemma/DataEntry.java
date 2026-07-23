package com.lemmabase.lemma;

import java.util.List;
import org.jspecify.annotations.Nullable;

/** One data entry on {@link Show}. */
public record DataEntry(
    Object type,
    @Nullable Object prefilled,
    @Nullable Object suggestion,
    @Nullable List<String> neededByRules) {}
