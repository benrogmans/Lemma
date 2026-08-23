package com.lemmabase.lemma;

import com.fasterxml.jackson.core.JsonParser;
import com.fasterxml.jackson.core.JsonToken;
import java.io.IOException;
import java.util.List;
import java.util.Map;
import org.jspecify.annotations.Nullable;
/**
 * Show.
 * @param spec spec
 * @param commentary commentary
 * @param effectiveFrom effectiveFrom
 * @param effectiveTo effectiveTo
 * @param versions versions
 * @param startLine startLine
 * @param sourceType sourceType
 * @param data data
 * @param rules rules
 * @param meta meta
 */
public record Show(
    String spec,
    @Nullable String commentary,
    @Nullable String effectiveFrom,
    @Nullable String effectiveTo,
    @Nullable List<ShowVersion> versions,
    int startLine,
    @Nullable SourceType sourceType,
    Map<String, ShowData> data,
    Map<String, LemmaType> rules,
    Map<String, MetaValue> meta) {
  /**
   * ShowData.
   * @param type type
   * @param prefilled prefilled
   * @param suggestion suggestion
   * @param neededByRules neededByRules
   */
  public record ShowData(
      LemmaType type,
      @Nullable RuleResultValue prefilled,
      @Nullable RuleResultValue suggestion,
      List<String> neededByRules) {
    /**
     * Parses JSON.
     *
     * @param p parser at value start
     * @return parsed value
     * @throws IOException if JSON IO fails
     */
    static ShowData read(JsonParser p) throws IOException {
      JsonReading.expectStartObject(p, "ShowData");
      LemmaType type = null;
      RuleResultValue prefilled = null;
      RuleResultValue suggestion = null;
      List<String> neededByRules = null;
      while (p.nextToken() != JsonToken.END_OBJECT) {
        String field = p.currentName();
        p.nextToken();
        switch (field) {
          case "type" -> type = LemmaType.read(p);
          case "prefilled" -> prefilled = RuleResultValue.read(p);
          case "suggestion" -> suggestion = RuleResultValue.read(p);
          case "needed_by_rules" -> neededByRules = JsonReading.readList(p, JsonReading::readString);
          default -> JsonReading.unknownField(field, "ShowData");
        }
      }
      if (type == null) {
        JsonReading.missingRequired("type", "ShowData");
      }
      if (neededByRules == null) {
        JsonReading.missingRequired("needed_by_rules", "ShowData");
      }
      return new ShowData(type, prefilled, suggestion, neededByRules);
    }
  }
  /**
   * ShowVersion.
   * @param effectiveFrom effectiveFrom
   * @param effectiveTo effectiveTo
   */
  public record ShowVersion(@Nullable String effectiveFrom, @Nullable String effectiveTo) {
    /**
     * Parses JSON.
     *
     * @param p parser at value start
     * @return parsed value
     * @throws IOException if JSON IO fails
     */
    static ShowVersion read(JsonParser p) throws IOException {
      JsonReading.expectStartObject(p, "ShowVersion");
      String effectiveFrom = null;
      String effectiveTo = null;
      while (p.nextToken() != JsonToken.END_OBJECT) {
        String field = p.currentName();
        p.nextToken();
        switch (field) {
          case "effective_from" -> effectiveFrom = JsonReading.readString(p);
          case "effective_to" -> effectiveTo = JsonReading.readString(p);
          default -> JsonReading.unknownField(field, "ShowVersion");
        }
      }
      return new ShowVersion(effectiveFrom, effectiveTo);
    }
  }

  /**
   * Parses JSON.
   *
   * @param p parser at value start
   * @return parsed value
   * @throws IOException if JSON IO fails
   */
  public static Show read(JsonParser p) throws IOException {
    JsonReading.expectStartObject(p, "Show");
    String spec = null;
    String commentary = null;
    String effectiveFrom = null;
    String effectiveTo = null;
    List<ShowVersion> versions = null;
    Integer startLine = null;
    SourceType sourceType = null;
    Map<String, ShowData> data = null;
    Map<String, LemmaType> rules = null;
    Map<String, MetaValue> meta = null;
    while (p.nextToken() != JsonToken.END_OBJECT) {
      String field = p.currentName();
      p.nextToken();
      switch (field) {
        case "spec" -> spec = JsonReading.readString(p);
        case "commentary" -> commentary = JsonReading.readString(p);
        case "effective_from" -> effectiveFrom = JsonReading.readString(p);
        case "effective_to" -> effectiveTo = JsonReading.readString(p);
        case "versions" -> versions = JsonReading.readList(p, ShowVersion::read);
        case "start_line" -> startLine = JsonReading.readInt(p);
        case "source_type" -> sourceType = SourceType.read(p);
        case "data" -> data = JsonReading.readMap(p, ShowData::read);
        case "rules" -> rules = JsonReading.readMap(p, LemmaType::read);
        case "meta" -> meta = JsonReading.readMap(p, MetaValue::read);
        default -> JsonReading.unknownField(field, "Show");
      }
    }
    if (spec == null) {
      JsonReading.missingRequired("spec", "Show");
    }
    if (startLine == null) {
      JsonReading.missingRequired("start_line", "Show");
    }
    if (data == null) {
      JsonReading.missingRequired("data", "Show");
    }
    if (rules == null) {
      JsonReading.missingRequired("rules", "Show");
    }
    if (meta == null) {
      JsonReading.missingRequired("meta", "Show");
    }
    return new Show(
        spec,
        commentary,
        effectiveFrom,
        effectiveTo,
        versions,
        startLine,
        sourceType,
        data,
        rules,
        meta);
  }
}
