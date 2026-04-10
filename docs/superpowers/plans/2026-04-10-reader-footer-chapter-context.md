# Reader Footer Chapter Context Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render the current 1-indexed chapter number on the left of the reader footer, the cropped current chapter title in the middle, and the existing progress percent on the right.

**Architecture:** Keep chapter lookup in `xteink-fs`, derived from `progress.bin.current_page_start_offset` and parsed `chapters.idx`. Keep footer layout in `xteink-render`, where left and right segments are measured first and the title is hard-cropped to the remaining width while preserving at least one character of spacing on both sides.

**Tech Stack:** Rust, `heapless`, shared framebuffer rendering in `xteink-render`, cache metadata parsing in `xteink-fs`, simulator integration tests.

---

## File Map

- Modify: `crates/xteink-fs/src/cache.rs`
  - Add a small reader for the `CHP1` chapter metadata format if not already present.
- Modify: `crates/xteink-fs/src/reader.rs`
  - Resolve the active chapter from `current_page_start_offset`.
  - Build footer context from chapter number, title, and progress.
  - Pass plain footer data into the render path.
- Modify: `crates/xteink-render/src/lib.rs`
  - Add a footer rendering API that accepts left, middle, and right text segments.
  - Implement width reservation and title cropping.
- Modify: `crates/xteink-render/tests/epub_render.rs`
  - Add a focused footer layout test.
- Modify: `simulator/tests/storage.rs`
  - Add an integration test proving chapter footer data appears on a real cached EPUB fixture.
- Modify: `README.md`
  - Document the footer behavior after implementation.

### Task 1: Add Footer Layout Unit Coverage

**Files:**
- Modify: `crates/xteink-render/tests/epub_render.rs`
- Reference: `crates/xteink-render/src/lib.rs`

- [ ] **Step 1: Write the failing footer layout test**

Add a test that renders a footer with:
- left: `"12"`
- middle: a long chapter title
- right: `"37%"`

Assert:
- left text starts at the left footer margin
- right text remains visible at the right footer edge
- the middle title is present only within the remaining width
- there is at least one character-cell gap between left and middle and between middle and right

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p xteink-render --test epub_render footer -- --nocapture`
Expected: FAIL because no footer API exists for three-segment chapter/progress layout

- [ ] **Step 3: Implement minimal footer layout support**

In `crates/xteink-render/src/lib.rs`:
- add a small footer render helper or result type that accepts `left`, `middle`, `right`
- measure left and right text first
- reserve one character-cell space on each side of the title
- hard-crop the title to the remaining width
- render left, cropped middle, and right on a single line

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p xteink-render --test epub_render footer -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run validity check**

Run: `cargo check`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/xteink-render/src/lib.rs crates/xteink-render/tests/epub_render.rs
git commit -m "feat: add chapter-aware footer layout"
```

### Task 2: Add Chapter Resolution Unit Coverage

**Files:**
- Modify: `crates/xteink-fs/src/cache.rs`
- Modify: `crates/xteink-fs/src/reader.rs`
- Modify: `crates/xteink-fs/tests/cache_paths.rs`

- [ ] **Step 1: Write the failing chapter-resolution test**

Add a test that constructs parsed chapter metadata such as:
- `(0, "Introduction")`
- `(1024, "First chapter")`
- `(4096, "Second chapter")`

Assert:
- offset `0` resolves to chapter `1`, `"Introduction"`
- offset `1500` resolves to chapter `2`, `"First chapter"`
- offset `999999` resolves to the last matching chapter

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p xteink-fs chapter -- --nocapture`
Expected: FAIL because active chapter resolution helper does not exist yet

- [ ] **Step 3: Implement minimal chapter metadata reader and resolver**

In `crates/xteink-fs/src/cache.rs` and `crates/xteink-fs/src/reader.rs`:
- add a small parser for `chapters.idx` records (`CHP1`, `u64 offset`, `u16 len`, bytes)
- expose a tiny helper that returns the active `chapter_number` and `chapter_title` for a given `current_page_start_offset`
- keep parsing local to `xteink-fs`

- [ ] **Step 4: Run the targeted test to verify it passes**

Run: `cargo test -p xteink-fs chapter -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run validity check**

