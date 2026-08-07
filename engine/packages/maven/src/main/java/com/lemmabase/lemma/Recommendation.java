package com.lemmabase.lemma;

import com.fasterxml.jackson.core.JsonParser;
import com.fasterxml.jackson.core.JsonToken;
import java.io.IOException;
import org.jspecify.annotations.Nullable;

/** Structural quality recommendation from {@link Engine#quality()}. Advisory only. */
public record Recommendation(
    String message,
    String spec,
    @Nullable String effectiveFrom,
    @Nullable String repository,
    EngineError.EngineErrorSource source) {

  static Recommendation read(JsonParser p) throws IOException {
    JsonReading.expectStartObject(p, "Recommendation");
    String message = null;
    String spec = null;
    String effectiveFrom = null;
    boolean effectiveFromSeen = false;
    String repository = null;
    boolean repositorySeen = false;
    EngineError.EngineErrorSource source = null;
    while (p.nextToken() != JsonToken.END_OBJECT) {
      String field = p.currentName();
      p.nextToken();
      switch (field) {
        case "message" -> message = JsonReading.readString(p);
        case "spec" -> spec = JsonReading.readString(p);
        case "effective_from" -> {
          effectiveFromSeen = true;
          effectiveFrom =
              p.currentToken() == JsonToken.VALUE_NULL ? null : JsonReading.readString(p);
        }
        case "repository" -> {
          repositorySeen = true;
          repository =
              p.currentToken() == JsonToken.VALUE_NULL ? null : JsonReading.readString(p);
        }
        case "source" -> source = EngineError.EngineErrorSource.read(p);
        default -> JsonReading.unknownField(field, "Recommendation");
      }
    }
    if (message == null) {
      JsonReading.missingRequired("message", "Recommendation");
    }
    if (spec == null) {
      JsonReading.missingRequired("spec", "Recommendation");
    }
    if (!effectiveFromSeen) {
      JsonReading.missingRequired("effective_from", "Recommendation");
    }
    if (!repositorySeen) {
      JsonReading.missingRequired("repository", "Recommendation");
    }
    if (source == null) {
      JsonReading.missingRequired("source", "Recommendation");
    }
    return new Recommendation(message, spec, effectiveFrom, repository, source);
  }
}
