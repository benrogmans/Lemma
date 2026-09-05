package com.lemmabase.lemma;

import com.fasterxml.jackson.core.JsonGenerator;
import com.fasterxml.jackson.core.JsonParser;
import com.fasterxml.jackson.core.JsonToken;
import java.io.IOException;
import java.io.StringWriter;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.time.Duration;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;

/** Host-side LemmaBase HTTP loop. Package-private. */
final class LemmaBase {
  private LemmaBase() {}

  /**
   * Downloads a repository from LemmaBase using the given HTTP client.
   *
   * @param client HTTP client for the request
   * @param repository repository identifier (e.g. {@code @iso/countries})
   * @return downloaded source and repository id
   * @throws LemmaException on validation, HTTP, or transport failure
   */
  static RepositoryInstallResult install(HttpClient client, String repository) {
    return install(client, repository, null);
  }

  static RepositoryInstallResult install(
      HttpClient client, String repository, @org.jspecify.annotations.Nullable String limitsJson) {
    long handle = Native.installStart(repository, limitsJson);
    try {
      String stepJson = Native.installStep(handle);
      while (true) {
        Step step = Step.parse(stepJson);
        if (step instanceof Step.Finished finished) {
          return finished.result();
        }
        if (!(step instanceof Step.Fetch fetch)) {
          throw new LemmaBugError("BUG: unknown install step tag: " + stepJson);
        }
        try {
          HttpRequest.Builder builder =
              HttpRequest.newBuilder()
                  .uri(URI.create(fetch.url()))
                  .timeout(Duration.ofSeconds(30))
                  .GET();
          for (Header header : fetch.headers()) {
            builder.header(header.name(), header.value());
          }
          HttpResponse<String> response =
              client.send(builder.build(), HttpResponse.BodyHandlers.ofString());
          String headersJson = headersJson(response.headers().map());
          stepJson =
              Native.installRespond(handle, response.statusCode(), headersJson, response.body());
        } catch (IOException e) {
          String msg = e.getMessage();
          stepJson =
              Native.installFail(handle, msg != null ? msg : e.getClass().getName());
        } catch (InterruptedException e) {
          Thread.currentThread().interrupt();
          String msg = e.getMessage();
          stepJson =
              Native.installFail(handle, msg != null ? msg : e.getClass().getName());
        }
      }
    } finally {
      if (handle != 0) {
        Native.installFree(handle);
        handle = 0;
      }
    }
  }

  private static String headersJson(Map<String, List<String>> map) {
    StringWriter sw = new StringWriter();
    try (JsonGenerator g = JsonReading.FACTORY.createGenerator(sw)) {
      g.writeStartArray();
      for (var entry : map.entrySet()) {
        for (String value : entry.getValue()) {
          g.writeStartObject();
          g.writeStringField("name", entry.getKey());
          g.writeStringField("value", value);
          g.writeEndObject();
        }
      }
      g.writeEndArray();
    } catch (IOException e) {
      throw new LemmaBugError("BUG: headers JSON encode failed: " + e.getMessage());
    }
    return sw.toString();
  }

  private record Header(String name, String value) {}

  private sealed interface Step {
    record Fetch(String url, List<Header> headers) implements Step {}

    record Finished(RepositoryInstallResult result) implements Step {}

    static Step parse(String json) {
      try (JsonParser p = JsonReading.parserFor(json)) {
        JsonReading.expectStartObject(p, "InstallStep");
        while (p.nextToken() != JsonToken.END_OBJECT) {
          String field = p.currentName();
          p.nextToken();
          return switch (field) {
            case "fetch" -> parseFetch(p);
            case "finished" -> parseFinished(p);
            default -> throw new LemmaBugError("BUG: unknown InstallStep tag '" + field + "': " + json);
          };
        }
        throw new LemmaBugError("BUG: InstallStep missing tag: " + json);
      } catch (LemmaException e) {
        throw e;
      } catch (IOException e) {
        throw new LemmaBugError("BUG: InstallStep parse failed: " + e.getMessage());
      }
    }

    private static Step parseFetch(JsonParser p) throws IOException {
      JsonReading.expectStartObject(p, "fetch");
      String url = null;
      List<Header> headers = List.of();
      while (p.nextToken() != JsonToken.END_OBJECT) {
        String field = p.currentName();
        p.nextToken();
        switch (field) {
          case "url" -> url = p.getText();
          case "repository" -> p.skipChildren();
          case "headers" -> headers = parseHeaders(p);
          default -> JsonReading.unknownField(field, "fetch");
        }
      }
      if (url == null) {
        throw new LemmaBugError("BUG: fetch step missing url");
      }
      return new Fetch(url, headers);
    }

    private static List<Header> parseHeaders(JsonParser p) throws IOException {
      List<Header> headers = new ArrayList<>();
      if (p.currentToken() != JsonToken.START_ARRAY) {
        throw new LemmaBugError("BUG: fetch.headers must be an array");
      }
      while (p.nextToken() != JsonToken.END_ARRAY) {
        JsonReading.expectStartObject(p, "header");
        String name = null;
        String value = null;
        while (p.nextToken() != JsonToken.END_OBJECT) {
          String field = p.currentName();
          p.nextToken();
          switch (field) {
            case "name" -> name = p.getText();
            case "value" -> value = p.getText();
            default -> JsonReading.unknownField(field, "header");
          }
        }
        if (name == null || value == null) {
          throw new LemmaBugError("BUG: header missing name or value");
        }
        headers.add(new Header(name, value));
      }
      return headers;
    }

    private static Step parseFinished(JsonParser p) throws IOException {
      JsonReading.expectStartObject(p, "finished");
      RepositoryInstallResult result = null;
      LemmaException error = null;
      while (p.nextToken() != JsonToken.END_OBJECT) {
        String field = p.currentName();
        p.nextToken();
        switch (field) {
          case "ok" -> result = RepositoryInstallResult.read(p);
          case "err" -> {
            String errorsJson = JsonReading.bufferCurrentAsString(p);
            error = new LemmaException("LemmaBase install failed", errorsJson);
          }
          default -> JsonReading.unknownField(field, "finished");
        }
      }
      if (error != null) {
        throw error;
      }
      if (result == null) {
        throw new LemmaBugError("BUG: finished step missing ok or err");
      }
      return new Finished(result);
    }
  }
}
