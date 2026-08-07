package com.lemmabase.lemma;

import com.fasterxml.jackson.core.JsonParser;
import java.io.IOException;
import java.util.List;

final class JsonSupport {
  private JsonSupport() {}

  static List<EngineError> parseEngineErrors(String json) {
    try (JsonParser p = JsonReading.parserFor(json)) {
      return JsonReading.readList(p, EngineError::read);
    } catch (IOException e) {
      throw new LemmaBugError("BUG: failed to parse EngineError JSON: " + e.getMessage());
    }
  }

  static Response parseResponse(String json) {
    try (JsonParser p = JsonReading.parserFor(json)) {
      return Response.read(p);
    } catch (IOException e) {
      throw new LemmaBugError("BUG: failed to parse Response JSON: " + e.getMessage());
    }
  }

  static Show parseShow(String json) {
    try (JsonParser p = JsonReading.parserFor(json)) {
      return Show.read(p);
    } catch (IOException e) {
      throw new LemmaBugError("BUG: failed to parse Show JSON: " + e.getMessage());
    }
  }

  static List<ResolvedRepository> parseList(String json) {
    try (JsonParser p = JsonReading.parserFor(json)) {
      return JsonReading.readList(p, ResolvedRepository::read);
    } catch (IOException e) {
      throw new LemmaBugError("BUG: failed to parse list JSON: " + e.getMessage());
    }
  }

  static ResourceLimits parseLimits(String json) {
    try (JsonParser p = JsonReading.parserFor(json)) {
      return ResourceLimits.read(p);
    } catch (IOException e) {
      throw new LemmaBugError("BUG: failed to parse ResourceLimits JSON: " + e.getMessage());
    }
  }

  static List<Recommendation> parseRecommendations(String json) {
    try (JsonParser p = JsonReading.parserFor(json)) {
      return JsonReading.readList(p, Recommendation::read);
    } catch (IOException e) {
      throw new LemmaBugError("BUG: failed to parse Recommendation JSON: " + e.getMessage());
    }
  }

  static String limitsToJson(ResourceLimits limits) {
    return limits.toJson();
  }
}
