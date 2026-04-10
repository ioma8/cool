# Offset-Window Pagination Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the broken page-number/prefix-cache EPUB flow with a strict offset-window reader model using full `content.txt`, `chapters.idx`, and persisted `previous/current/next` page offsets.

**Architecture:** `xteink-fs` becomes the authoritative owner of cached EPUB state and reader progress. `xteink-render` keeps the cached-text paginator but is exposed through offset-centric APIs, while firmware/simulator/web call sites are adapted to the new offset-window results without reintroducing page-number truth.

**Tech Stack:** Rust `no_std` workspace, `xteink-fs`, `xteink-render`, `xteink-epub`, simulator host tests, Cargo test

---

## File Map

**Create:**
- `docs/superpowers/plans/2026-04-10-offset-window-pagination.md`

**Modify:**
- `crates/xteink-fs/src/cache.rs`
- `crates/xteink-fs/src/reader.rs`
- `crates/xteink-fs/tests/cache_paths.rs`
- `crates/xteink-render/src/epub.rs`
- `crates/xteink-render/src/lib.rs`
- `crates/xteink-render/tests/epub_render.rs`
- `simulator/tests/storage.rs`
- `simulator/src/storage.rs`
- `web-simulator/src/lib.rs`
- `firmware/src/main.rs`
- `crates/xteink-app/src/session.rs`
- `crates/xteink-app/tests/session.rs`

**Responsibilities:**
- `crates/xteink-fs/src/cache.rs`: remove `pages.idx`, redefine persisted progress as offset-window state, update cache path helpers and binary encoding.
- `crates/xteink-fs/src/reader.rs`: replace page-index resume/render flow with offset-window navigation, full-cache build, chapter-offset lookup, and progress calculation.
- `crates/xteink-render/src/epub.rs`: expose full-book cache build path and chapter-offset emission needed by `chapters.idx`.
- `crates/xteink-render/src/lib.rs`: keep cached replay logic, but add/clarify offset-window render helpers and EOF detection semantics.
- `simulator/tests/storage.rs`: add tests for offset-window correctness instead of page-number assumptions.
- wrapper call sites (`simulator`, `web-simulator`, `firmware`, `xteink-app`): adapt to new reader result shape while preserving UI behavior.

### Task 1: Redefine Cache Metadata And Progress Encoding

**Files:**
- Modify: `crates/xteink-fs/src/cache.rs`
- Test: `crates/xteink-fs/tests/cache_paths.rs`

- [ ] **Step 1: Write the failing test**

Add tests that assert:
- `CachePaths` no longer includes `pages`
- `ProgressState` round-trips three offsets: previous/current/next
- old `current_page_hint` assumptions are removed from the type and encoding

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p xteink-fs cache_paths -- --nocapture`
Expected: FAIL because `pages.idx` and the old progress encoding are still present

- [ ] **Step 3: Write minimal implementation**

In `crates/xteink-fs/src/cache.rs`:
- remove `PAGES_FILE_NAME`
- remove `pages` from `CachePaths`
- redefine `ProgressState` to:

```rust
pub struct ProgressState {
    pub previous_page_start_offset: u64,
    pub current_page_start_offset: u64,
    pub next_page_start_offset: u64,
}
```

- update `encode_progress` / `decode_progress` to use 24 bytes
- update path helper tests accordingly

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p xteink-fs cache_paths -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run project validity check**

Run: `cargo check -p xteink-fs`
Expected: PASS or compile errors only in downstream crates that still use the old API

- [ ] **Step 6: Commit**

```bash
git add crates/xteink-fs/src/cache.rs crates/xteink-fs/tests/cache_paths.rs
git commit -m "refactor: store epub progress as offset window"
```

### Task 2: Replace Full/Prefix Cache Build With Full Cached Text + Chapter Offsets

**Files:**
- Modify: `crates/xteink-render/src/epub.rs`
- Modify: `crates/xteink-render/tests/epub_render.rs`

- [ ] **Step 1: Write the failing test**

Add tests that assert:
- cache build emits the full cached text for a real fixture, not only a prefix
- chapter metadata is emitted as byte offsets into cached text
- early-page rendering no longer relies on `cached_pages` or partial-prefix progress

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p xteink-render epub_render -- --nocapture`
Expected: FAIL because cache build still stops after a prefix/chapter-boundary strategy