Run: `cargo check`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/xteink-fs/src/cache.rs crates/xteink-fs/src/reader.rs crates/xteink-fs/tests/cache_paths.rs
git commit -m "feat: resolve active chapter footer context"
```

### Task 3: Wire Footer Context Through Reader Rendering

**Files:**
- Modify: `crates/xteink-fs/src/reader.rs`
- Modify: `crates/xteink-render/src/lib.rs`

- [ ] **Step 1: Write the failing integration-oriented reader test**

Add or extend a host-side test to prove that when reader state has:
- valid `progress.bin`
- valid `chapters.idx`

the render path requests footer output with:
- left chapter number
- middle chapter title prefix
- right progress percent

Use the smallest available host-testable seam rather than adding new persistence fields.

- [ ] **Step 2: Run the targeted test to verify it fails**

Run: `cargo test -p simulator --test storage footer -- --nocapture`
Expected: FAIL because the reader still renders progress-only footer output

- [ ] **Step 3: Implement minimal wiring**

In `crates/xteink-fs/src/reader.rs`:
- load chapter metadata after progress is known
- resolve the active chapter from `page_start_offset`
- build footer strings
- call the new three-segment footer render helper

Do not store chapter number or title in `progress.bin`.

- [ ] **Step 4: Run targeted tests to verify pass**

Run:
- `cargo test -p xteink-fs -- --nocapture`
- `cargo test -p xteink-render --test epub_render -- --nocapture`

Expected: PASS

- [ ] **Step 5: Run validity check**

Run: `cargo check`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/xteink-fs/src/reader.rs crates/xteink-render/src/lib.rs
git commit -m "feat: render chapter context in reader footer"
```

### Task 4: Prove Real Fixture Footer Behavior End To End

**Files:**
- Modify: `simulator/tests/storage.rs`
- Reference: `simulator/sdcard/`

- [ ] **Step 1: Write the failing real-fixture footer assertion**

Using the existing real EPUB fixture flow, add an assertion that a rendered page after a known chapter boundary includes:
- the expected 1-indexed chapter number
- the expected prefix of the chapter title
- the expected progress percent

Prefer checking the same host-visible output mechanism already used for framebuffer assertions.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p simulator --test storage footer -- --nocapture`
Expected: FAIL before the end-to-end footer wiring is complete

- [ ] **Step 3: Adjust implementation only if needed**

If the test reveals edge cases:
- fix chapter selection around exact chapter boundary offsets
- fix footer cropping so left/right segments always survive
- keep title cropping hard and deterministic

- [ ] **Step 4: Run the full relevant verification set**

Run:
- `cargo test -p xteink-fs -- --nocapture`
- `cargo test -p xteink-render --test epub_render -- --nocapture`
- `cargo test -p simulator --test storage -- --nocapture`
- `cargo check`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add simulator/tests/storage.rs crates/xteink-fs/src/reader.rs crates/xteink-render/src/lib.rs
git commit -m "test: cover footer chapter context on real epub cache"
```

### Task 5: Update Docs And Final Verification

**Files:**
- Modify: `README.md`
- Modify: `docs/superpowers/specs/2026-04-10-reader-footer-chapter-context-design.md`

- [ ] **Step 1: Update docs**

Document:
- footer now shows current chapter number on the left
- footer shows the cropped start of the current chapter title in the middle
- progress remains on the right
- title cropping preserves the number and percent first

- [ ] **Step 2: Run final formatting and diff checks**

Run:
- `cargo fmt --all`
- `git diff --check`

Expected: PASS

- [ ] **Step 3: Run final relevant verification**

Run:
- `cargo check`
- `cargo test -p xteink-fs -- --nocapture`
- `cargo test -p xteink-render --test epub_render -- --nocapture`
- `cargo test -p simulator --test storage -- --nocapture`

Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add README.md docs/superpowers/specs/2026-04-10-reader-footer-chapter-context-design.md
git commit -m "docs: describe reader footer chapter context"
```
