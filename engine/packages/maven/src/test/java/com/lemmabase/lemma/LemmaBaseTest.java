package com.lemmabase.lemma;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.io.IOException;
import java.net.Authenticator;
import java.net.CookieHandler;
import java.net.ProxySelector;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpHeaders;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Duration;
import java.util.Optional;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.Executor;
import javax.net.ssl.SSLContext;
import javax.net.ssl.SSLParameters;
import javax.net.ssl.SSLSession;
import org.junit.jupiter.api.Test;

final class LemmaBaseTest {
  private static final Path FIXTURES_DIR =
      Path.of(System.getProperty("user.dir"))
          .toAbsolutePath()
          .normalize()
          .resolve("../../tests/registry_fixtures")
          .normalize();

  @Test
  void installSucceeds() throws Exception {
    String body =
        Files.readString(
            FIXTURES_DIR.resolve("@iso").resolve("countries.lemma"), StandardCharsets.UTF_8);
    HttpClient client =
        new FixtureHttpClient("https://lemmabase.com/@iso/countries.lemma", 200, body);
    RepositoryInstallResult result = LemmaBase.install(client, "@iso/countries");
    assertEquals("@iso/countries", result.id());
    assertTrue(result.source().contains("spec alpha2"));
  }

  @Test
  void installNotFoundThrowsLemmaException() {
    HttpClient client =
        new FixtureHttpClient("https://lemmabase.com/@iso/nonexistent.lemma", 404, "not found");
    LemmaException thrown =
        assertThrows(LemmaException.class, () -> LemmaBase.install(client, "@iso/nonexistent"));
    assertFalse(thrown.errors().isEmpty());
    assertTrue(
        thrown.errors().stream().anyMatch(e -> "not_found".equals(e.registryKind())),
        thrown.errors().toString());
  }

  @Test
  void installNetworkErrorThrowsLemmaException() {
    HttpClient client = new FailingHttpClient();
    LemmaException thrown =
        assertThrows(LemmaException.class, () -> LemmaBase.install(client, "@iso/countries"));
    assertFalse(thrown.errors().isEmpty());
    assertTrue(
        thrown.errors().stream().anyMatch(e -> "network_error".equals(e.registryKind())),
        thrown.errors().toString());
  }

  @Test
  void installEmptyIdThrowsLemmaException() {
    HttpClient client = new FixtureHttpClient("https://lemmabase.com/unused.lemma", 200, "unused");
    LemmaException thrown =
        assertThrows(LemmaException.class, () -> LemmaBase.install(client, "   "));
    assertFalse(thrown.errors().isEmpty());
    assertTrue(
        thrown.errors().stream()
            .anyMatch(e -> "registry".equals(e.kind()) || "request".equals(e.kind())),
        thrown.errors().toString());
  }

  private abstract static class AbstractHttpClient extends HttpClient {
    @Override
    public Optional<CookieHandler> cookieHandler() {
      return Optional.empty();
    }

    @Override
    public Optional<Duration> connectTimeout() {
      return Optional.empty();
    }

    @Override
    public Redirect followRedirects() {
      return Redirect.NEVER;
    }

    @Override
    public Optional<ProxySelector> proxy() {
      return Optional.empty();
    }

    @Override
    public SSLContext sslContext() {
      throw new UnsupportedOperationException();
    }

    @Override
    public SSLParameters sslParameters() {
      throw new UnsupportedOperationException();
    }

    @Override
    public Optional<Authenticator> authenticator() {
      return Optional.empty();
    }

    @Override
    public Version version() {
      return Version.HTTP_1_1;
    }

    @Override
    public Optional<Executor> executor() {
      return Optional.empty();
    }

    @Override
    public <T> CompletableFuture<HttpResponse<T>> sendAsync(
        HttpRequest request, HttpResponse.BodyHandler<T> responseBodyHandler) {
      throw new UnsupportedOperationException();
    }

    @Override
    public <T> CompletableFuture<HttpResponse<T>> sendAsync(
        HttpRequest request,
        HttpResponse.BodyHandler<T> responseBodyHandler,
        HttpResponse.PushPromiseHandler<T> pushPromiseHandler) {
      throw new UnsupportedOperationException();
    }
  }

  private static final class FixtureHttpClient extends AbstractHttpClient {
    private final String expectedUrl;
    private final int status;
    private final String body;

    FixtureHttpClient(String expectedUrl, int status, String body) {
      this.expectedUrl = expectedUrl;
      this.status = status;
      this.body = body;
    }

    @Override
    @SuppressWarnings("unchecked")
    public <T> HttpResponse<T> send(HttpRequest request, HttpResponse.BodyHandler<T> handler) {
      assertEquals(expectedUrl, request.uri().toString());
      assertEquals("lemmabase.com", request.uri().getHost());
      return new FixedResponse<>(request, status, (T) body);
    }
  }

  private static final class FailingHttpClient extends AbstractHttpClient {
    @Override
    public <T> HttpResponse<T> send(HttpRequest request, HttpResponse.BodyHandler<T> handler)
        throws IOException {
      assertEquals("https://lemmabase.com/@iso/countries.lemma", request.uri().toString());
      throw new IOException("connection refused");
    }
  }

  private record FixedResponse<T>(HttpRequest request, int status, T body) implements HttpResponse<T> {
    @Override
    public int statusCode() {
      return status;
    }

    @Override
    public HttpRequest request() {
      return request;
    }

    @Override
    public Optional<HttpResponse<T>> previousResponse() {
      return Optional.empty();
    }

    @Override
    public HttpHeaders headers() {
      return HttpHeaders.of(java.util.Map.of(), (a, b) -> true);
    }

    @Override
    public T body() {
      return body;
    }

    @Override
    public Optional<SSLSession> sslSession() {
      return Optional.empty();
    }

    @Override
    public URI uri() {
      return request.uri();
    }

    @Override
    public HttpClient.Version version() {
      return HttpClient.Version.HTTP_1_1;
    }
  }
}
