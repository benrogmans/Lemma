package com.lemmabase.lemma;

import java.util.List;
import java.util.Map;
import org.jspecify.annotations.Nullable;

/** Spec interface from {@link Engine#show}. */
public record Show(
    String spec,
    @Nullable String commentary,
    @Nullable String effectiveFrom,
    @Nullable String effectiveTo,
    int startLine,
    @Nullable Object sourceType,
    @Nullable List<ShowVersion> versions,
    Map<String, DataEntry> data,
    Map<String, Object> rules,
    @Nullable Map<String, Object> meta) {}
