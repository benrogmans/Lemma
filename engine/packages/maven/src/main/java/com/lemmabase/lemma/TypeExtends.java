package com.lemmabase.lemma;

import com.fasterxml.jackson.core.JsonParser;
import com.fasterxml.jackson.core.JsonToken;
import java.io.IOException;

/** What a type extends: a primitive built-in, or a custom type by name. */
public sealed interface TypeExtends {
  String kind();

  record Primitive() implements TypeExtends {
    @Override
    public String kind() {
      return "primitive";
    }
  }

  record Custom(String parent, String family, TypeDefiningSpec definingSpec) implements TypeExtends {
    @Override
    public String kind() {
      return "custom";
    }

    static Custom read(JsonParser p) throws IOException {
      JsonReading.expectStartObject(p, "TypeExtends.Custom");
      String parent = null;
      String family = null;
      TypeDefiningSpec definingSpec = null;
      while (p.nextToken() != JsonToken.END_OBJECT) {
        String field = p.currentName();
        p.nextToken();
        switch (field) {
          case "kind" -> {
            String kind = JsonReading.readString(p);
            if (!"custom".equals(kind)) {
              throw new LemmaBugError("BUG: expected kind 'custom', got '" + kind + "'");
            }
          }
          case "parent" -> parent = JsonReading.readString(p);
          case "family" -> family = JsonReading.readString(p);
          case "defining_spec" -> definingSpec = TypeDefiningSpec.read(p);
          default -> JsonReading.unknownField(field, "TypeExtends.Custom");
        }
      }
      if (parent == null) {
        JsonReading.missingRequired("parent", "TypeExtends.Custom");
      }
      if (family == null) {
        JsonReading.missingRequired("family", "TypeExtends.Custom");
      }
      if (definingSpec == null) {
        JsonReading.missingRequired("defining_spec", "TypeExtends.Custom");
      }
      return new Custom(parent, family, definingSpec);
    }
  }

  /** Where a custom type's extension chain is rooted. */
  public sealed interface TypeDefiningSpec {
    String kind();

    record Local() implements TypeDefiningSpec {
      @Override
      public String kind() {
        return "local";
      }
    }

    record Import() implements TypeDefiningSpec {
      @Override
      public String kind() {
        return "import";
      }
    }

    static TypeDefiningSpec read(JsonParser p) throws IOException {
      JsonReading.expectStartObject(p, "TypeDefiningSpec");
      String json = JsonReading.bufferObjectAsString(p);
      String kind = JsonReading.findTag(json, "kind");
      if (kind == null) {
        throw new LemmaBugError("BUG: missing 'kind' in TypeDefiningSpec");
      }
      try (JsonParser reader = JsonReading.parserFor(json)) {
        return switch (kind) {
          case "local" -> {
            consumeUnitObject(reader, "local", "TypeDefiningSpec.Local");
            yield new Local();
          }
          case "import" -> {
            consumeUnitObject(reader, "import", "TypeDefiningSpec.Import");
            yield new Import();
          }
          default -> throw new LemmaBugError("BUG: unknown kind value: " + kind);
        };
      }
    }

    private static void consumeUnitObject(JsonParser p, String expectedKind, String typeName)
        throws IOException {
      JsonReading.expectStartObject(p, typeName);
      while (p.nextToken() != JsonToken.END_OBJECT) {
        String field = p.currentName();
        p.nextToken();
        if ("kind".equals(field)) {
          String kind = JsonReading.readString(p);
          if (!expectedKind.equals(kind)) {
            throw new LemmaBugError("BUG: expected kind '" + expectedKind + "', got '" + kind + "'");
          }
        } else {
          JsonReading.unknownField(field, typeName);
        }
      }
    }
  }

  static TypeExtends read(JsonParser p) throws IOException {
    JsonReading.expectStartObject(p, "TypeExtends");
    String json = JsonReading.bufferObjectAsString(p);
    String kind = JsonReading.findTag(json, "kind");
    if (kind == null) {
      throw new LemmaBugError("BUG: missing 'kind' in TypeExtends");
    }
    try (JsonParser reader = JsonReading.parserFor(json)) {
      return switch (kind) {
        case "primitive" -> {
          TypeDefiningSpec.consumeUnitObject(reader, "primitive", "TypeExtends.Primitive");
          yield new Primitive();
        }
        case "custom" -> Custom.read(reader);
        default -> throw new LemmaBugError("BUG: unknown kind value: " + kind);
      };
    }
  }
}
