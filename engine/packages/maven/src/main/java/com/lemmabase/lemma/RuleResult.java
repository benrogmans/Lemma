package com.lemmabase.lemma;

import java.math.BigDecimal;
import java.util.List;
import java.util.Map;
import org.jspecify.annotations.Nullable;

/** One rule result (serde {@code RuleResult} JSON with BigDecimal magnitudes). */
public record RuleResult(
    boolean vetoed,
    @Nullable String display,
    @Nullable String vetoReason,
    String ruleType,
    @Nullable Map<String, BigDecimal> measure,
    @Nullable Map<String, BigDecimal> ratio,
    @Nullable BigDecimal number,
    @Nullable Boolean booleanValue,
    @Nullable String text,
    @Nullable Object date,
    @Nullable Object time,
    @Nullable CalendarResult calendar,
    @Nullable RangeResult range,
    @Nullable List<String> missingData,
    @Nullable Object explanation) {}
