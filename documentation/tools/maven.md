---
nav_title: Maven
nav_order: 40
---

# Maven

Embed the Lemma engine in Java, Kotlin, or Scala via `com.lemmabase:lemma-engine` on Maven Central. The JAR ships prebuilt `lemma_jni` natives for the same six targets as the Hex package (no musl).

Decimals use `java.math.BigDecimal`. `float` / `double` data values are rejected so binary floating point cannot silently corrupt exact magnitudes. See [Precision](../learn/precision.md).

## Install

Maven:

```xml
<dependency>
  <groupId>com.lemmabase</groupId>
  <artifactId>lemma-engine</artifactId>
  <version>0.9.0</version>
</dependency>
```

Gradle (Kotlin DSL) — consume the published artifact; this repository builds the package with Maven only:

```kotlin
implementation("com.lemmabase:lemma-engine:0.9.0")
```

Requires JDK 17+.

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
| `list()` / `show(...)` / `source(...)` / `remove(...)` | Inspect and manage loaded specs |
| `limits()` | Current resource limits |
| `Lemma.format(String)` | Format source without an engine |
| `close()` | Drop native engine (`AutoCloseable` + `Cleaner`) |

`RunRequest.of(spec)` defaults: repository null, effective null, rules null (all rules), explain false. An empty `rules` list is a request error. With `explain(true)`, `RuleResult.explanation()` is a JSON tree matching [explanation.v1.json](../schemas/explanation.v1.json).

## Errors and veto

- Invalid Lemma or bad requests → unchecked `LemmaException` with WASM-shaped `EngineError` entries.
- Domain `veto` → still a successful `Response`; check `RuleResult.vetoed()` / `vetoReason()`.
- Use-after-close or invariant failures → `LemmaBugError`.

## Thread safety

One `Engine` instance; serialize access across threads (native side holds a mutex). Prefer one engine per worker, or external locking around a shared instance.

## Development in this repository

```bash
cargo build -p lemma_jni
cd engine/packages/maven && ./mvnw test
```

`cargo precommit` builds `lemma_jni` and runs `./mvnw -B test`.
