package com.lemmabase.lemma;

import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.file.Files;
import java.nio.file.Path;
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

  static native String format(String code);

  static native String limits(long handle);

  private static void loadLibrary() {
    String triple = rustTargetTriple();
    String resourcePath = "native/" + triple + "/" + libraryFileName();
    try (InputStream in = Native.class.getClassLoader().getResourceAsStream(resourcePath)) {
      if (in == null) {
        // Development: load from cargo target directory next to the package.
        Path dev = discoverDevLibrary();
        if (dev != null) {
          System.load(dev.toAbsolutePath().toString());
          return;
        }
        throw new LemmaBugError(
            "BUG: native library resource missing: "
                + resourcePath
                + " (and no local cargo build found)");
      }
      Path dir = Files.createTempDirectory("lemma-jni-");
      dir.toFile().deleteOnExit();
      Path lib = dir.resolve(libraryFileName());
      try (OutputStream out = Files.newOutputStream(lib)) {
        in.transferTo(out);
      }
      lib.toFile().deleteOnExit();
      System.load(lib.toAbsolutePath().toString());
    } catch (IOException e) {
      throw new LemmaBugError("BUG: failed to extract native library: " + e.getMessage());
    }
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
    String override = System.getenv("LEMMA_JNI_LIBRARY");
    if (override != null && !override.isBlank()) {
      Path path = Path.of(override);
      if (Files.isRegularFile(path)) {
        return path;
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
