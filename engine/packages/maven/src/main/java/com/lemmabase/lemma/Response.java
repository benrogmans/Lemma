package com.lemmabase.lemma;

import java.util.List;
import java.util.Map;
import org.jspecify.annotations.Nullable;

/** Evaluation response (serde {@code Response} JSON). */
public record Response(
    String spec,
    String effective,
    @Nullable String specHash,
    @Nullable String specEffectiveFrom,
    @Nullable String specEffectiveTo,
    Map<String, RuleResult> results) {}
