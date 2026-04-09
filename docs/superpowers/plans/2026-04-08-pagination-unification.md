# Pagination Unification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate missing or skipped lines between EPUB pages by replacing the current split direct-vs-cached pagination behavior with one shared, authoritative pagination pipeline in `xteink-render`.

**Architecture:** Build a single paginator state machine that owns text buffering, wrapping, deferred break handling, page advancement, and exact consumed-text tracking. Both direct EPUB rendering and cached replay will translate their inputs into the same small event stream and feed that shared paginator, so page boundaries are decided in one place only.

**Tech Stack:** Rust 2024, `no_std` shared renderer in `xteink-render`, fixed-size buffers, host regression tests via `cargo test`, embedded validation via `cargo check` for `riscv32imc-unknown-none-elf`

---

### Task 1: Add focused paginator regressions before refactoring

**Files:**
- Create: `crates/xteink-render/tests/paginator.rs`
- Modify: `crates/xteink-render/tests/epub_render.rs`
- Test: `crates/xteink-render/tests/paginator.rs`
- Test: `crates/xteink-render/tests/epub_render.rs`

- [ ] **Step 1: Write the failing tests**

Add small, explicit regressions for:
- long text split across two pages is continuous
- deferred line break after text is preserved across a page boundary
- deferred paragraph break after text is preserved across a page boundary
- explicit page break starts the next page exactly once
- cached page 1 matches direct page 1 for the same fixture

- [ ] **Step 2: Run the tests to verify they fail**

Run:
- `cargo test -p xteink-render --test paginator --target aarch64-apple-darwin`
- `cargo test -p xteink-render --test epub_render --target aarch64-apple-darwin cached_page_matches_direct_epub_render_for_following_page -- --test-threads=1`

Expected: FAIL because the shared paginator does not exist yet and the current direct/cached paths still diverge.

- [ ] **Step 3: Keep the red tests minimal**

Ensure each test asserts one boundary behavior only. Prefer direct byte equality for framebuffer comparisons and avoid broad fixture sweeps in this task.

- [ ] **Step 4: Run a validity check**

Run: `cargo check -p xteink-render --target aarch64-apple-darwin`
Expected: PASS

### Task 2: Introduce one shared paginator state machine

**Files:**
- Create: `crates/xteink-render/src/paginator.rs`
- Modify: `crates/xteink-render/src/lib.rs`
- Modify: `crates/xteink-render/src/text.rs`
- Test: `crates/xteink-render/tests/paginator.rs`

- [ ] **Step 1: Write the failing test**

Add a unit test that feeds a small event stream into the new paginator API and asserts:
- exact consumed bytes
- page transitions
- cursor position after deferred breaks

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p xteink-render --test paginator --target aarch64-apple-darwin`
Expected: FAIL because `paginator.rs` does not exist yet.

- [ ] **Step 3: Write minimal implementation**

Add a `PaginatorState` with:
- current page index
- current cursor y
- pending post-text action
- current text buffer
- explicit-page-break mode flag if needed

Add a small input enum such as:
- `Text(&str)`
- `LineBreak`
- `ParagraphBreak`
- `PageBreak`
- `End`

Expose a single feed/step API that:
- accepts an event
- flushes text through wrapped layout
- advances pages
- returns consumed text and whether target page is complete

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p xteink-render --test paginator --target aarch64-apple-darwin`
Expected: PASS

- [ ] **Step 5: Run a validity check**

Run: `cargo check -p xteink-render --target aarch64-apple-darwin`
Expected: PASS

### Task 3: Refactor direct EPUB rendering to use only the paginator

**Files:**
- Modify: `crates/xteink-render/src/epub.rs`
- Modify: `crates/xteink-render/src/paginator.rs`
- Test: `crates/xteink-render/tests/epub_render.rs`

- [ ] **Step 1: Write the failing test**

Extend the direct-render regression so it asserts:
- page 0 still renders correctly
- page 1 render remains stable
- no text is lost when a text block spans a page boundary

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p xteink-render --test epub_render --target aarch64-apple-darwin -- --test-threads=1`
Expected: FAIL until direct EPUB rendering is routed through the new paginator.

- [ ] **Step 3: Write minimal implementation**

In `render_epub_with_mode(...)`:
- remove local page-boundary bookkeeping that duplicates pagination rules
- translate `EpubEvent` values into paginator events
- let paginator decide when text is consumed, when deferred breaks apply, and when a page ends
- keep cache emission attached to paginator-consumed text only

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p xteink-render --test epub_render --target aarch64-apple-darwin -- --test-threads=1`
Expected: PASS for direct-render regressions.

- [ ] **Step 5: Run validity checks**

Run:
- `cargo check -p xteink-render --target aarch64-apple-darwin`
- `cargo check -p xteink-reader --features embedded --target riscv32imc-unknown-none-elf -Zbuild-std=core`

Expected: PASS

### Task 4: Change the cache stream to record authoritative paginator output

**Files:**
- Modify: `crates/xteink-render/src/epub.rs`
- Modify: `crates/xteink-render/src/lib.rs`
- Modify: `crates/xteink-render/src/paginator.rs`
- Test: `crates/xteink-render/tests/epub_render.rs`

- [ ] **Step 1: Write the failing test**

