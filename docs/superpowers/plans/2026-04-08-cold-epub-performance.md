# Cold EPUB Performance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce cold EPUB parse/build cost and cold-open time-to-first-page by first removing parser complexity, then decoupling first-page render from full cache construction, without increasing device memory usage.

**Architecture:** Stage 1 keeps behavior stable while removing hidden parser complexity and adding measurement guardrails. Stage 2 changes the cache-miss flow so the system stops once the requested page is ready and resumes cache construction later from chapter-level state instead of rebuilding from the start.

**Tech Stack:** Rust 2024, `no_std` fixed-workspace parser in `xteink-epub`, shared framebuffer renderer in `xteink-render`, SD/cache integration in `xteink-fs`, host regression tests via `cargo test`

---

### Task 1: Add measurable cold-parse baselines

**Files:**
- Modify: `crates/xteink-render/tests/epub_render.rs`
- Modify: `crates/xteink-epub/tests/task3.rs`
- Test: `crates/xteink-render/tests/epub_render.rs`
- Test: `crates/xteink-epub/tests/task3.rs`

- [ ] **Step 1: Write the failing test**

Add a parser-focused regression in `crates/xteink-epub/tests/task3.rs` that runs the large fixture through the runtime-sized workspace and records elapsed time with `std::time::Instant`.

- [ ] **Step 2: Run test to verify it fails or is missing**

Run: `cargo test -p xteink-epub --target aarch64-apple-darwin task3 -- --nocapture`
Expected: FAIL or lack the new timing regression.

- [ ] **Step 3: Write minimal implementation**

Add timing-style test helpers that print:
- OPF/catalog preparation duration
- full cold parse duration for the large fixture
- first-page-only duration for the same fixture

Keep assertions functional, not time-threshold based.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p xteink-epub --target aarch64-apple-darwin task3 -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run validity check**

Run: `cargo check -p xteink-epub --target aarch64-apple-darwin`
Expected: PASS

### Task 2: Remove `O(spine * manifest)` OPF lookup behavior

**Files:**
- Modify: `crates/xteink-epub/src/lib.rs`
- Test: `crates/xteink-epub/tests/task3.rs`

- [ ] **Step 1: Write the failing test**

Add a targeted parser test asserting that a large OPF with many manifest items and many `itemref`s resolves the spine in a single manifest pass without changing output order.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p xteink-epub --target aarch64-apple-darwin task3 -- --nocapture`
Expected: FAIL because the current implementation still rescans manifest entries.

- [ ] **Step 3: Write minimal implementation**

In `parse_opf()`:
- build a compact fixed-buffer manifest index in one pass
- resolve `idref -> href/media-type` from that index
- preserve current fixed-memory bounds and error behavior

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p xteink-epub --target aarch64-apple-darwin task3 -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run validity checks**

Run:
- `cargo test -p xteink-epub runtime_workspace_handles_happiness_trap_pocketbook_fixture --target aarch64-apple-darwin`
- `cargo check -p xteink-reader --features embedded --target riscv32imc-unknown-none-elf -Zbuild-std=core`
Expected: PASS

### Task 3: Remove hot-loop parser overhead in `next_event`

**Files:**
- Modify: `crates/xteink-epub/src/lib.rs`
- Test: `crates/xteink-epub/tests/task3.rs`

- [ ] **Step 1: Write the failing test**

Add a focused regression that exercises a multi-chapter parse and asserts no semantic change while recording per-run timing.

- [ ] **Step 2: Run test to verify it fails or exposes missing coverage**

Run: `cargo test -p xteink-epub --target aarch64-apple-darwin task3 -- --nocapture`
Expected: missing coverage or failing timing-harness setup.

- [ ] **Step 3: Write minimal implementation**

Optimize `next_event()` and chapter loading by:
- removing per-event `chapter_dir` copying
- moving chapter-local derived state into `load_current_chapter()`
- reducing repeated hot-loop bookkeeping where it does not affect semantics

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p xteink-epub --target aarch64-apple-darwin task3 -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run validity checks**

Run:
- `cargo test -p xteink-render --test epub_render --target aarch64-apple-darwin`
- `cargo check -p xteink-reader --features embedded --target riscv32imc-unknown-none-elf -Zbuild-std=core`
Expected: PASS

### Task 4: Re-measure Stage 1 and lock in parser regressions

**Files:**
- Modify: `crates/xteink-render/tests/epub_render.rs`
- Modify: `crates/xteink-epub/tests/task3.rs`

- [ ] **Step 1: Write the failing test**

Add explicit regression coverage that the large cold-parse fixture still:
- succeeds
- emits cacheable text
- does not hit `OutOfSpace`

- [ ] **Step 2: Run test to verify it fails if protection is missing**

Run: `cargo test -p xteink-render --test epub_render --target aarch64-apple-darwin`
Expected: FAIL only if the new guardrail is not yet present.

- [ ] **Step 3: Write minimal implementation**

Keep the tests small and stable:
- broad all-fixture `OutOfSpace` sweep
- one large-fixture cold-parse stress regression
- parser-level runtime workspace regression

- [ ] **Step 4: Run test to verify it passes**

Run:
- `cargo test -p xteink-render --test epub_render --target aarch64-apple-darwin`
- `cargo test -p xteink-epub runtime_workspace_handles_happiness_trap_pocketbook_fixture --target aarch64-apple-darwin`
Expected: PASS

- [ ] **Step 5: Record baseline**

Run: `/usr/bin/time -lp cargo test -p xteink-render --test epub_render large_fixture_cold_parse_to_cache_without_out_of_space --target aarch64-apple-darwin`
Expected: PASS with a recorded new baseline for comparison.

### Task 5: Add resumable cache metadata for partial builds

**Files:**
- Modify: `crates/xteink-fs/src/cache.rs`
- Modify: `crates/xteink-fs/src/reader.rs`
- Add or Modify: `crates/xteink-fs/tests/cache_paths.rs`

- [ ] **Step 1: Write the failing test**

Add a host-side cache metadata regression that can round-trip:
- cache version
- source size
- content length built so far
- next resume checkpoint (chapter index or equivalent)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p xteink-fs --test cache_paths --target aarch64-apple-darwin`
Expected: FAIL because resume metadata does not exist yet.

