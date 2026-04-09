# Byte-Offset EPUB Reader Cache Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the current EPUB cache-prefix/page-heuristic reader flow with a `content.txt`-backed byte-offset reader model shared by simulator and firmware, while preserving early page-0 render and current RAM limits.

**Architecture:** EPUB parsing becomes a cache builder that writes normalized UTF-8 text into `content.txt`, chapter byte offsets into `chapters.idx`, and later page byte offsets into `pages.idx`. Reader reopen, page rendering, and progress all become byte-offset based over cached text, with `progress.bin` storing the authoritative current byte offset and `meta.txt` tracking build/layout validity.

**Tech Stack:** Rust 2024, `no_std` EPUB parser in `crates/xteink-epub`, shared framebuffer renderer in `crates/xteink-render`, shared cache/reader orchestration in `crates/xteink-fs`, simulator/firmware via shared `xteink-app` session flow

**Current checkpoint:** `crates/xteink-render` now has the byte-offset cached-text renderer and tests passing. The remaining work is wiring `crates/xteink-fs` progress/resume onto byte offsets, then propagating that into simulator and firmware storage adapters. The `pages.idx` / `chapters.idx` sidecars are still planned, not implemented yet.

---

## File Structure

**Existing files to modify**
- `crates/xteink-fs/src/cache.rs`
  - redefine cache metadata around byte-offset cache build state and new sidecar paths
- `crates/xteink-fs/src/reader.rs`
  - replace page/chapter heuristic reader flow with `content.txt` / `pages.idx` / `progress.bin` orchestration
- `crates/xteink-render/src/lib.rs`
  - expose rendering over cached text from a byte offset and page-offset discovery hooks
- `crates/xteink-render/src/paginator.rs`
  - emit exact page-start byte offsets while paginating cached text
- `crates/xteink-render/src/text.rs`
  - preserve exact consumed-byte accounting for UTF-8 cached text replay
- `crates/xteink-render/src/epub.rs`
  - reduce runtime responsibility to EPUB-to-text-cache production, not long-term progress heuristics
- `simulator/src/storage.rs`
  - stay a low-level FS adapter only; remove any remaining progress assumptions tied to page numbers
- `crates/xteink-app/src/session.rs`
  - keep using the shared reader API while adapting to byte-offset-backed progress semantics if signatures change

**Existing test files to modify**
- `crates/xteink-fs/tests/cache_paths.rs`
- `crates/xteink-render/tests/epub_render.rs`
- `simulator/tests/storage.rs`
- `simulator/tests/runtime.rs`

**New files likely to add**
- `crates/xteink-fs/tests/reader_cache.rs`
  - focused host tests for `content.txt` / `chapters.idx` / `pages.idx` / `progress.bin`

---

### Task 1: Redefine cache metadata and sidecar paths around byte offsets

**Files:**
- Modify: `crates/xteink-fs/src/cache.rs`
- Test: `crates/xteink-fs/tests/cache_paths.rs`

- [ ] **Step 1: Write the failing test**

Add tests in `crates/xteink-fs/tests/cache_paths.rs` that assert:
- cache paths include `content.txt`, `chapters.idx`, `pages.idx`, `progress.bin`, `meta.txt`
- `CacheMeta` round-trips the new fields:
  - `version`
  - `source_size`
  - `content_length`
  - `build_complete`
  - `next_chapter_index`
  - layout signature fields
- `progress.bin` helpers round-trip:
  - `current_byte_offset`
  - `current_page_hint`

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p xteink-fs --test cache_paths --target aarch64-apple-darwin`
Expected: FAIL because the current cache model still uses `cached_pages`, `cached_progress_percent`, and old progress encoding.

- [ ] **Step 3: Write minimal implementation**

In `crates/xteink-fs/src/cache.rs`:
- add new constants:
  - `CHAPTERS_FILE_NAME`
  - `PAGES_FILE_NAME`
- expand `CachePaths` with:
  - `chapters`
  - `pages`
- replace `CacheMeta` fields with the byte-offset model:
  - `version: u8`
  - `source_size: u32`
  - `content_length: u64`
  - `build_complete: bool`
  - `next_chapter_index: u16`
  - `layout_sig_version: u16`
  - `layout_sig_width: u16`
  - `layout_sig_height: u16`
  - `layout_sig_content_height: u16`
  - `layout_sig_font: u32`
  - `layout_sig_paginator: u32`
- add `ProgressState` for `progress.bin`
- add little-endian encode/decode helpers for:
  - `progress.bin`
  - `chapters.idx` record (`u64`)
  - `pages.idx` record (`u64`)
- bump `CACHE_VERSION`

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p xteink-fs --test cache_paths --target aarch64-apple-darwin`
Expected: PASS