Add a regression proving the cache stream:
- preserves exact consumed text order
- preserves explicit page boundaries
- preserves deferred line and paragraph spacing semantics

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p xteink-render --test epub_render --target aarch64-apple-darwin cached_page_matches_direct_epub_render_for_following_page -- --test-threads=1`
Expected: FAIL until the cache stream is derived purely from paginator decisions.

- [ ] **Step 3: Write minimal implementation**

Adjust cache emission so:
- text chunks are emitted only after the paginator reports them consumed
- break markers are emitted only as authoritative paginator output
- page-break markers are emitted exactly where paginator advances pages

Do not emit speculative or pre-layout markers.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p xteink-render --test epub_render --target aarch64-apple-darwin cached_page_matches_direct_epub_render_for_following_page -- --test-threads=1`
Expected: PASS

- [ ] **Step 5: Run a validity check**

Run: `cargo check -p xteink-render --target aarch64-apple-darwin`
Expected: PASS

### Task 5: Refactor cached replay to use the same paginator and remove fallback heuristics

**Files:**
- Modify: `crates/xteink-render/src/pagination.rs`
- Modify: `crates/xteink-render/src/lib.rs`
- Modify: `crates/xteink-render/src/paginator.rs`
- Test: `crates/xteink-render/tests/paginator.rs`
- Test: `crates/xteink-render/tests/epub_render.rs`

- [ ] **Step 1: Write the failing test**

Add a regression proving cached replay:
- consumes the authoritative event stream without height-based drift
- renders sequential pages identically to direct rendering for at least pages 0 and 1

- [ ] **Step 2: Run the test to verify it fails**

Run:
- `cargo test -p xteink-render --test paginator --target aarch64-apple-darwin`
- `cargo test -p xteink-render --test epub_render --target aarch64-apple-darwin -- --test-threads=1`

Expected: FAIL until cached replay delegates fully to the shared paginator.

- [ ] **Step 3: Write minimal implementation**

Make cached replay:
- decode cache events
- feed them into the shared paginator
- stop using local page-height heuristics as an alternative source of page boundaries once explicit paginator output exists

Delete or collapse duplicated cached-page logic that is now redundant.

- [ ] **Step 4: Run the tests to verify they pass**

Run:
- `cargo test -p xteink-render --test paginator --target aarch64-apple-darwin`
- `cargo test -p xteink-render --test epub_render --target aarch64-apple-darwin -- --test-threads=1`

Expected: PASS

- [ ] **Step 5: Run a validity check**

Run: `cargo check -p xteink-render --target aarch64-apple-darwin`
Expected: PASS

### Task 6: Remove obsolete pagination paths and stabilize the final API

**Files:**
- Modify: `crates/xteink-render/src/lib.rs`
- Modify: `crates/xteink-render/src/pagination.rs`
- Modify: `crates/xteink-render/src/epub.rs`
- Test: `crates/xteink-render/tests/paginator.rs`
- Test: `crates/xteink-render/tests/epub_render.rs`

- [ ] **Step 1: Write the failing test**

Add a regression that exercises a multi-page cached render sequence and ensures there is no per-page text loss or duplication across several consecutive pages.

- [ ] **Step 2: Run the test to verify it fails if cleanup is incomplete**

Run: `cargo test -p xteink-render --test epub_render --target aarch64-apple-darwin -- --test-threads=1`
Expected: FAIL only if legacy code paths still interfere.

- [ ] **Step 3: Write minimal implementation**

Remove no-longer-needed state and helpers:
- duplicated page-boundary state machines
- old marker interpretation branches that are no longer authoritative
- ad hoc fallback semantics that are now dead code

Keep the public renderer API stable unless a smaller change is clearly safer.

- [ ] **Step 4: Run the tests to verify they pass**

Run:
- `cargo test -p xteink-render --test paginator --target aarch64-apple-darwin`
- `cargo test -p xteink-render --test epub_render --target aarch64-apple-darwin -- --test-threads=1`

Expected: PASS

- [ ] **Step 5: Run full validity checks**

Run:
- `cargo check --workspace --target aarch64-apple-darwin`
- `cargo check -p xteink-reader --features embedded --target riscv32imc-unknown-none-elf -Zbuild-std=core`

Expected: PASS

### Task 7: Verify on real fixture behavior and prepare the device check

**Files:**
- Modify: `crates/xteink-render/tests/epub_render.rs`
- Optional docs note: `docs/PROJECT_OVERVIEW.md`

- [ ] **Step 1: Add final end-to-end regressions**

Ensure the final suite includes:
- direct vs cached equality for page 0
- direct vs cached equality for page 1
- sequential cached navigation across several pages without gaps

- [ ] **Step 2: Run final host tests**

Run:
- `cargo test -p xteink-render --test epub_render --target aarch64-apple-darwin -- --test-threads=1`
- `cargo test -p xteink-render --test paginator --target aarch64-apple-darwin`

Expected: PASS

- [ ] **Step 3: Run final embedded validity check**

Run: `cargo check -p xteink-reader --features embedded --target riscv32imc-unknown-none-elf -Zbuild-std=core`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/xteink-render/src/lib.rs \
        crates/xteink-render/src/text.rs \
        crates/xteink-render/src/epub.rs \
        crates/xteink-render/src/pagination.rs \
        crates/xteink-render/src/paginator.rs \
        crates/xteink-render/tests/paginator.rs \
        crates/xteink-render/tests/epub_render.rs \
        docs/superpowers/plans/2026-04-08-pagination-unification.md
git commit -m "unify direct and cached pagination"
```
