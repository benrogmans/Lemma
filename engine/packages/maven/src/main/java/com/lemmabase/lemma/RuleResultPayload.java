package com.lemmabase.lemma;

import java.math.BigDecimal;
import java.util.Map;
import org.jspecify.annotations.Nullable;

/** Payload for one range endpoint. */
public record RuleResultPayload(
    @Nullable Map<String, BigDecimal> measure,
    @Nullable Map<String, BigDecimal> ratio,
    @Nullable BigDecimal number,
    @Nullable Boolean booleanValue,
    @Nullable String text,
    @Nullable Object date,
    @Nullable Object time,
    @Nullable CalendarResult calendar) {}
