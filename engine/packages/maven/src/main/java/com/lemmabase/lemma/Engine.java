package com.lemmabase.lemma;

import java.lang.ref.Cleaner;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.concurrent.locks.ReentrantLock;
import org.jspecify.annotations.Nullable;

/** Lemma rules engine. Thread-safe. All methods acquire an internal lock; concurrent calls from multiple threads are safe. */
public final class Engine implements AutoCloseable {
  private static final Cleaner CLEANER = Cleaner.create();

  private final NativeState state;
  private final Cleaner.Cleanable cleanable;

  private Engine(long handle) {
    if (handle == 0L) {
      throw new LemmaBugError("BUG: native engine create returned null handle");
    }
    this.state = new NativeState(handle);
    this.cleanable = CLEANER.register(this, new DestroyAction(state));
  }

  public static Engine create() {
    return new Engine(Native.create());
  }

  public static Engine create(ResourceLimits limits) {
    Objects.requireNonNull(limits, "limits");
    return new Engine(Native.createWithLimits(JsonSupport.limitsToJson(limits)));
  }

  public void load(String code) {
    Objects.requireNonNull(code, "code");
    state.lock.lock();
    try {
      Native.load(state.requireHandle(), code);
    } finally {
      state.lock.unlock();
    }
  }

  public void load(Map<String, String> sources) {
    Objects.requireNonNull(sources, "sources");
    String[] labels = sources.keySet().toArray(String[]::new);
    String[] codes = new String[labels.length];
    for (int i = 0; i < labels.length; i++) {
      codes[i] = Objects.requireNonNull(sources.get(labels[i]), "source code");
    }
    state.lock.lock();
    try {
      Native.loadLabeled(state.requireHandle(), labels, codes);
    } finally {
      state.lock.unlock();
    }
  }

  public List<ResolvedRepository> list() {
    String json;
    state.lock.lock();
    try {
      json = Native.list(state.requireHandle());
    } finally {
      state.lock.unlock();
    }
    return JsonSupport.parseList(json);
  }

  public Show show(@Nullable String repository, String spec, @Nullable String effective) {
    Objects.requireNonNull(spec, "spec");
    String json;
    state.lock.lock();
    try {
      json = Native.show(state.requireHandle(), repository, spec, effective);
    } finally {
      state.lock.unlock();
    }
    return JsonSupport.parseShow(json);
  }

  public String source(
      @Nullable String repository, @Nullable String spec, @Nullable String effective) {
    state.lock.lock();
    try {
      return Native.source(state.requireHandle(), repository, spec, effective);
    } finally {
      state.lock.unlock();
    }
  }

  public Response run(RunRequest request) {
    Objects.requireNonNull(request, "request");
    Map<String, String> data = RunDataValues.toEngineStrings(request.data());
    String[] keys = data.keySet().toArray(String[]::new);
    String[] values = new String[keys.length];
    for (int i = 0; i < keys.length; i++) {
      values[i] = data.get(keys[i]);
    }
    String[] rules =
        request.rules() == null ? null : request.rules().toArray(String[]::new);
    String json;
    state.lock.lock();
    try {
      json =
          Native.run(
              state.requireHandle(),
              request.repository(),
              request.spec(),
              request.effective(),
              keys,
              values,
              rules,
              request.explain());
    } finally {
      state.lock.unlock();
    }
    return JsonSupport.parseResponse(json);
  }

  public void remove(@Nullable String repository, String spec, @Nullable String effective) {
    Objects.requireNonNull(spec, "spec");
    state.lock.lock();
    try {
      Native.remove(state.requireHandle(), repository, spec, effective);
    } finally {
      state.lock.unlock();
    }
  }

  public ResourceLimits limits() {
    String json;
    state.lock.lock();
    try {
      json = Native.limits(state.requireHandle());
    } finally {
      state.lock.unlock();
    }
    return JsonSupport.parseLimits(json);
  }

  @Override
  public void close() {
    state.lock.lock();
    try {
      state.destroy();
    } finally {
      state.lock.unlock();
    }
    // Outside lock: destroy() already ran; clean() deregisters the cleanable to prevent
    // duplicate cleanup by the Cleaner thread.
    cleanable.clean();
  }

  /**
   * Native handle lifecycle shared by Engine methods and the Cleaner action. {@code handle == 0}
   * means destroyed. All reads/writes happen under {@link #lock}.
   */
  private static final class NativeState {
    final ReentrantLock lock = new ReentrantLock();
    private long handle;

    NativeState(long handle) {
      this.handle = handle;
    }

    long requireHandle() {
      if (handle == 0L) {
        throw new LemmaBugError("BUG: Engine used after close");
      }
      return handle;
    }

    void destroy() {
      if (handle == 0L) {
        return;
      }
      long toDestroy = handle;
      handle = 0L;
      Native.destroy(toDestroy);
    }
  }

  private static final class DestroyAction implements Runnable {
    private final NativeState state;

    DestroyAction(NativeState state) {
      this.state = state;
    }

    @Override
    public void run() {
      state.lock.lock();
      try {
        state.destroy();
      } finally {
        state.lock.unlock();
      }
    }
  }
}
