---
nav_title: Java / Kotlin
nav_order: 40
---

# Java / Kotlin

Embed the Lemma engine in Java, Kotlin, or Scala via `com.lemmabase:lemma-engine` on Maven Central. The JAR ships prebuilt `lemma_jni` natives for the same six targets as the Hex package (no musl).

Decimals use `java.math.BigDecimal`. `float` / `double` data values are rejected so binary floating point cannot silently corrupt exact magnitudes. See [Precision](../learn/precision.md).

## Install

Maven:

```xml
<dependency>
  <groupId>com.lemmabase</groupId>
  <artifactId>lemma-engine</artifactId>
  <version>0.9.6</version>
</dependency>
```

Gradle (Kotlin DSL): consume the published artifact; this repository builds the package with Maven only:

```kotlin
implementation("com.lemmabase:lemma-engine:0.9.6")
```

Requires JDK 21+.

## Java example

```java
import com.lemmabase.lemma.Engine;
import com.lemmabase.lemma.Response;
import com.lemmabase.lemma.RunRequest;
import com.lemmabase.lemma.RuleResult;
import java.math.BigDecimal;
import java.util.Map;

try (Engine engine = Engine.create()) {
  engine.load("""
      spec order
      data quantity: number
      data unit_price: number
      data tax_rate: number
      rule subtotal: quantity * unit_price
      rule tax: subtotal * tax_rate
      rule total: subtotal + tax
      """);

  Response response = engine.run(
      RunRequest.of("order")
          .data(Map.of(
              "quantity", 3,
              "unit_price", new BigDecimal("19.99"),
              "tax_rate", new BigDecimal("0.21"))));

  RuleResult total = response.results().get("total");
  BigDecimal amount = total.number();
}
```

## Kotlin example

Same artifact:

```kotlin
import com.lemmabase.lemma.Engine
import com.lemmabase.lemma.RunRequest
import java.math.BigDecimal

Engine.create().use { engine ->
    engine.load("""
        spec order
        data quantity: number
        data unit_price: number
        data tax_rate: number
        rule subtotal: quantity * unit_price
        rule tax: subtotal * tax_rate
        rule total: subtotal + tax
        """.trimIndent())

    val response = engine.run(
        RunRequest.of("order").data(
            mapOf(
                "quantity" to 3,
                "unit_price" to BigDecimal("19.99"),
                "tax_rate" to BigDecimal("0.21"),
            )
        )
    )
    val total: BigDecimal = response.results()["total"]!!.number()!!
}
```

## API surface

| Method | Role |
|--------|------|
| `Engine.create()` / `Engine.create(ResourceLimits)` | Construct engine |
| `load(String)` / `load(Map<String,String>)` | Validate and load sources (volatile or labeled) |
| `run(RunRequest)` | Evaluate; named fields only |
| `list()` / `show(...)` / `source(...)` / `remove(...)` / `update(...)` | Inspect and manage loaded specs |
| `limits()` | Current resource limits |
| `Lemma.format(String)` | Format source without an engine |
| `close()` | Drop native engine (`AutoCloseable` + `Cleaner`) |

`RunRequest.of(spec)` defaults: repository null, effective null, rules null (all rules), explain false. An empty `rules` list is a request error. With `explain(true)`, `RuleResult.explanation()` is an `ExplanationNode.Rule` whose children are a sealed `ExplanationNode` tree. `show(...)` returns `Show`; each `Show.data` entry is `ShowData`. Non-veto `RuleResult` flattens `RuleResultValue` (`display()` / typed accessors).

## Errors and veto

- Invalid Lemma or bad requests → unchecked `LemmaException` with WASM-shaped `EngineError` entries.
- Domain `veto` → still a successful `Response`; check `RuleResult.vetoed()` / `vetoReason()`.
- Use-after-close or invariant failures → `LemmaBugError`.

## Thread safety

`Engine` is thread-safe: every method acquires an internal `ReentrantLock`. Multiple threads may share one instance without external synchronization. Long-running evaluations block other callers on that lock, so prefer one engine per worker for throughput.

## Native library loading

The JAR embeds `lemma_jni` natives. On first use, the SDK extracts the native for the current platform to a version-keyed cache and loads it. Override the automatic resolution:

| Priority | Source | Example |
|----------|--------|---------|
| 1 | System property `lemma.native.library` | `-Dlemma.native.library=/opt/liblemma_jni.so` |
| 2 | Environment variable `LEMMA_JNI_LIBRARY` | `LEMMA_JNI_LIBRARY=/opt/liblemma_jni.so` |
| 3 | Bundled JAR resource (extracted to cache) | Automatic |

The cache path is `~/.cache/lemma-jni/{version}-{triple}/`. Override the cache root with the system property `lemma.native.cache.dir`.

## ExplanationNode dispatch

`ExplanationNode` is a sealed interface. Each variant record implements `ExplanationNode.type()` returning its discriminator (`rule`, `compose`, `data`, `data_unused`, `conversion`, `veto`). Use pattern matching (Java 21+) or switch on `type()`:

```java
switch (node.type()) {
    case "rule" -> handleRule((ExplanationNode.Rule) node);
    case "compose" -> handleCompose((ExplanationNode.Compose) node);
    case "data" -> handleData((ExplanationNode.Data) node);
    case "data_unused" -> handleDataUnused((ExplanationNode.DataUnused) node);
    case "conversion" -> handleConversion((ExplanationNode.Conversion) node);
    case "veto" -> handleVeto((ExplanationNode.Veto) node);
    default -> throw new IllegalStateException("unknown explanation type: " + node.type());
}
```

`LemmaType` follows the same pattern with `kind()`.