- [ ] **Step 5: Run validity check**

Run: `cargo check -p xteink-fs --target aarch64-apple-darwin`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/xteink-fs/src/cache.rs crates/xteink-fs/tests/cache_paths.rs
git commit -m "redefine reader cache metadata for byte offsets"
```

### Task 2: Add byte-offset cached-text rendering primitives

**Files:**
- Modify: `crates/xteink-render/src/lib.rs`
- Modify: `crates/xteink-render/src/paginator.rs`
- Modify: `crates/xteink-render/src/text.rs`
- Test: `crates/xteink-render/tests/epub_render.rs`

- [ ] **Step 1: Write the failing test**

Add tests in `crates/xteink-render/tests/epub_render.rs` that assert:
- rendering cached text from a byte offset produces the same page image as rendering the same prefix through the existing cached stream path
- paginating from a byte offset reports the next page start byte offset
- progress-relevant consumed byte counts match the actual UTF-8 byte position of the rendered page start

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p xteink-render --test epub_render --target aarch64-apple-darwin -- --test-threads=1`
Expected: FAIL because the current cached replay API is page-number oriented and does not return byte offsets.

- [ ] **Step 3: Write minimal implementation**

In `crates/xteink-render/src/lib.rs` and `crates/xteink-render/src/paginator.rs`:
- add a cached-text page render API shaped like:
  - `render_cached_text_page_from_offset(...) -> { page_start_byte, next_page_start_byte, rendered_page, consumed_bytes }`
- ensure the paginator tracks exact UTF-8 consumed bytes from the cached stream
- expose the discovered next page start byte offset whenever a boundary is crossed
- keep footer-reserved content height behavior unchanged

In `crates/xteink-render/src/text.rs`:
- verify consumed-byte accounting remains exact for partial UTF-8 chunks and wrapped lines
- do not regress the previously fixed missing-line behavior

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p xteink-render --test epub_render --target aarch64-apple-darwin -- --test-threads=1`
Expected: PASS

- [ ] **Step 5: Run validity check**

Run: `cargo check -p xteink-render --target aarch64-apple-darwin`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/xteink-render/src/lib.rs crates/xteink-render/src/paginator.rs crates/xteink-render/src/text.rs crates/xteink-render/tests/epub_render.rs
git commit -m "add byte-offset cached text rendering"
```

### Task 3: Convert EPUB rendering to text-cache production only

**Files:**
- Modify: `crates/xteink-render/src/epub.rs`
- Test: `crates/xteink-render/tests/epub_render.rs`

- [ ] **Step 1: Write the failing test**

Add a regression in `crates/xteink-render/tests/epub_render.rs` that asserts:
- cold EPUB cache build emits `content.txt` bytes and chapter offsets
- it can stop once page `0` is renderable
- it no longer needs page/chapter percentage heuristics from the EPUB parser

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p xteink-render --test epub_render --target aarch64-apple-darwin cold_open_prefix_render_matches_direct_first_page -- --test-threads=1`
Expected: FAIL or require API changes because current `CacheBuildResult` still carries page-heuristic progress fields.

- [ ] **Step 3: Write minimal implementation**

In `crates/xteink-render/src/epub.rs`:
- remove `rendered_progress_percent` / `cached_progress_percent` responsibilities from EPUB cache build results
- redefine `CacheBuildResult` around byte offsets and build state:
  - `rendered_page`
  - `content_length_built`
  - `next_chapter_index`
  - `build_complete`
  - `rendered_page_start_byte`
  - `next_page_start_byte_if_known`
- continue to emit normalized text into the sink
- emit chapter boundaries through a second callback or structured observer so `chapters.idx` can be written
- stop cold-open work once page `0` is renderable, but allow caller-controlled continuation for the rest of the build

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p xteink-render --test epub_render --target aarch64-apple-darwin -- --test-threads=1`
Expected: PASS

- [ ] **Step 5: Run validity checks**

