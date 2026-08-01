package com.lemmabase.lemma;

import com.fasterxml.jackson.core.JsonParser;
import com.fasterxml.jackson.core.JsonToken;
import java.io.IOException;

/** Provenance of a loaded source. Externally tagged; unit {@code Volatile} is bare {@code "volatile"}. */
public sealed interface SourceType {

  record Volatile() implements SourceType {}

  record Path(String value) implements SourceType {}

  record Dependency(String value) implements SourceType {}

  static SourceType read(JsonParser p) throws IOException {
    if (p.currentToken() == JsonToken.VALUE_STRING) {
      String value = p.getText();
      if ("volatile".equals(value)) {
        return new Volatile();
      }
      throw new LemmaBugError("BUG: unknown string value '" + value + "' in SourceType");
    }
    JsonReading.expectStartObject(p, "SourceType");
    p.nextToken();
    if (p.currentToken() == JsonToken.END_OBJECT) {
      throw new LemmaBugError("BUG: empty object for SourceType");
    }
    String tag = p.currentName();
    p.nextToken();
    SourceType result =
        switch (tag) {
          case "path" -> new Path(JsonReading.readString(p));
          case "dependency" -> new Dependency(JsonReading.readString(p));
          default -> throw new LemmaBugError("BUG: unknown tag '" + tag + "' in SourceType");
        };
    p.nextToken();
    if (p.currentToken() != JsonToken.END_OBJECT) {
      throw new LemmaBugError("BUG: expected END_OBJECT for SourceType");
    }
    return result;
  }
}
