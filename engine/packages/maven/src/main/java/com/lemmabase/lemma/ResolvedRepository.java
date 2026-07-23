package com.lemmabase.lemma;

import java.util.List;
import org.jspecify.annotations.Nullable;

/** One repository group from {@link Engine#list()}. */
public record ResolvedRepository(@Nullable String repository, List<ListedSpec> specs) {}
