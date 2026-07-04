---
nav_title: Maven
nav_order: 40
---

# Maven (coming soon!)

A Maven package that brings the Lemma engine to the JVM is on the way, so Java, Kotlin, and Scala projects will be able to embed the engine directly. It is not published yet.

Until it lands, embed Lemma through one of the other SDKs, or drive the engine from the command line with the [Lemma CLI](../reference/cli.md). Check back soon.

# Example

```java
import com.lemmabase.lemma.Engine;
import com.lemmabase.lemma.Response;
import java.util.Map;

Engine engine = Engine.create();

engine.load("""
    spec pricing
    data count: number
    data price: 10
    rule total: count * price
    rule discount: 0
      unless count >= 10 then 5
      unless count >= 50 then 15
    """);

Response response = engine.run("pricing", Map.of("count", "25"));
```
