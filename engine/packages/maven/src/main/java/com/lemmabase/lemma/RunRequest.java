package com.lemmabase.lemma;

import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import org.jspecify.annotations.Nullable;

/**
 * Named arguments for {@link Engine#run(RunRequest)}. Defaults: repository null, effective null,
 * rules null (all rules), explain false.
 */
public final class RunRequest {
  private final String spec;
  private final @Nullable String repository;
  private final @Nullable String effective;
  private final Map<String, ?> data;
  private final @Nullable List<String> rules;
  private final boolean explain;

  private RunRequest(
      String spec,
      @Nullable String repository,
      @Nullable String effective,
      Map<String, ?> data,
      @Nullable List<String> rules,
      boolean explain) {
    this.spec = Objects.requireNonNull(spec, "spec");
    this.repository = repository;
    this.effective = effective;
    // Not Map.copyOf: callers may pass a null value (e.g. an explicit override probe) and
    // must get back an attributed LemmaException from RunDataValues.toEngineStrings, not a bare
    // NullPointerException from the immutable-map constructor.
    this.data = Collections.unmodifiableMap(new LinkedHashMap<>(Objects.requireNonNull(data, "data")));
    this.rules = rules == null ? null : List.copyOf(rules);
    this.explain = explain;
  }

  public static RunRequest of(String spec) {
    return new RunRequest(spec, null, null, Map.of(), null, false);
  }

  public RunRequest repository(@Nullable String repository) {
    return new RunRequest(spec, repository, effective, data, rules, explain);
  }

  public RunRequest effective(@Nullable String effective) {
    return new RunRequest(spec, repository, effective, data, rules, explain);
  }

  public RunRequest data(Map<String, ?> data) {
    return new RunRequest(spec, repository, effective, data, rules, explain);
  }

  public RunRequest rules(@Nullable List<String> rules) {
    return new RunRequest(spec, repository, effective, data, rules, explain);
  }

  public RunRequest explain(boolean explain) {
    return new RunRequest(spec, repository, effective, data, rules, explain);
  }

  public String spec() {
    return spec;
  }

  public @Nullable String repository() {
    return repository;
  }

  public @Nullable String effective() {
    return effective;
  }

  public Map<String, ?> data() {
    return data;
  }

  public @Nullable List<String> rules() {
    return rules;
  }

  public boolean explain() {
    return explain;
  }
}
