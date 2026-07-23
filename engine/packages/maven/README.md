# lemma-engine (Maven)

JVM binding for the Lemma rules engine. Coordinates: `com.lemmabase:lemma-engine`.

```xml
<dependency>
  <groupId>com.lemmabase</groupId>
  <artifactId>lemma-engine</artifactId>
  <version>0.9.0</version>
</dependency>
```

Docs: [documentation/tools/maven.md](../../../documentation/tools/maven.md). Explanation JSON (when `RunRequest.explain(true)`): [explanation.v1.json](../../../documentation/schemas/explanation.v1.json).

## Develop

From the repository root:

```bash
cargo build -p lemma_jni
./mvnw -B test
```

The Maven wrapper copies the host `lemma_jni` shared library into `src/main/resources/native/<triple>/` before tests. That directory is gitignored; release packaging fills all six triples in CI.
