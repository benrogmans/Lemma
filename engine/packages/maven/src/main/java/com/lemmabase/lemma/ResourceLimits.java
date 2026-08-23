package com.lemmabase.lemma;

import com.fasterxml.jackson.core.JsonParser;
import com.fasterxml.jackson.core.JsonToken;
import java.io.IOException;
/**
 * ResourceLimits.
 * @param maxSourceSizeBytes maxSourceSizeBytes
 * @param maxExpressionDepth maxExpressionDepth
 * @param maxExpressionCount maxExpressionCount
 * @param maxDataValueBytes maxDataValueBytes
 * @param maxLoadedBytes maxLoadedBytes
 * @param maxSources maxSources
 * @param maxNormalizedExpressionNodes maxNormalizedExpressionNodes
 * @param maxSpecDependencyDepth maxSpecDependencyDepth
 * @param maxDagSpecs maxDagSpecs
 * @param maxNormalFormDepth maxNormalFormDepth
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
   * Parses JSON.
   *
   * @param p parser at value start
   * @return parsed value
   * @throws IOException if JSON IO fails
   */
  public static ResourceLimits read(JsonParser p) throws IOException {
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
}
