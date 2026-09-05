package com.lemmabase.lemma;

import com.fasterxml.jackson.core.JsonParser;
import com.fasterxml.jackson.core.JsonToken;
import java.io.IOException;
import org.jspecify.annotations.Nullable;

/**
 * Complete resource-limit snapshot from {@link Engine#limits()}. Defaults live in the engine; do
 * not construct this with hardcoded magnitudes. Use {@link #builder()} for named overrides at
 * create time.
 *
 * @param maxSourceSizeBytes maximum size of one loaded source text in bytes
 * @param maxExpressionDepth maximum expression nesting depth
 * @param maxExpressionCount maximum expression nodes per source (parser-level)
 * @param maxDataValueBytes maximum size of a single data value in bytes
 * @param maxLoadedBytes maximum total bytes to load in one batch
 * @param maxSources maximum number of sources in one load batch
 * @param maxNormalizedExpressionNodes maximum unique normal-form cells reachable from one rule root
 * @param maxSpecDependencyDepth maximum depth of the spec dependency chain
 * @param maxDagSpecs maximum number of specs in one dependency DAG
 * @param maxNormalFormDepth maximum nesting depth of a rule's normalized NormalForm DAG
 */
public record ResourceLimits(
    long maxSourceSizeBytes,
    long maxExpressionDepth,
    long maxExpressionCount,
    long maxDataValueBytes,
    long maxLoadedBytes,
    long maxSources,
    long maxNormalizedExpressionNodes,
    long maxSpecDependencyDepth,
    long maxDagSpecs,
    long maxNormalFormDepth) {

  /**
   * Named overrides for {@link Engine#create(Builder)}. Unset keys keep engine defaults.
   *
   * @return empty builder
   */
  public static Builder builder() {
    return new Builder();
  }

  /**
   * Parses JSON.
   *
   * @param p parser at value start
   * @return parsed value
   * @throws IOException if JSON IO fails
   */
  static ResourceLimits read(JsonParser p) throws IOException {
    if (p.currentToken() != JsonToken.START_OBJECT) {
      throw new LemmaBugError("BUG: expected START_OBJECT for ResourceLimits");
    }
    Long maxSourceSizeBytes = null;
    Long maxExpressionDepth = null;
    Long maxExpressionCount = null;
    Long maxDataValueBytes = null;
    Long maxLoadedBytes = null;
    Long maxSources = null;
    Long maxNormalizedExpressionNodes = null;
    Long maxSpecDependencyDepth = null;
    Long maxDagSpecs = null;
    Long maxNormalFormDepth = null;
    while (p.nextToken() != JsonToken.END_OBJECT) {
      String field = p.currentName();
      p.nextToken();
      switch (field) {
        case "max_source_size_bytes" -> maxSourceSizeBytes = p.getLongValue();
        case "max_expression_depth" -> maxExpressionDepth = p.getLongValue();
        case "max_expression_count" -> maxExpressionCount = p.getLongValue();
        case "max_data_value_bytes" -> maxDataValueBytes = p.getLongValue();
        case "max_loaded_bytes" -> maxLoadedBytes = p.getLongValue();
        case "max_sources" -> maxSources = p.getLongValue();
        case "max_normalized_expression_nodes" -> maxNormalizedExpressionNodes = p.getLongValue();
        case "max_spec_dependency_depth" -> maxSpecDependencyDepth = p.getLongValue();
        case "max_dag_specs" -> maxDagSpecs = p.getLongValue();
        case "max_normal_form_depth" -> maxNormalFormDepth = p.getLongValue();
        default -> JsonReading.unknownField(field, "ResourceLimits");
      }
    }
    if (maxSourceSizeBytes == null) {
      JsonReading.missingRequired("max_source_size_bytes", "ResourceLimits");
    }
    if (maxExpressionDepth == null) {
      JsonReading.missingRequired("max_expression_depth", "ResourceLimits");
    }
    if (maxExpressionCount == null) {
      JsonReading.missingRequired("max_expression_count", "ResourceLimits");
    }
    if (maxDataValueBytes == null) {
      JsonReading.missingRequired("max_data_value_bytes", "ResourceLimits");
    }
    if (maxLoadedBytes == null) {
      JsonReading.missingRequired("max_loaded_bytes", "ResourceLimits");
    }
    if (maxSources == null) {
      JsonReading.missingRequired("max_sources", "ResourceLimits");
    }
    if (maxNormalizedExpressionNodes == null) {
      JsonReading.missingRequired("max_normalized_expression_nodes", "ResourceLimits");
    }
    if (maxSpecDependencyDepth == null) {
      JsonReading.missingRequired("max_spec_dependency_depth", "ResourceLimits");
    }
    if (maxDagSpecs == null) {
      JsonReading.missingRequired("max_dag_specs", "ResourceLimits");
    }
    if (maxNormalFormDepth == null) {
      JsonReading.missingRequired("max_normal_form_depth", "ResourceLimits");
    }
    return new ResourceLimits(
        maxSourceSizeBytes,
        maxExpressionDepth,
        maxExpressionCount,
        maxDataValueBytes,
        maxLoadedBytes,
        maxSources,
        maxNormalizedExpressionNodes,
        maxSpecDependencyDepth,
        maxDagSpecs,
        maxNormalFormDepth);
  }

  /**
   * Serializes limits to JSON for JNI.
   *
   * @return JSON object text
   */
  public String toJson() {
    return "{\"max_source_size_bytes\":"
        + maxSourceSizeBytes
        + ",\"max_expression_depth\":"
        + maxExpressionDepth
        + ",\"max_expression_count\":"
        + maxExpressionCount
        + ",\"max_data_value_bytes\":"
        + maxDataValueBytes
        + ",\"max_loaded_bytes\":"
        + maxLoadedBytes
        + ",\"max_sources\":"
        + maxSources
        + ",\"max_normalized_expression_nodes\":"
        + maxNormalizedExpressionNodes
        + ",\"max_spec_dependency_depth\":"
        + maxSpecDependencyDepth
        + ",\"max_dag_specs\":"
        + maxDagSpecs
        + ",\"max_normal_form_depth\":"
        + maxNormalFormDepth
        + "}";
  }

  /**
   * Named limit overrides for engine creation. Only set keys are sent to JNI; unset keys keep
   * engine defaults.
   */
  public static final class Builder {
    private @Nullable Long maxSourceSizeBytes;
    private @Nullable Long maxExpressionDepth;
    private @Nullable Long maxExpressionCount;
    private @Nullable Long maxDataValueBytes;
    private @Nullable Long maxLoadedBytes;
    private @Nullable Long maxSources;
    private @Nullable Long maxNormalizedExpressionNodes;
    private @Nullable Long maxSpecDependencyDepth;
    private @Nullable Long maxDagSpecs;
    private @Nullable Long maxNormalFormDepth;

    private Builder() {}

    /**
     * Sets maximum size of one loaded source text in bytes.
     *
     * @param value override value
     * @return this builder
     */
    public Builder maxSourceSizeBytes(long value) {
      this.maxSourceSizeBytes = value;
      return this;
    }

    /**
     * Sets maximum expression nesting depth.
     *
     * @param value override value
     * @return this builder
     */
    public Builder maxExpressionDepth(long value) {
      this.maxExpressionDepth = value;
      return this;
    }

    /**
     * Sets maximum expression nodes per source (parser-level).
     *
     * @param value override value
     * @return this builder
     */
    public Builder maxExpressionCount(long value) {
      this.maxExpressionCount = value;
      return this;
    }

    /**
     * Sets maximum size of a single data value in bytes.
     *
     * @param value override value
     * @return this builder
     */
    public Builder maxDataValueBytes(long value) {
      this.maxDataValueBytes = value;
      return this;
    }

    /**
     * Sets maximum total bytes to load in one batch.
     *
     * @param value override value
     * @return this builder
     */
    public Builder maxLoadedBytes(long value) {
      this.maxLoadedBytes = value;
      return this;
    }

    /**
     * Sets maximum number of sources in one load batch.
     *
     * @param value override value
     * @return this builder
     */
    public Builder maxSources(long value) {
      this.maxSources = value;
      return this;
    }

    /**
     * Sets maximum unique normal-form cells reachable from one rule root.
     *
     * @param value override value
     * @return this builder
     */
    public Builder maxNormalizedExpressionNodes(long value) {
      this.maxNormalizedExpressionNodes = value;
      return this;
    }

    /**
     * Sets maximum depth of the spec dependency chain.
     *
     * @param value override value
     * @return this builder
     */
    public Builder maxSpecDependencyDepth(long value) {
      this.maxSpecDependencyDepth = value;
      return this;
    }

    /**
     * Sets maximum number of specs in one dependency DAG.
     *
     * @param value override value
     * @return this builder
     */
    public Builder maxDagSpecs(long value) {
      this.maxDagSpecs = value;
      return this;
    }

    /**
     * Sets maximum nesting depth of a rule's normalized NormalForm DAG.
     *
     * @param value override value
     * @return this builder
     */
    public Builder maxNormalFormDepth(long value) {
      this.maxNormalFormDepth = value;
      return this;
    }

    /**
     * Serializes set overrides only for JNI.
     *
     * @return JSON object with only non-null keys; empty object when nothing set
     */
    String toJson() {
      StringBuilder json = new StringBuilder("{");
      boolean first = true;
      first = append(json, first, "max_source_size_bytes", maxSourceSizeBytes);
      first = append(json, first, "max_expression_depth", maxExpressionDepth);
      first = append(json, first, "max_expression_count", maxExpressionCount);
      first = append(json, first, "max_data_value_bytes", maxDataValueBytes);
      first = append(json, first, "max_loaded_bytes", maxLoadedBytes);
      first = append(json, first, "max_sources", maxSources);
      first = append(json, first, "max_normalized_expression_nodes", maxNormalizedExpressionNodes);
      first = append(json, first, "max_spec_dependency_depth", maxSpecDependencyDepth);
      first = append(json, first, "max_dag_specs", maxDagSpecs);
      append(json, first, "max_normal_form_depth", maxNormalFormDepth);
      json.append('}');
      return json.toString();
    }

    private static boolean append(
        StringBuilder json, boolean first, String key, @Nullable Long value) {
      if (value == null) {
        return first;
      }
      if (!first) {
        json.append(',');
      }
      json.append('"').append(key).append("\":").append(value);
      return false;
    }
  }
}
