package com.lemmabase.lemma;

import java.lang.ref.Cleaner;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import org.jspecify.annotations.Nullable;

/** Lemma rules engine. One instance; serialize access across threads. */
public final class Engine implements AutoCloseable {
  private static final Cleaner CLEANER = Cleaner.create();

  private final long handle;
  private final Cleaner.Cleanable cleanable;
  private boolean closed;

  private Engine(long handle) {
    if (handle == 0L) {
      throw new LemmaBugError("BUG: native engine create returned null handle");
    }
    this.handle = handle;
    this.cleanable = CLEANER.register(this, new DestroyAction(handle));
  }

  public static Engine create() {
    return new Engine(Native.create());
  }

  public static Engine create(ResourceLimits limits) {
    Objects.requireNonNull(limits, "limits");
    return new Engine(Native.createWithLimits(JsonSupport.limitsToJson(limits)));
  }

  public void load(String code) {
    ensureOpen();
    Objects.requireNonNull(code, "code");
    Native.load(handle, code);
  }

  public void load(Map<String, String> sources) {
    ensureOpen();
    Objects.requireNonNull(sources, "sources");
    String[] labels = sources.keySet().toArray(String[]::new);
    String[] codes = new String[labels.length];
    for (int i = 0; i < labels.length; i++) {
      codes[i] = Objects.requireNonNull(sources.get(labels[i]), "source code");
    }
    Native.loadLabeled(handle, labels, codes);
  }

  public List<ResolvedRepository> list() {
    ensureOpen();
    return JsonSupport.parseList(Native.list(handle));
  }

  public Show show(@Nullable String repository, String spec, @Nullable String effective) {
    ensureOpen();
    Objects.requireNonNull(spec, "spec");
    return JsonSupport.parseShow(Native.show(handle, repository, spec, effective));
  }

  public String source(
      @Nullable String repository, @Nullable String spec, @Nullable String effective) {
    ensureOpen();
    return Native.source(handle, repository, spec, effective);
  }

  public Response run(RunRequest request) {
    ensureOpen();
    Objects.requireNonNull(request, "request");
    Map<String, String> data = DataValues.toEngineStrings(request.data());
    String[] keys = data.keySet().toArray(String[]::new);
    String[] values = new String[keys.length];
    for (int i = 0; i < keys.length; i++) {
      values[i] = data.get(keys[i]);
    }
    String[] rules =
        request.rules() == null ? null : request.rules().toArray(String[]::new);
    String json =
        Native.run(
            handle,
            request.repository(),
            request.spec(),
            request.effective(),
            keys,
            values,
            rules,
            request.explain());
    return JsonSupport.parseResponse(json);
  }

  public void remove(@Nullable String repository, String spec, @Nullable String effective) {
    ensureOpen();
    Objects.requireNonNull(spec, "spec");
    Native.remove(handle, repository, spec, effective);
  }

  public ResourceLimits limits() {
    ensureOpen();
    return JsonSupport.parseLimits(Native.limits(handle));
  }

  @Override
  public void close() {
    if (closed) {
      return;
    }
    closed = true;
    cleanable.clean();
  }

  private void ensureOpen() {
    if (closed) {
      throw new LemmaBugError("BUG: Engine used after close");
    }
  }

  private static final class DestroyAction implements Runnable {
    private final long handle;

    DestroyAction(long handle) {
      this.handle = handle;
    }

    @Override
    public void run() {
      Native.destroy(handle);
    }
  }
}
