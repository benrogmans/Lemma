package com.lemmabase.lemma;

import com.fasterxml.jackson.core.JsonParser;
import com.fasterxml.jackson.core.JsonToken;
import java.io.IOException;
import org.jspecify.annotations.Nullable;

/** Structured error thrown by Engine run/show/load. */
public record EngineError(
    String kind,
    String message,
    @Nullable String relatedData,
    @Nullable String spec,
    @Nullable String relatedSpec,
    @Nullable EngineErrorSource source,
    @Nullable String suggestion,
    @Nullable String repository,
    @Nullable String registryKind,
    @Nullable String requestKind,
    @Nullable String limitName,
    @Nullable String limitValue,
    @Nullable String actualValue) {

  /** Source location attached to an {@link EngineError}. */
  public record EngineErrorSource(String attribute, int line, int column, int length) {
    static EngineErrorSource read(JsonParser p) throws IOException {
      JsonReading.expectStartObject(p, "EngineErrorSource");
      String attribute = null;
      Integer line = null;
      Integer column = null;
      Integer length = null;
      while (p.nextToken() != JsonToken.END_OBJECT) {
        String field = p.currentName();
        p.nextToken();
        switch (field) {
          case "attribute" -> attribute = JsonReading.readString(p);
          case "line" -> line = JsonReading.readInt(p);
          case "column" -> column = JsonReading.readInt(p);
          case "length" -> length = JsonReading.readInt(p);
          default -> JsonReading.unknownField(field, "EngineErrorSource");
        }
      }
      if (attribute == null) {
        JsonReading.missingRequired("attribute", "EngineErrorSource");
      }
      if (line == null) {
        JsonReading.missingRequired("line", "EngineErrorSource");
      }
      if (column == null) {
        JsonReading.missingRequired("column", "EngineErrorSource");
      }
      if (length == null) {
        JsonReading.missingRequired("length", "EngineErrorSource");
      }
      return new EngineErrorSource(attribute, line, column, length);
    }
  }

  public static EngineError read(JsonParser p) throws IOException {
    JsonReading.expectStartObject(p, "EngineError");
    String kind = null;
    String message = null;
    String relatedData = null;
    String spec = null;
    String relatedSpec = null;
    EngineErrorSource source = null;
    boolean sourceSeen = false;
    String suggestion = null;
    String repository = null;
    String registryKind = null;
    String requestKind = null;
    String limitName = null;
    String limitValue = null;
    String actualValue = null;
    boolean relatedDataSeen = false;
    boolean specSeen = false;
    boolean relatedSpecSeen = false;
    boolean suggestionSeen = false;
    boolean repositorySeen = false;
    boolean registryKindSeen = false;
    boolean requestKindSeen = false;
    boolean limitNameSeen = false;
    boolean limitValueSeen = false;
    boolean actualValueSeen = false;
    while (p.nextToken() != JsonToken.END_OBJECT) {
      String field = p.currentName();
      p.nextToken();
      switch (field) {
        case "kind" -> kind = JsonReading.readString(p);
        case "message" -> message = JsonReading.readString(p);
        case "related_data" -> {
          relatedDataSeen = true;
          relatedData = JsonReading.readString(p);
        }
        case "spec" -> {
          specSeen = true;
          spec = JsonReading.readString(p);
        }
        case "related_spec" -> {
          relatedSpecSeen = true;
          relatedSpec = JsonReading.readString(p);
        }
        case "source" -> {
          sourceSeen = true;
          source =
              p.currentToken() == JsonToken.VALUE_NULL ? null : EngineErrorSource.read(p);
        }
        case "suggestion" -> {
          suggestionSeen = true;
          suggestion = JsonReading.readString(p);
        }
        case "repository" -> {
          repositorySeen = true;
          repository = JsonReading.readString(p);
        }
        case "registry_kind" -> {
          registryKindSeen = true;
          registryKind = JsonReading.readString(p);
        }
        case "request_kind" -> {
          requestKindSeen = true;
          requestKind = JsonReading.readString(p);
        }
        case "limit_name" -> {
          limitNameSeen = true;
          limitName = JsonReading.readString(p);
        }
        case "limit_value" -> {
          limitValueSeen = true;
          limitValue = JsonReading.readString(p);
        }
        case "actual_value" -> {
          actualValueSeen = true;
          actualValue = JsonReading.readString(p);
        }
        default -> JsonReading.unknownField(field, "EngineError");
      }
    }
    if (kind == null) {
      JsonReading.missingRequired("kind", "EngineError");
    }
    if (message == null) {
      JsonReading.missingRequired("message", "EngineError");
    }
    if (!relatedDataSeen) {
      JsonReading.missingRequired("related_data", "EngineError");
    }
    if (!specSeen) {
      JsonReading.missingRequired("spec", "EngineError");
    }
    if (!relatedSpecSeen) {
      JsonReading.missingRequired("related_spec", "EngineError");
    }
    if (!sourceSeen) {
      JsonReading.missingRequired("source", "EngineError");
    }
    if (!suggestionSeen) {
      JsonReading.missingRequired("suggestion", "EngineError");
    }
    if (!repositorySeen) {
      JsonReading.missingRequired("repository", "EngineError");
    }
    if (!registryKindSeen) {
      JsonReading.missingRequired("registry_kind", "EngineError");
    }
    if (!requestKindSeen) {
      JsonReading.missingRequired("request_kind", "EngineError");
    }
    if (!limitNameSeen) {
      JsonReading.missingRequired("limit_name", "EngineError");
    }
    if (!limitValueSeen) {
      JsonReading.missingRequired("limit_value", "EngineError");
    }
    if (!actualValueSeen) {
      JsonReading.missingRequired("actual_value", "EngineError");
    }
    return new EngineError(
        kind,
        message,
        relatedData,
        spec,
        relatedSpec,
        source,
        suggestion,
        repository,
        registryKind,
        requestKind,
        limitName,
        limitValue,
        actualValue);
  }
}
