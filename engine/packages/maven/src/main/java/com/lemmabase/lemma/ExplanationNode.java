package com.lemmabase.lemma;

import com.fasterxml.jackson.core.JsonParser;
import com.fasterxml.jackson.core.JsonToken;
import java.io.IOException;
import java.util.List;
import org.jspecify.annotations.Nullable;

/** Nested explanation tree node (tagged by {@code type}). */
public sealed interface ExplanationNode {
  String type();

  /** One evaluated unless condition, stated as a fact. */
  record Cause(String condition, String value, @Nullable List<ExplanationNode> children) {
    static Cause read(JsonParser p) throws IOException {
      JsonReading.expectStartObject(p, "Cause");
      String condition = null;
      String value = null;
      List<ExplanationNode> children = null;
      while (p.nextToken() != JsonToken.END_OBJECT) {
        String field = p.currentName();
        p.nextToken();
        switch (field) {
          case "condition" -> condition = JsonReading.readString(p);
          case "value" -> value = JsonReading.readString(p);
          case "children" -> children = JsonReading.readList(p, ExplanationNode::read);
          default -> JsonReading.unknownField(field, "Cause");
        }
      }
      if (condition == null) {
        JsonReading.missingRequired("condition", "Cause");
      }
      if (value == null) {
        JsonReading.missingRequired("value", "Cause");
      }
      return new Cause(condition, value, children);
    }
  }

  record ConversionStep(String role, String text) {
    static ConversionStep read(JsonParser p) throws IOException {
      JsonReading.expectStartObject(p, "ConversionStep");
      String role = null;
      String text = null;
      while (p.nextToken() != JsonToken.END_OBJECT) {
        String field = p.currentName();
        p.nextToken();
        switch (field) {
          case "role" -> role = JsonReading.readString(p);
          case "text" -> text = JsonReading.readString(p);
          default -> JsonReading.unknownField(field, "ConversionStep");
        }
      }
      if (role == null) {
        JsonReading.missingRequired("role", "ConversionStep");
      }
      if (text == null) {
        JsonReading.missingRequired("text", "ConversionStep");
      }
      if (!("outcome".equals(role) || "rule".equals(role) || "source".equals(role))) {
        throw new LemmaBugError("BUG: invalid ConversionStep role '" + role + "'");
      }
      return new ConversionStep(role, text);
    }
  }

  record Rule(
      String name,
      String result,
      String body,
      @Nullable List<Cause> causes,
      @Nullable List<ExplanationNode> children)
      implements ExplanationNode {
    @Override
    public String type() {
      return "rule";
    }

    static Rule read(JsonParser p) throws IOException {
      JsonReading.expectStartObject(p, "ExplanationNode.Rule");
      String name = null;
      String result = null;
      String body = null;
      List<Cause> causes = null;
      List<ExplanationNode> children = null;
      while (p.nextToken() != JsonToken.END_OBJECT) {
        String field = p.currentName();
        p.nextToken();
        switch (field) {
          case "type" -> expectType(p, "rule");
          case "name" -> name = JsonReading.readString(p);
          case "result" -> result = JsonReading.readString(p);
          case "body" -> body = JsonReading.readString(p);
          case "causes" -> causes = JsonReading.readList(p, Cause::read);
          case "children" -> children = JsonReading.readList(p, ExplanationNode::read);
          default -> JsonReading.unknownField(field, "ExplanationNode.Rule");
        }
      }
      if (name == null) {
        JsonReading.missingRequired("name", "ExplanationNode.Rule");
      }
      if (result == null) {
        JsonReading.missingRequired("result", "ExplanationNode.Rule");
      }
      if (body == null) {
        JsonReading.missingRequired("body", "ExplanationNode.Rule");
      }
      return new Rule(name, result, body, causes, children);
    }
  }

  record Compose(String expression, List<ExplanationNode> operands) implements ExplanationNode {
    @Override
    public String type() {
      return "compose";
    }

    static Compose read(JsonParser p) throws IOException {
      JsonReading.expectStartObject(p, "ExplanationNode.Compose");
      String expression = null;
      List<ExplanationNode> operands = null;
      while (p.nextToken() != JsonToken.END_OBJECT) {
        String field = p.currentName();
        p.nextToken();
        switch (field) {
          case "type" -> expectType(p, "compose");
          case "expression" -> expression = JsonReading.readString(p);
          case "operands" -> operands = JsonReading.readList(p, ExplanationNode::read);
          default -> JsonReading.unknownField(field, "ExplanationNode.Compose");
        }
      }
      if (expression == null) {
        JsonReading.missingRequired("expression", "ExplanationNode.Compose");
      }
      if (operands == null) {
        JsonReading.missingRequired("operands", "ExplanationNode.Compose");
      }
      return new Compose(expression, operands);
    }
  }

