package com.lemmabase.lemma;

import com.fasterxml.jackson.core.JsonParser;
import com.fasterxml.jackson.databind.DeserializationContext;
import com.fasterxml.jackson.databind.JsonDeserializer;
import java.io.IOException;
import java.math.BigDecimal;

/** Parse JSON string or number tokens into {@link BigDecimal} via plain decimal text. */
final class BigDecimalPlainDeserializer extends JsonDeserializer<BigDecimal> {
  @Override
  public BigDecimal deserialize(JsonParser parser, DeserializationContext context)
      throws IOException {
    return switch (parser.currentToken()) {
      case VALUE_STRING -> new BigDecimal(parser.getText());
      case VALUE_NUMBER_INT, VALUE_NUMBER_FLOAT -> new BigDecimal(parser.getText());
      case VALUE_NULL -> null;
      default ->
          (BigDecimal)
              context.handleUnexpectedToken(BigDecimal.class, parser);
    };
  }
}
