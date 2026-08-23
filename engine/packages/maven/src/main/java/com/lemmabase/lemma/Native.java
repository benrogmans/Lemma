package com.lemmabase.lemma;

import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.URISyntaxException;
import java.net.URL;
import java.nio.charset.StandardCharsets;
import java.nio.file.FileAlreadyExistsException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.Locale;

/** JNI declarations and native library load. */
final class Native {
  static {
    loadLibrary();
  }

  private Native() {}

  static native long create();

  static native long createWithLimits(String limitsJson);

  static native void destroy(long handle);

  static native void load(long handle, String code);

  static native void loadLabeled(long handle, String[] labels, String[] codes);

  static native String list(long handle);

  static native String show(long handle, String repository, String spec, String effective);

  static native String source(long handle, String repository, String spec, String effective);

  static native String run(
      long handle,
      String repository,
      String spec,
      String effective,
      String[] dataKeys,
      String[] dataValues,
      String[] rules,
      boolean explain);

  static native void remove(long handle, String repository, String spec, String effective);

  static native void update(
      long handle,
      String repository,
      String spec,
      String effective,
      String code,
      String attribute);

  static native String format(String code);

  static native String limits(long handle);

  static native String quality(long handle);

  private static void loadLibrary() {
    String triple = rustTargetTriple();
    String libName = libraryFileName();

    String propertyOverride = System.getProperty("lemma.native.library");
    if (propertyOverride != null && !propertyOverride.isBlank()) {
      Path overridePath = Path.of(propertyOverride);
      if (!Files.isRegularFile(overridePath)) {
        throw new LemmaBugError(
            "BUG: lemma.native.library set to '"
                + propertyOverride
                + "' but it is not a regular file");
      }
      System.load(overridePath.toAbsolutePath().toString());
      return;
    }

    String envOverride = System.getenv("LEMMA_JNI_LIBRARY");
    if (envOverride != null && !envOverride.isBlank()) {
      Path overridePath = Path.of(envOverride);
      if (!Files.isRegularFile(overridePath)) {
        throw new LemmaBugError(
            "BUG: LEMMA_JNI_LIBRARY set to '" + envOverride + "' but it is not a regular file");
      }
      System.load(overridePath.toAbsolutePath().toString());
      return;
    }

    String resourcePath = "native/" + triple + "/" + libName;
    URL resourceUrl = Native.class.getClassLoader().getResource(resourcePath);

    if (resourceUrl != null) {
      String protocol = resourceUrl.getProtocol();
      if ("file".equals(protocol)) {
        try {
          Path filePath = Path.of(resourceUrl.toURI());
          System.load(filePath.toAbsolutePath().toString());
          return;
        } catch (URISyntaxException e) {
          throw new LemmaBugError("BUG: invalid resource URI: " + resourceUrl + " - " + e);
        }
      } else if ("jar".equals(protocol)) {
        loadFromJar(resourceUrl, resourcePath, triple, libName);
        return;
      } else {
        throw new LemmaBugError("BUG: unsupported resource URL protocol: " + protocol);
      }
    }

    Path dev = discoverDevLibrary();
    if (dev != null) {
      System.load(dev.toAbsolutePath().toString());
      return;
    }

    throw new LemmaBugError(
        "BUG: native library not found. Checked: "
            + "lemma.native.library property, "
            + "LEMMA_JNI_LIBRARY env, "
            + "resource '"
            + resourcePath
            + "', "
            + "cargo target directories");
  }

  private static void loadFromJar(URL resourceUrl, String resourcePath, String triple, String libName) {
    String version = getImplementationVersion();
    Path cacheRoot = getCacheRoot();
    Path cachedLib = extractToCache(resourcePath, triple, libName, version, cacheRoot);
    System.load(cachedLib.toAbsolutePath().toString());
  }