  record Data(String name, String display) implements ExplanationNode {
    @Override
    public String type() {
      return "data";
    }

    static Data read(JsonParser p) throws IOException {
      JsonReading.expectStartObject(p, "ExplanationNode.Data");
      String name = null;
      String display = null;
      while (p.nextToken() != JsonToken.END_OBJECT) {
        String field = p.currentName();
        p.nextToken();
        switch (field) {
          case "type" -> expectType(p, "data");
          case "name" -> name = JsonReading.readString(p);
          case "display" -> display = JsonReading.readString(p);
          default -> JsonReading.unknownField(field, "ExplanationNode.Data");
        }
      }
      if (name == null) {
        JsonReading.missingRequired("name", "ExplanationNode.Data");
      }
      if (display == null) {
        JsonReading.missingRequired("display", "ExplanationNode.Data");
      }
      return new Data(name, display);
    }
  }

  record DataUnused(String name) implements ExplanationNode {
    @Override
    public String type() {
      return "data_unused";
    }

    static DataUnused read(JsonParser p) throws IOException {
      JsonReading.expectStartObject(p, "ExplanationNode.DataUnused");
      String name = null;
      while (p.nextToken() != JsonToken.END_OBJECT) {
        String field = p.currentName();
        p.nextToken();
        switch (field) {
          case "type" -> expectType(p, "data_unused");
          case "name" -> name = JsonReading.readString(p);
          default -> JsonReading.unknownField(field, "ExplanationNode.DataUnused");
        }
      }
      if (name == null) {
        JsonReading.missingRequired("name", "ExplanationNode.DataUnused");
      }
      return new DataUnused(name);
    }
  }

  record Conversion(
      String expression, List<ConversionStep> steps, List<ExplanationNode> operands)
      implements ExplanationNode {
    @Override
    public String type() {
      return "conversion";
    }

    static Conversion read(JsonParser p) throws IOException {
      JsonReading.expectStartObject(p, "ExplanationNode.Conversion");
      String expression = null;
      List<ConversionStep> steps = null;
      List<ExplanationNode> operands = null;
      while (p.nextToken() != JsonToken.END_OBJECT) {
        String field = p.currentName();
        p.nextToken();
        switch (field) {
          case "type" -> expectType(p, "conversion");
          case "expression" -> expression = JsonReading.readString(p);
          case "steps" -> steps = JsonReading.readList(p, ConversionStep::read);
          case "operands" -> operands = JsonReading.readList(p, ExplanationNode::read);
          default -> JsonReading.unknownField(field, "ExplanationNode.Conversion");
        }
      }
      if (expression == null) {
        JsonReading.missingRequired("expression", "ExplanationNode.Conversion");
      }
      if (steps == null) {
        JsonReading.missingRequired("steps", "ExplanationNode.Conversion");
      }
      if (operands == null) {
        JsonReading.missingRequired("operands", "ExplanationNode.Conversion");
      }
      return new Conversion(expression, steps, operands);
    }
  }

  record Veto(@Nullable String message) implements ExplanationNode {
    @Override
    public String type() {
      return "veto";
    }

    static Veto read(JsonParser p) throws IOException {
      JsonReading.expectStartObject(p, "ExplanationNode.Veto");
      String message = null;
      while (p.nextToken() != JsonToken.END_OBJECT) {
        String field = p.currentName();
        p.nextToken();
        switch (field) {
          case "type" -> expectType(p, "veto");
          case "message" -> message = JsonReading.readString(p);
          default -> JsonReading.unknownField(field, "ExplanationNode.Veto");
        }
      }
      return new Veto(message);
    }
  }

  private static void expectType(JsonParser p, String expected) throws IOException {
    String type = JsonReading.readString(p);
    if (!expected.equals(type)) {
      throw new LemmaBugError("BUG: expected type '" + expected + "', got '" + type + "'");
    }
  }

  static ExplanationNode read(JsonParser p) throws IOException {
    JsonReading.expectStartObject(p, "ExplanationNode");
    String json = JsonReading.bufferObjectAsString(p);
    String type = JsonReading.findTag(json, "type");
    if (type == null) {
      throw new LemmaBugError("BUG: missing 'type' in ExplanationNode");
    }
    try (JsonParser reader = JsonReading.parserFor(json)) {
      return switch (type) {
        case "rule" -> Rule.read(reader);
        case "compose" -> Compose.read(reader);
        case "data" -> Data.read(reader);
        case "data_unused" -> DataUnused.read(reader);
        case "conversion" -> Conversion.read(reader);
        case "veto" -> Veto.read(reader);
        default -> throw new LemmaBugError("BUG: unknown type value: " + type);
      };
    }
  }
}
