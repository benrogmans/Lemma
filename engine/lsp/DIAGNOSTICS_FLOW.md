# Why registry errors might not show as squiggles

## Flow when a file has `fact x = doc @nonexistent/...`

1. **LSP has the file**  
   `workspace.files` contains `(Url, TrackedFile)` for the open file.  
   The file was added via `didOpen` / `didChange` or `discover_workspace_files` (if a folder was opened).

2. **Debounce runs**  
   After 250ms without changes, the debounce task runs. It:
   - Reads `local_docs = workspace.all_parsed_docs()`, `sources = workspace.sources_map()`, `attr_map = workspace.attribute_to_url_and_text()`.
   - Calls `resolve_registry_references(local_docs, &mut sources, registry, limits).await`.

3. **Engine resolution**  
   Engine finds the `@nonexistent/...` reference, calls the registry (LemmaBase).  
   Registry fails (e.g. network). Engine returns `Err(LemmaError::Registry { details, ... })`.  
   `details.source_location` is the **fact’s** source (from the AST).  
   That source’s `attribute` was set when we **parsed** the file: it’s the value we passed to `parse(&text, &attribute, &limits)` in `update_file`, i.e. `WorkspaceModel::attribute_for_url(&url)`.

4. **Attributing the error to a file**  
   We call `attribute_errors_to_files(&registry_error, &attr_map)`.  
   `attr_map` is built from the same workspace: for each `(url, tracked)` we insert  
   `(attribute_for_url(url), (url, text))`.  
   So the map’s keys are exactly the same as the `attribute` used when parsing.  
   We look up `err.location().attribute` in `attr_map`. If it matches, we add the error to that file’s diagnostics.

5. **Publishing**  
   We convert `FileDiagnostics` to LSP diagnostics and call `client.publish_diagnostics(url, diagnostics, None)`.

## Where it can go wrong

- **Attribute mismatch**  
  If the error’s `source_location.attribute` is not equal to any key in `attr_map`, the error is dropped and that file gets no diagnostics.  
  That can happen if:
  - The URL used when building `attr_map` is not the same as the URL used when we called `update_file` (same workspace, so they should be the same).
  - Path normalization differs (e.g. `file:///path` vs `/path`, or symlinks) so `attribute_for_url` gives different strings for “the same” file.
  - The engine ever builds the error with a different source (e.g. from a different document or a different attribute).  
  **Check**: Log or assert in the LSP that `attr_map` keys and the error’s `attribute` use the same string for the file that contains the `@...` reference.

- **Debounce / validation not running**  
  If the debounce task never runs (e.g. no `request_workspace_validation()` after open, or no 250ms wait), we never call `resolve_registry_references` and never publish.  
  If the user only opens a **single file** (no folder), `discover_workspace_files` is not run; the file is only in the workspace via `didOpen`. So we rely on `didOpen` → `update_file` → `request_workspace_validation()` and then the debounce task running.  
  **Check**: Confirm that after opening the file, the debounce task actually runs (e.g. log at the start of the validation block).

- **Empty workspace when task runs**  
  If when the debounce task runs `workspace.files` is empty, `local_docs` and `attr_map` are empty. Then we call `resolve_registry_references` with empty docs (no refs to resolve) and get `Ok(empty)`, so we never hit the Registry error path.  
  **Check**: Ensure the file is still in `workspace.files` when the debounce runs (no race that clears it).

- **Resolution never returns**  
  If `resolve_registry_references` blocks (e.g. network call to LemmaBase never completes), we never reach `attribute_errors_to_files` or `publish_diagnostics`.  
  **Check**: In that case, adding a timeout and falling back to planning without resolution would at least show “Document not found” diagnostics.

- **Editor not showing diagnostics**  
  The client might not be sending diagnostics to the right document, or the editor might be filtering them.  
  **Check**: Inspect LSP traffic (e.g. “Log channel” for the language server) to see if `publish_diagnostics` is sent and for which URI.

## Suggested next step

Add a test in the LSP that:

1. Builds a `WorkspaceModel` with one file (e.g. content of `13_registry_missing.lemma`) and a known `Url`.
2. Builds `attr_map = workspace.attribute_to_url_and_text()`.
3. Builds a `LemmaError::Registry` whose `source_location.attribute` equals the key used for that file in `attr_map` (i.e. `attribute_for_url(&url)`).
4. Calls `attribute_errors_to_files(&error, &attr_map)` and asserts we get exactly one `FileDiagnostics` for that file with one error.

If that test passes, the attribution logic is correct and the bug is elsewhere (debounce, empty workspace, or editor/client).