- [ ] **Step 3: Write minimal implementation**

In `crates/xteink-render/src/epub.rs`:
- replace `build_epub_cache_prefix_with_text_sink_and_cancel` usage path with a full-book cache build API
- emit chapter boundary offsets during full linearization
- remove `cached_pages`, `resume_page`, `resume_cursor_y`, and cached-prefix progress assumptions from the public result type if no longer needed
- keep output bounded to what `xteink-fs` needs: content length, chapter offsets, completion

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p xteink-render epub_render -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run project validity check**

Run: `cargo check -p xteink-render`
Expected: PASS or compile errors only in downstream crates that still use removed fields

- [ ] **Step 6: Commit**

```bash
git add crates/xteink-render/src/epub.rs crates/xteink-render/tests/epub_render.rs
git commit -m "refactor: build full cached epub text and chapter offsets"
```

### Task 3: Introduce Offset-Window Rendering In `xteink-fs`

**Files:**
- Modify: `crates/xteink-fs/src/reader.rs`

- [ ] **Step 1: Write the failing test**

Add tests in the most appropriate existing test target that assert:
- first render from cold open returns `current=0`
- first render computes a nonzero `next`
- progress for first page is exactly `0`
- next-page navigation advances by offset, not page number

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p simulator --test storage -- --nocapture`
Expected: FAIL because reader flow is still page-index based

- [ ] **Step 3: Write minimal implementation**

In `crates/xteink-fs/src/reader.rs`:
- remove all `pages.idx` read/write logic
- remove `current_page_hint` / page-index-based resume
- replace `render_epub_page_from_entry*` internals with offset-window state:

```rust
struct OffsetWindow {
    previous_page_start_offset: u64,
    current_page_start_offset: u64,
    next_page_start_offset: u64,
}
```

- on cold open, load `progress.bin` or initialize zeros
- render from `current_page_start_offset`
- on next page, shift `previous=current`, `current=next`, recompute new `next`
- compute progress from `current / content_length`, with page zero forced to `0%`
- persist the new offset window after each successful render

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p simulator --test storage -- --nocapture`
Expected: relevant storage tests PASS

- [ ] **Step 5: Run project validity check**

Run: `cargo check -p xteink-fs`
Expected: PASS or downstream compile errors only

- [ ] **Step 6: Commit**

```bash
git add crates/xteink-fs/src/reader.rs
git commit -m "refactor: drive epub reader from persisted offset window"
```

### Task 4: Make Previous-Page Navigation Exact And Fallback-Safe

**Files:**
- Modify: `crates/xteink-fs/src/reader.rs`
- Test: `simulator/tests/storage.rs`

- [ ] **Step 1: Write the failing test**

Add tests that assert:
- after rendering page 0 then page 1, a back turn returns exactly to page 0 content
- if persisted `previous_page_start_offset` is invalid, fallback scan from chapter start or `0` still finds the correct previous page

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p simulator --test storage -- --nocapture`
Expected: FAIL because previous-page behavior is not yet based on exact offset windows

- [ ] **Step 3: Write minimal implementation**

Implement in `crates/xteink-fs/src/reader.rs`:
- direct back-turn from persisted `previous_page_start_offset`
- fallback path that finds nearest chapter boundary from `chapters.idx` and scans forward page-by-page using cached text replay
- offset validity checks against `content_length`

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p simulator --test storage -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run project validity check**

Run: `cargo check -p simulator`
Expected: PASS or only wrapper API errors still pending in other crates

- [ ] **Step 6: Commit**

```bash
git add crates/xteink-fs/src/reader.rs simulator/tests/storage.rs
git commit -m "feat: support exact previous page turns with offset fallback"
```

### Task 5: Rework Progress Semantics And EOF Detection

**Files:**
- Modify: `crates/xteink-fs/src/reader.rs`
- Modify: `crates/xteink-render/src/lib.rs`
- Test: `simulator/tests/storage.rs`

- [ ] **Step 1: Write the failing test**

Add tests that assert:
- first page always shows `0%`
- intermediate pages are `< 100%`
- only the terminal page may show `100%`
- early pages of the real fixture no longer spike to `99%`

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p simulator --test storage -- --nocapture`
Expected: FAIL until percent calculation uses offset-window + EOF semantics

