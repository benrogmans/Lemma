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

  /**
   * Creates a run request for one spec with empty data.
   *
   * @param spec spec name
   * @return new request
   */
  public static RunRequest of(String spec) {
    return new RunRequest(spec, null, null, Map.of(), null, false);
  }

  /**
   * Sets the repository handle.
   *
   * @param repository repository handle; {@code null} for default
   * @return copy with updated repository
   */
  public RunRequest repository(@Nullable String repository) {
    return new RunRequest(spec, repository, effective, data, rules, explain);
  }

  /**
   * Sets the effective date.
   *
   * @param effective effective date; {@code null} for latest
   * @return copy with updated effective date
   */
  public RunRequest effective(@Nullable String effective) {
    return new RunRequest(spec, repository, effective, data, rules, explain);
  }

  /**
   * Sets input data bindings.
   *
   * @param data data map
   * @return copy with updated data
   */
  public RunRequest data(Map<String, ?> data) {
    return new RunRequest(spec, repository, effective, data, rules, explain);
  }

  /**
   * Sets the rule subset to evaluate.
   *
   * @param rules rule names; {@code null} for all rules
   * @return copy with updated rules
   */
  public RunRequest rules(@Nullable List<String> rules) {
    return new RunRequest(spec, repository, effective, data, rules, explain);
  }

  /**
   * Sets explain mode.
   *
   * @param explain whether to include explanations
   * @return copy with updated explain flag
   */
  public RunRequest explain(boolean explain) {
    return new RunRequest(spec, repository, effective, data, rules, explain);
  }

  /** Spec name.
   *
   * @return spec name
   */
  public String spec() {
    return spec;
  }

  /** Repository handle.
   *
   * @return repository handle or null
   */
  public @Nullable String repository() {
    return repository;
  }

  /** Effective date.
   *
   * @return effective date or null
   */
  public @Nullable String effective() {
    return effective;
  }

  /** Input data bindings.
   *
   * @return data map
   */
  public Map<String, ?> data() {
    return data;
  }

  /** Rule subset; {@code null} means all rules.
   *
   * @return rule names or null
   */
  public @Nullable List<String> rules() {
    return rules;
  }

  /** Whether explain mode is enabled.
   *
   * @return explain flag
   */
  public boolean explain() {
    return explain;
  }
}