Run:
- `cargo check -p xteink-render --target aarch64-apple-darwin`
- `cargo test -p xteink-epub --test task3 --target aarch64-apple-darwin`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/xteink-render/src/epub.rs crates/xteink-render/tests/epub_render.rs
git commit -m "make epub render path build byte-offset text cache"
```

### Task 4: Add shared cache reader tests for `content.txt` / `chapters.idx` / `pages.idx`

**Files:**
- Create: `crates/xteink-fs/tests/reader_cache.rs`
- Modify: `crates/xteink-fs/src/reader.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/xteink-fs/tests/reader_cache.rs` with host-side tests that assert:
- cold miss creates all sidecars:
  - `content.txt`
  - `chapters.idx`
  - `pages.idx`
  - `progress.bin`
  - `meta.txt`
- `content.txt` is non-empty after page `0` render
- `pages.idx` record `0` is byte offset `0`
- resume from saved byte offset reopens the same page prefix
- stale `pages.idx` layout signature is ignored and rebuilt from `content.txt`
- missing or corrupt `chapters.idx` during an incomplete build forces a restart from chapter `0`
- missing or stale `meta.txt` makes `progress.bin` untrusted and forces reopen to ignore saved byte offset

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p xteink-fs --test reader_cache --target aarch64-apple-darwin`
Expected: FAIL because `reader.rs` still uses the old cache selection and progress semantics.

- [ ] **Step 3: Write minimal implementation scaffold**

In `crates/xteink-fs/src/reader.rs`:
- add small helpers for:
  - reading/writing `chapters.idx`
  - reading/writing `pages.idx`
  - reading/writing `progress.bin`
  - validating layout signature
- add one `CacheState` resolver that returns:
  - `Miss`
  - `Building`
  - `Ready`
  - `Stale`

Spell out the decision table in code comments and tests:
- `Miss`: no trusted `content.txt` or no trusted `meta.txt`
- `Building`: `content.txt` exists, `build_complete == false`, and required sidecars are valid
- `Ready`: `content.txt`, `pages.idx`, and `meta.txt` are valid for the current source/layout
- `Stale`: any sidecar mismatch, invalid layout/version, or missing required sidecar; `progress.bin` must be ignored

Do not fully switch open/page-turn paths yet; this task is scaffolding plus tests.

- [ ] **Step 4: Run test to verify it partially passes**

Run: `cargo test -p xteink-fs --test reader_cache --target aarch64-apple-darwin`
Expected: PASS for the new helpers that are already implemented, or reduced failures isolated to the main render flow.

- [ ] **Step 5: Run validity check**

Run: `cargo check -p xteink-fs --target aarch64-apple-darwin`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/xteink-fs/src/reader.rs crates/xteink-fs/tests/reader_cache.rs
git commit -m "add byte-offset cache reader scaffolding"
```

### Task 5: Switch cold-open and page-turn orchestration to `content.txt` / `pages.idx`

**Files:**
- Modify: `crates/xteink-fs/src/reader.rs`
- Modify: `crates/xteink-app/src/session.rs`
- Test: `crates/xteink-fs/tests/reader_cache.rs`
- Test: `simulator/tests/storage.rs`

- [ ] **Step 1: Write the failing test**

Add tests that assert:
- cold open renders page `0` from cached text once bytes exist in `content.txt`
- page turns use `content.txt` and `pages.idx` only
- reopen uses `progress.bin.current_byte_offset`
- no page/chapter heuristic progress path remains

- [ ] **Step 2: Run test to verify it fails**

Run:
- `cargo test -p xteink-fs --test reader_cache --target aarch64-apple-darwin`
- `cargo test -p simulator --test storage --target aarch64-apple-darwin`
Expected: FAIL because current open/page-turn still branch on old cache meta and page-index assumptions.

- [ ] **Step 3: Write minimal implementation**

In `crates/xteink-fs/src/reader.rs`:
- on cache miss:
  - start EPUB-to-cache build
  - write `chapters.idx`
  - append `content.txt`
  - append `pages.idx` as page boundaries are discovered
  - render page `0` from `content.txt`
- on cache hit or partial build:
  - render pages only from `content.txt` using `pages.idx`
  - if target page start is missing, extend `pages.idx` by paginating cached text forward
- on reopen:
  - read `progress.bin.current_byte_offset`
  - resolve to floor page start in `pages.idx`
  - render from that byte offset
- persist back:
  - authoritative `current_byte_offset`
  - optional `current_page_hint`

In `crates/xteink-app/src/session.rs`:
- adapt to any API changes in `EpubRenderResult` if it now returns byte-offset-backed progress or page hints

- [ ] **Step 4: Run test to verify it passes**

Run:
- `cargo test -p xteink-fs --test reader_cache --target aarch64-apple-darwin`
- `cargo test -p simulator --test storage --target aarch64-apple-darwin`
Expected: PASS

- [ ] **Step 5: Run validity checks**

Run:
- `cargo check -p xteink-app --target aarch64-apple-darwin`
- `cargo check -p xteink-reader --features embedded --target riscv32imc-unknown-none-elf -Zbuild-std=core`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/xteink-fs/src/reader.rs crates/xteink-app/src/session.rs crates/xteink-fs/tests/reader_cache.rs simulator/tests/storage.rs
git commit -m "switch reader flow to content txt and page offsets"
```

