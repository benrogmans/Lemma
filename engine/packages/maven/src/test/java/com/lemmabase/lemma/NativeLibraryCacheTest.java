package com.lemmabase.lemma;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.io.IOException;
import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.HexFormat;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

final class NativeLibraryCacheTest {
  private static final String LIB_NAME = "liblemma_jni.so";
  private static final String TRIPLE = "x86_64-unknown-linux-gnu";
  private static final String VERSION = "0.9.1-test";
  private static final String RESOURCE_PATH = "native/" + TRIPLE + "/" + LIB_NAME;

  @Test
  void implementationVersionIsSemVer() {
    String version = Native.implementationVersion();
    assertTrue(version.matches("\\d+\\.\\d+\\.\\d+"), "version: " + version);
  }

  @Test
  void matchingContentHashReusesCacheFile(@TempDir Path tempDir) throws Exception {
    byte[] resourceBytes = readResource(RESOURCE_PATH);
    String hash = sha256Hex(resourceBytes);
    Path cacheRoot = tempDir.resolve("cache");
    Path cacheDir =
        cacheRoot.resolve("lemma-jni").resolve(VERSION + "-" + TRIPLE).resolve(hash);
    Files.createDirectories(cacheDir);
    Path existingLib = cacheDir.resolve(LIB_NAME);
    Files.write(existingLib, resourceBytes);

    Path result = Native.extractToCache(RESOURCE_PATH, TRIPLE, LIB_NAME, VERSION, cacheRoot);

    assertEquals(existingLib, result);
    assertArrayEquals(resourceBytes, Files.readAllBytes(result));
  }

  @Test
  void staleVersionOnlyCacheIsIgnored(@TempDir Path tempDir) throws Exception {
    byte[] resourceBytes = readResource(RESOURCE_PATH);
    Path cacheRoot = tempDir.resolve("cache");
    Path staleDir = cacheRoot.resolve("lemma-jni").resolve(VERSION + "-" + TRIPLE);
    Files.createDirectories(staleDir);
    Path staleLib = staleDir.resolve(LIB_NAME);
    Files.write(staleLib, new byte[] {0x7f, 'E', 'L', 'F', 9, 9, 9, 9});

    Path result = Native.extractToCache(RESOURCE_PATH, TRIPLE, LIB_NAME, VERSION, cacheRoot);

    assertNotEquals(staleLib, result);
    assertArrayEquals(resourceBytes, Files.readAllBytes(result));
    assertTrue(result.toString().contains(sha256Hex(resourceBytes)));
  }

  @Test
  void cachePathIncludesContentHash(@TempDir Path tempDir) throws Exception {
    byte[] resourceBytes = readResource(RESOURCE_PATH);
    String hash = sha256Hex(resourceBytes);
    Path cacheRoot = tempDir.resolve("cache");

    Path result = Native.extractToCache(RESOURCE_PATH, TRIPLE, LIB_NAME, VERSION, cacheRoot);

    Path expected =
        cacheRoot
            .resolve("lemma-jni")
            .resolve(VERSION + "-" + TRIPLE)
            .resolve(hash)
            .resolve(LIB_NAME);
    assertEquals(expected, result);
    assertTrue(result.startsWith(cacheRoot));
  }

  @Test
  void differentVersionsUseDifferentCachePaths(@TempDir Path tempDir) throws Exception {
    Path cacheRoot = tempDir.resolve("cache");
    String version1 = "1.0.0";
    String version2 = "2.0.0";

    Path result1 = Native.extractToCache(RESOURCE_PATH, TRIPLE, LIB_NAME, version1, cacheRoot);
    Path result2 = Native.extractToCache(RESOURCE_PATH, TRIPLE, LIB_NAME, version2, cacheRoot);

    assertNotEquals(result1, result2);
    assertTrue(result1.toString().contains(version1));
    assertTrue(result2.toString().contains(version2));
  }

  private static byte[] readResource(String path) throws IOException {
    try (InputStream in = NativeLibraryCacheTest.class.getClassLoader().getResourceAsStream(path)) {
      assertTrue(in != null, "test resource missing: " + path);
      return in.readAllBytes();
    }
  }

  private static String sha256Hex(byte[] bytes) throws NoSuchAlgorithmException {
    return HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(bytes));
  }
}