- [ ] **Step 3: Write minimal implementation**

In `crates/xteink-fs/src/reader.rs` and `crates/xteink-render/src/lib.rs`:
- expose enough information from cached page render to detect terminal page / EOF
- compute progress strictly from `current_page_start_offset` and `content_length`
- force `0%` when `current == 0`
- clamp nonterminal pages to `99%`
- allow `100%` only when the current page is terminal and `next == content_length`

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p simulator --test storage -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run project validity check**

Run: `cargo check -p xteink-fs -p xteink-render -p simulator`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/xteink-fs/src/reader.rs crates/xteink-render/src/lib.rs simulator/tests/storage.rs
git commit -m "fix: compute epub progress from page start offsets"
```

### Task 6: Adapt App/Firmware/Simulator/Web Call Sites To Offset-Window Results

**Files:**
- Modify: `crates/xteink-app/src/session.rs`
- Modify: `crates/xteink-app/tests/session.rs`
- Modify: `simulator/src/storage.rs`
- Modify: `web-simulator/src/lib.rs`
- Modify: `firmware/src/main.rs`

- [ ] **Step 1: Write the failing test**

Add or update app/session tests asserting the reader API still supports:
- open current book
- next page
- previous page
- chapter jump
- footer percent display

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p xteink-app -- --nocapture`
Expected: FAIL until wrapper traits and mocks match the new reader shape

- [ ] **Step 3: Write minimal implementation**

Update call sites to:
- stop treating page number as authoritative persisted state
- consume new offset-window-based render result fields
- keep UI-facing `rendered_page` only if still needed locally for display/navigation bookkeeping
- ensure firmware/simulator/web read the updated progress semantics without extra translation

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p xteink-app -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run project validity check**

Run: `cargo check -p xteink-app -p simulator -p firmware`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/xteink-app/src/session.rs crates/xteink-app/tests/session.rs simulator/src/storage.rs web-simulator/src/lib.rs firmware/src/main.rs
git commit -m "refactor: adapt reader clients to offset-window pagination"
```

### Task 7: End-To-End Verification

**Files:**
- Modify: `simulator/tests/storage.rs`
- Modify: `crates/xteink-render/tests/epub_render.rs`

- [ ] **Step 1: Write the failing test**

Add end-to-end regression tests covering:
- cold open on real fixture
- several next-page turns without blank pages
- previous-page exact return
- chapter jump from `chapters.idx`
- progress monotonicity without early spikes

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p simulator --test storage -- --nocapture`
Expected: FAIL before all subsystems are integrated

- [ ] **Step 3: Write minimal implementation**

Make only the smallest integration fixes needed after the system is already mostly in place. Avoid new refactors here; this task is for closing remaining gaps revealed by end-to-end tests.

- [ ] **Step 4: Run tests to verify they pass**

Run:
- `cargo test -p xteink-render -- --nocapture`
- `cargo test -p xteink-fs -- --nocapture`
- `cargo test -p xteink-app -- --nocapture`
- `cargo test -p simulator --test storage -- --nocapture`

Expected: PASS

- [ ] **Step 5: Run final validity checks**

Run:
- `cargo check`
- `git diff --check`

Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/xteink-render/tests/epub_render.rs simulator/tests/storage.rs
git commit -m "test: cover offset-window epub pagination end to end"
```

## Notes For Execution

- Keep iterations small. Do not bundle cache format changes, render API changes, and wrapper updates into one commit.
- After every code change, run `cargo check` or the narrowest relevant `cargo test` target before proceeding.
- Prefer deleting obsolete page-number state instead of adapting it.
- Do not reintroduce `pages.idx` as an optimization during this rewrite.
- When uncertain, preserve the invariant that `current_page_start_offset` is the only reader truth.