### Task 6: Replace progress with exact byte-offset semantics

**Files:**
- Modify: `crates/xteink-fs/src/reader.rs`
- Modify: `crates/xteink-app/src/session_ui.rs`
- Test: `simulator/tests/storage.rs`
- Test: `simulator/tests/runtime.rs`

- [ ] **Step 1: Write the failing test**

Add tests that assert:
- early `Decisive` progress is monotonic
- progress never drops after cache extension
- reopen restores the same progress as the saved byte offset implies
- incomplete-build UI does not show a misleading final-book numeric percent if `content_length` is not yet final

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p simulator --test storage --test runtime --target aarch64-apple-darwin`
Expected: FAIL because current progress UI still expects a numeric percentage in all states.

- [ ] **Step 3: Write minimal implementation**

In `crates/xteink-fs/src/reader.rs`:
- compute numeric percent only when `build_complete == true`
- otherwise return a distinct incomplete-build progress state

In `crates/xteink-app/src/session_ui.rs`:
- render:
  - exact numeric percent when the book is fully cached
  - “building cache” footer state when it is not yet fully cached

Keep the footer layout and reserved content height unchanged.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p simulator --test storage --test runtime --target aarch64-apple-darwin`
Expected: PASS

- [ ] **Step 5: Run validity checks**

Run:
- `cargo test -p xteink-app --target aarch64-apple-darwin`
- `cargo check -p xteink-reader --features embedded --target riscv32imc-unknown-none-elf -Zbuild-std=core`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/xteink-fs/src/reader.rs crates/xteink-app/src/session_ui.rs simulator/tests/storage.rs simulator/tests/runtime.rs
git commit -m "make reader progress byte-offset based"
```

### Task 7: Remove obsolete cache-prefix/page-heuristic code

**Files:**
- Modify: `crates/xteink-fs/src/reader.rs`
- Modify: `crates/xteink-render/src/epub.rs`
- Modify: `crates/xteink-fs/src/cache.rs`
- Test: `cargo test --workspace --target aarch64-apple-darwin`

- [ ] **Step 1: Write the failing test**

Add or update regression coverage to prove the old fields and behavior are gone:
- no use of `cached_pages`
- no use of `cached_progress_percent`
- no use of `resume_page` / `resume_cursor_y` for reading semantics

The concrete check can be compile-time or test-only depending on what is easiest to verify cleanly.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --workspace --target aarch64-apple-darwin`
Expected: FAIL while stale code paths still exist.

- [ ] **Step 3: Write minimal implementation**

Remove obsolete:
- page-count-based cache metadata
- chapter/page heuristic percent helpers
- old resume metadata fields tied to paginator internal state
- old test assumptions that progress must change every single page

Leave only the byte-offset cache model.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --workspace --target aarch64-apple-darwin`
Expected: PASS

- [ ] **Step 5: Run embedded validity check**

Run: `cargo check -p xteink-reader --features embedded --target riscv32imc-unknown-none-elf -Zbuild-std=core`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/xteink-fs/src/reader.rs crates/xteink-render/src/epub.rs crates/xteink-fs/src/cache.rs
git commit -m "remove obsolete epub cache heuristics"
```

### Task 8: Final end-to-end verification and cache migration proof

**Files:**
- Test only

- [ ] **Step 1: Run workspace tests**

Run: `cargo test --workspace --target aarch64-apple-darwin`
Expected: PASS

- [ ] **Step 2: Run embedded build validation**

Run: `cargo check -p xteink-reader --features embedded --target riscv32imc-unknown-none-elf -Zbuild-std=core`
Expected: PASS

- [ ] **Step 3: Run focused simulator regressions**

Run:
- `cargo test -p simulator --test runtime --test storage --target aarch64-apple-darwin`
- `cargo test -p xteink-render --test epub_render --target aarch64-apple-darwin -- --test-threads=1`
Expected: PASS

- [ ] **Step 4: Manually verify cache migration**

Manual check:
- start with an old `/.cool` cache
- open an EPUB
- confirm old cache is ignored via version mismatch
- confirm new `content.txt`, `chapters.idx`, `pages.idx`, `progress.bin`, and `meta.txt` are created

- [ ] **Step 5: Commit final verification-only adjustments if any**

```bash
git add -A
git commit -m "verify byte-offset reader cache migration"
```
