package com.lemmabase.lemma;

/**
 * Resource limits for an {@link Engine}. All fields match {@code ResourceLimits} in Rust.
 */
public record ResourceLimits(
    int maxSourceSizeBytes,
    int maxExpressionDepth,
    int maxExpressionCount,
    int maxDataValueBytes,
    int maxLoadedBytes,
    int maxSources,
    int maxNormalizedExpressionNodes,
    int maxSpecDependencyDepth,
    int maxDagSpecs,
    int maxNormalFormDepth) {}
