package com.lemmabase.lemma;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.attribute.FileTime;
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
  void preExistingCacheFileIsReused(@TempDir Path tempDir) throws IOException {
    Path cacheRoot = tempDir.resolve("cache");
    Path cacheDir = cacheRoot.resolve("lemma-jni").resolve(VERSION + "-" + TRIPLE);
    Files.createDirectories(cacheDir);
    Path existingLib = cacheDir.resolve(LIB_NAME);
    byte[] existingContent = new byte[] {0x7f, 'E', 'L', 'F', 9, 9, 9, 9};
    Files.write(existingLib, existingContent);
    FileTime originalModified = Files.getLastModifiedTime(existingLib);

    Path result = Native.extractToCache(RESOURCE_PATH, TRIPLE, LIB_NAME, VERSION, cacheRoot);

    assertEquals(existingLib, result, "should return existing path");
    assertEquals(
        originalModified,
        Files.getLastModifiedTime(result),
        "should not modify existing file");
    assertArrayEquals(
        existingContent, Files.readAllBytes(result), "should keep original content");
  }

  @Test
  void cachePathStructureIsCorrect(@TempDir Path tempDir) throws IOException {
    Path cacheRoot = tempDir.resolve("cache");
    Path cacheDir = cacheRoot.resolve("lemma-jni").resolve(VERSION + "-" + TRIPLE);
    Files.createDirectories(cacheDir);
    Path existingLib = cacheDir.resolve(LIB_NAME);
    Files.write(existingLib, new byte[] {1, 2, 3});

    Path result = Native.extractToCache(RESOURCE_PATH, TRIPLE, LIB_NAME, VERSION, cacheRoot);

    Path expectedPath = cacheRoot.resolve("lemma-jni").resolve(VERSION + "-" + TRIPLE).resolve(LIB_NAME);
    assertEquals(expectedPath, result, "cache path should follow version-triple structure");
    assertTrue(result.startsWith(cacheRoot), "result should be under cache root");
  }

  @Test
  void differentVersionsUseDifferentCachePaths(@TempDir Path tempDir) throws IOException {
    Path cacheRoot = tempDir.resolve("cache");
    String version1 = "1.0.0";
    String version2 = "2.0.0";

    Path cache1 = cacheRoot.resolve("lemma-jni").resolve(version1 + "-" + TRIPLE);
    Path cache2 = cacheRoot.resolve("lemma-jni").resolve(version2 + "-" + TRIPLE);
    Files.createDirectories(cache1);
    Files.createDirectories(cache2);
    Files.write(cache1.resolve(LIB_NAME), new byte[] {1});
    Files.write(cache2.resolve(LIB_NAME), new byte[] {2});

    Path result1 = Native.extractToCache(RESOURCE_PATH, TRIPLE, LIB_NAME, version1, cacheRoot);
    Path result2 = Native.extractToCache(RESOURCE_PATH, TRIPLE, LIB_NAME, version2, cacheRoot);

    assertTrue(!result1.equals(result2), "different versions should use different paths");
    assertArrayEquals(new byte[] {1}, Files.readAllBytes(result1));
    assertArrayEquals(new byte[] {2}, Files.readAllBytes(result2));
  }
}
