package com.lemmabase.lemma;

import com.fasterxml.jackson.core.JsonParser;
import com.fasterxml.jackson.core.JsonToken;
import java.io.IOException;
import java.util.Objects;

/**
 * Result of {@link Engine#install(String)}: LemmaBase repository source, not yet loaded.
 *
 * @param source Lemma source text for the repository
 * @param id repository id (e.g. {@code @iso/countries})
 */
public record RepositoryInstallResult(String source, String id) {
  /**
   * Creates an install result.
   *
   * @param source Lemma source text
   * @param id repository id
   */
  public RepositoryInstallResult {
    Objects.requireNonNull(source, "source");
    Objects.requireNonNull(id, "id");
  }

  static RepositoryInstallResult read(JsonParser p) throws IOException {
    if (p.currentToken() != JsonToken.START_OBJECT) {
      throw new LemmaBugError("BUG: expected START_OBJECT for RepositoryInstallResult");
    }
    String source = null;
    String id = null;
    boolean sourceSeen = false;
    boolean idSeen = false;
    while (p.nextToken() != JsonToken.END_OBJECT) {
      String field = p.currentName();
      p.nextToken();
      switch (field) {
        case "source" -> {
          sourceSeen = true;
          source = JsonReading.readString(p);
        }
        case "id" -> {
          idSeen = true;
          id = JsonReading.readString(p);
        }
        default -> JsonReading.unknownField(field, "RepositoryInstallResult");
      }
    }
    if (!sourceSeen) {
      JsonReading.missingRequired("source", "RepositoryInstallResult");
    }
    if (!idSeen) {
      JsonReading.missingRequired("id", "RepositoryInstallResult");
    }
    return new RepositoryInstallResult(source, id);
  }
}