- [ ] **Step 3: Write minimal implementation**

Extend cache metadata in a backward-compatible way:
- version bump if needed
- chapter-granular resume checkpoint only
- no RAM growth in the runtime hot path

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p xteink-fs --test cache_paths --target aarch64-apple-darwin`
Expected: PASS

- [ ] **Step 5: Run validity checks**

Run:
- `cargo check -p xteink-fs --target aarch64-apple-darwin`
- `cargo check -p xteink-reader --features embedded --target riscv32imc-unknown-none-elf -Zbuild-std=core`
Expected: PASS

### Task 6: Stop cold open once the target page is renderable

**Files:**
- Modify: `crates/xteink-fs/src/reader.rs`
- Modify: `crates/xteink-render/src/epub.rs`
- Test: `crates/xteink-render/tests/epub_render.rs`

- [ ] **Step 1: Write the failing test**

Add a regression proving that on a cache miss:
- page 0 render returns before full-book cache completion
- emitted cached text is non-empty
- result is still a valid page render

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p xteink-render --test epub_render --target aarch64-apple-darwin`
Expected: FAIL because cold open still behaves like a full cache build.

- [ ] **Step 3: Write minimal implementation**

Change cold-open behavior:
- parse only until requested page is available
- persist partial cache metadata
- return the rendered page immediately

Do not add background threads or new allocator requirements.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p xteink-render --test epub_render --target aarch64-apple-darwin`
Expected: PASS

- [ ] **Step 5: Run validity checks**

Run:
- `cargo test -p xteink-fs --test cache_paths --target aarch64-apple-darwin`
- `cargo check -p xteink-reader --features embedded --target riscv32imc-unknown-none-elf -Zbuild-std=core`
Expected: PASS

### Task 7: Resume cache building from chapter checkpoints instead of restarting

**Files:**
- Modify: `crates/xteink-fs/src/reader.rs`
- Modify: `crates/xteink-fs/src/cache.rs`
- Possibly Modify: `crates/xteink-epub/src/lib.rs`
- Test: `crates/xteink-render/tests/epub_render.rs`

- [ ] **Step 1: Write the failing test**

Add a regression showing:
- first open creates partial cache state
- a later page request extends the cache
- parsing does not restart from the beginning of the book

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p xteink-render --test epub_render --target aarch64-apple-darwin`
Expected: FAIL because resumed cache build does not exist yet.

- [ ] **Step 3: Write minimal implementation**

Resume from chapter-granular checkpoints:
- reopen source EPUB
- skip directly to the next chapter boundary
- append to `content.txt`
- update progress metadata

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p xteink-render --test epub_render --target aarch64-apple-darwin`
Expected: PASS

- [ ] **Step 5: Run validity checks**

Run:
- `cargo test -p xteink-epub --target aarch64-apple-darwin`
- `cargo test -p xteink-render --test epub_render --target aarch64-apple-darwin`
- `cargo test -p xteink-fs --test cache_paths --target aarch64-apple-darwin`
- `cargo check -p xteink-reader --features embedded --target riscv32imc-unknown-none-elf -Zbuild-std=core`
Expected: PASS

### Task 8: Document and compare the before/after baselines

**Files:**
- Modify: `docs/superpowers/specs/2026-04-08-cold-epub-performance-design.md`
- Modify: `README.md` if the behavior change needs a note

- [ ] **Step 1: Record Stage 1 and Stage 2 measurements**

Run the host measurement commands and capture:
- full cold parse baseline before and after
- first-page-only baseline before and after

- [ ] **Step 2: Record device-facing measurement procedure**

Document the serial-log-based device timing method for cold cache miss.

- [ ] **Step 3: Update docs with final outcome**

Summarize:
- what changed
- what speedup was achieved
- any remaining bottlenecks

- [ ] **Step 4: Run final validity checks**

Run:
- `cargo test -p xteink-epub --target aarch64-apple-darwin`
- `cargo test -p xteink-render --test epub_render --target aarch64-apple-darwin`
- `cargo test -p xteink-fs --test cache_paths --target aarch64-apple-darwin`
- `cargo check -p xteink-reader --features embedded --target riscv32imc-unknown-none-elf -Zbuild-std=core`

Expected: PASS
