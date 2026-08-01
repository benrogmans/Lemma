package com.lemmabase.lemma;

import com.fasterxml.jackson.core.JsonParser;
import com.fasterxml.jackson.core.JsonToken;
import java.io.IOException;
import java.util.List;
import org.jspecify.annotations.Nullable;

/** One repository group from {@link Engine#list()}. */
public record ResolvedRepository(@Nullable String repository, List<ListedSpec> specs) {

  /** One listed spec row. */
  public record ListedSpec(
      String name, @Nullable String effectiveFrom, @Nullable String effectiveTo) {
    static ListedSpec read(JsonParser p) throws IOException {
      JsonReading.expectStartObject(p, "ListedSpec");
      String name = null;
      String effectiveFrom = null;
      String effectiveTo = null;
      while (p.nextToken() != JsonToken.END_OBJECT) {
        String field = p.currentName();
        p.nextToken();
        switch (field) {
          case "name" -> name = JsonReading.readString(p);
          case "effective_from" -> effectiveFrom = JsonReading.readString(p);
          case "effective_to" -> effectiveTo = JsonReading.readString(p);
          default -> JsonReading.unknownField(field, "ListedSpec");
        }
      }
      if (name == null) {
        JsonReading.missingRequired("name", "ListedSpec");
      }
      return new ListedSpec(name, effectiveFrom, effectiveTo);
    }
  }

  public static ResolvedRepository read(JsonParser p) throws IOException {
    JsonReading.expectStartObject(p, "ResolvedRepository");
    String repository = null;
    List<ListedSpec> specs = null;
    while (p.nextToken() != JsonToken.END_OBJECT) {
      String field = p.currentName();
      p.nextToken();
      switch (field) {
        case "repository" -> repository = JsonReading.readString(p);
        case "specs" -> specs = JsonReading.readList(p, ListedSpec::read);
        default -> JsonReading.unknownField(field, "ResolvedRepository");
      }
    }
    if (specs == null) {
      JsonReading.missingRequired("specs", "ResolvedRepository");
    }
    return new ResolvedRepository(repository, specs);
  }
}