  static Path extractToCache(
      String resourcePath, String triple, String libName, String version, Path cacheRoot) {
    Path cacheDir = cacheRoot.resolve("lemma-jni").resolve(version + "-" + triple);
    Path cachedLib = cacheDir.resolve(libName);

    if (Files.isRegularFile(cachedLib)) {
      return cachedLib;
    }

    try {
      Files.createDirectories(cacheDir);
    } catch (IOException e) {
      throw new LemmaBugError(
          "BUG: failed to create cache directory '"
              + cacheDir
              + "': "
              + e
              + " (override with lemma.native.cache.dir property)");
    }

    Path tempFile;
    try {
      tempFile = Files.createTempFile(cacheDir, "lemma_jni_", ".tmp");
    } catch (IOException e) {
      throw new LemmaBugError(
          "BUG: failed to create temp file in '"
              + cacheDir
              + "': "
              + e
              + " (override with lemma.native.cache.dir property)");
    }

    try (InputStream in = Native.class.getClassLoader().getResourceAsStream(resourcePath);
        OutputStream out = Files.newOutputStream(tempFile)) {
      if (in == null) {
        throw new LemmaBugError("BUG: resource disappeared: " + resourcePath);
      }
      in.transferTo(out);
    } catch (IOException e) {
      try {
        Files.deleteIfExists(tempFile);
      } catch (IOException ignored) {
      }
      throw new LemmaBugError("BUG: failed to extract native library: " + e);
    }

    try {
      Files.move(tempFile, cachedLib, StandardCopyOption.ATOMIC_MOVE);
    } catch (FileAlreadyExistsException e) {
      try {
        Files.deleteIfExists(tempFile);
      } catch (IOException ignored) {
      }
    } catch (IOException e) {
      try {
        Files.deleteIfExists(tempFile);
      } catch (IOException ignored) {
      }
      throw new LemmaBugError("BUG: failed to move extracted library to cache: " + e);
    }

    return cachedLib;
  }

  static String implementationVersion() {
    return getImplementationVersion();
  }

  private static String getImplementationVersion() {
    try (InputStream in = Native.class.getResourceAsStream("engine.version")) {
      if (in == null) {
        throw new LemmaBugError(
            "BUG: engine.version resource missing; cannot determine version for cache key");
      }
      String version = new String(in.readAllBytes(), StandardCharsets.UTF_8).trim();
      if (version.isBlank()) {
        throw new LemmaBugError(
            "BUG: engine.version blank; cannot determine version for cache key");
      }
      return version;
    } catch (IOException e) {
      throw new LemmaBugError("BUG: failed to read engine.version: " + e);
    }
  }

  private static Path getCacheRoot() {
    String cacheDirProperty = System.getProperty("lemma.native.cache.dir");
    if (cacheDirProperty != null && !cacheDirProperty.isBlank()) {
      return Path.of(cacheDirProperty);
    }
    String userHome = System.getProperty("user.home");
    if (userHome == null || userHome.isBlank()) {
      throw new LemmaBugError(
          "BUG: user.home property is not set; override with lemma.native.cache.dir property");
    }
    return Path.of(userHome, ".cache");
  }

  private static Path discoverDevLibrary() {
    String name = libraryFileName();
    String cargoTarget = System.getenv("CARGO_TARGET_DIR");
    if (cargoTarget != null && !cargoTarget.isBlank()) {
      Path[] fromEnv =
          new Path[] {
            Path.of(cargoTarget, "debug", name),
            Path.of(cargoTarget, "release", name),
          };
      for (Path candidate : fromEnv) {
        if (Files.isRegularFile(candidate)) {
          return candidate;
        }
      }
    }
    Path[] candidates =
        new Path[] {
          Path.of("target/debug").resolve(name),
          Path.of("target/release").resolve(name),
          Path.of("../../../../target/debug").resolve(name),
          Path.of("../../../../target/release").resolve(name),
          Path.of(System.getProperty("user.dir"), "target/debug", name),
          Path.of(System.getProperty("user.dir"), "target/release", name),
        };
    for (Path candidate : candidates) {
      if (Files.isRegularFile(candidate)) {
        return candidate;
      }
    }
    return null;
  }

  private static String libraryFileName() {
    String os = System.getProperty("os.name", "").toLowerCase(Locale.ROOT);
    if (os.contains("mac") || os.contains("darwin")) {
      return "liblemma_jni.dylib";
    }
    if (os.contains("win")) {
      return "lemma_jni.dll";
    }
    return "liblemma_jni.so";
  }

  private static String rustTargetTriple() {
    String os = System.getProperty("os.name", "").toLowerCase(Locale.ROOT);
    String arch = System.getProperty("os.arch", "").toLowerCase(Locale.ROOT);
    String rustArch =
        switch (arch) {
          case "amd64", "x86_64" -> "x86_64";
          case "aarch64", "arm64" -> "aarch64";
          default ->
              throw new LemmaBugError("BUG: unsupported CPU architecture for lemma_jni: " + arch);
        };
    if (os.contains("mac") || os.contains("darwin")) {
      return rustArch + "-apple-darwin";
    }
    if (os.contains("win")) {
      return rustArch + "-pc-windows-msvc";
    }
    if (os.contains("linux")) {
      return rustArch + "-unknown-linux-gnu";
    }
    throw new LemmaBugError("BUG: unsupported OS for lemma_jni: " + os);
  }
}
