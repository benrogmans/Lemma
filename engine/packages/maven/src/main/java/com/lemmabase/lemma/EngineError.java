package com.lemmabase.lemma;

import org.jspecify.annotations.Nullable;

/**
 * WASM-shaped engine error (same fields as TypeScript {@code EngineError}).
 */
public record EngineError(
    String kind,
    String message,
    @Nullable String relatedData,
    @Nullable String spec,
    @Nullable String relatedSpec,
    @Nullable EngineErrorSource source,
    @Nullable String suggestion,
    @Nullable String repository) {}
