package com.lemmabase.lemma;

/** Source location on an {@link EngineError}. */
public record EngineErrorSource(String attribute, int line, int column, int length) {}
