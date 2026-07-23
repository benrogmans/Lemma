package com.lemmabase.lemma;

import com.fasterxml.jackson.annotation.JsonIgnoreProperties;
import com.fasterxml.jackson.annotation.JsonProperty;
import com.fasterxml.jackson.core.type.TypeReference;
import com.fasterxml.jackson.databind.DeserializationFeature;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.PropertyNamingStrategies;
import com.fasterxml.jackson.databind.module.SimpleModule;
import java.math.BigDecimal;
import java.util.List;
import java.util.Map;

final class JsonSupport {
  private static final ObjectMapper MAPPER = createMapper();

  private JsonSupport() {}

  private static ObjectMapper createMapper() {
    ObjectMapper mapper = new ObjectMapper();
    mapper.setPropertyNamingStrategy(PropertyNamingStrategies.SNAKE_CASE);
    mapper.configure(DeserializationFeature.FAIL_ON_UNKNOWN_PROPERTIES, false);
    SimpleModule module = new SimpleModule();
    module.addDeserializer(BigDecimal.class, new BigDecimalPlainDeserializer());
    mapper.registerModule(module);
    mapper.addMixIn(RuleResult.class, RuleResultMixin.class);
    mapper.addMixIn(RuleResultPayload.class, RuleResultPayloadMixin.class);
    mapper.addMixIn(EngineError.class, EngineErrorMixin.class);
    return mapper;
  }

  static List<EngineError> parseEngineErrors(String json) {
    try {
      return MAPPER.readValue(json, new TypeReference<List<EngineError>>() {});
    } catch (Exception e) {
      throw new LemmaBugError("BUG: failed to parse EngineError JSON: " + e.getMessage());
    }
  }

  static Response parseResponse(String json) {
    try {
      return MAPPER.readValue(json, Response.class);
    } catch (Exception e) {
      throw new LemmaBugError("BUG: failed to parse Response JSON: " + e.getMessage());
    }
  }

  static Show parseShow(String json) {
    try {
      return MAPPER.readValue(json, Show.class);
    } catch (Exception e) {
      throw new LemmaBugError("BUG: failed to parse Show JSON: " + e.getMessage());
    }
  }

  static List<ResolvedRepository> parseList(String json) {
    try {
      return MAPPER.readValue(json, new TypeReference<List<ResolvedRepository>>() {});
    } catch (Exception e) {
      throw new LemmaBugError("BUG: failed to parse list JSON: " + e.getMessage());
    }
  }

  static ResourceLimits parseLimits(String json) {
    try {
      return MAPPER.readValue(json, ResourceLimits.class);
    } catch (Exception e) {
      throw new LemmaBugError("BUG: failed to parse ResourceLimits JSON: " + e.getMessage());
    }
  }

  static String limitsToJson(ResourceLimits limits) {
    try {
      return MAPPER.writeValueAsString(limits);
    } catch (Exception e) {
      throw new LemmaBugError("BUG: failed to serialize ResourceLimits: " + e.getMessage());
    }
  }

  @JsonIgnoreProperties(ignoreUnknown = true)
  private abstract static class RuleResultMixin {
    @JsonProperty("boolean")
    abstract Boolean booleanValue();
  }

  @JsonIgnoreProperties(ignoreUnknown = true)
  private abstract static class RuleResultPayloadMixin {
    @JsonProperty("boolean")
    abstract Boolean booleanValue();
  }

  @JsonIgnoreProperties(ignoreUnknown = true)
  private abstract static class EngineErrorMixin {
    @JsonProperty("related_data")
    abstract String relatedData();

    @JsonProperty("related_spec")
    abstract String relatedSpec();
  }
}
