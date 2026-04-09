# Cold EPUB Performance Design

## Goal

Improve cold EPUB open performance on both host and device by at least one order of magnitude in user-visible latency, without increasing runtime memory usage on the ESP32-C3 and without regressing rendering or cache correctness.

The target applies to two related but distinct cases:

- host: cold-cache full parse/build path used by regression tests and profiling
- device: cold-open time to first rendered page on a cache miss

## Current baseline and observed constraints

### Measured host baseline

The current host cold-parse stress test for `Happiness Trap Pocketbook, The - Russ Harris.epub` runs in roughly `0.26s` wall clock on the development machine. That number is useful as a regression baseline, but it hides the real embedded cost because:

- host CPU is much faster than the ESP32-C3
- host file IO is much faster than SD card IO
- the current device cold-open path does extra SD writes while building the cache

### Observed cold-open behavior on device

Cold open still does too much work before showing page 0:

- open source EPUB
- parse ZIP metadata and OPF
- walk spine entries
- inflate XHTML chapters
- emit text
- write `content.txt`
- write metadata/progress

That means the current cold-open path is effectively "render page 0 plus build a large part or all of the cache", which is the main reason the device feels slow on first open.

### Memory constraints

The design must not increase runtime memory usage on device. This is a hard constraint. Current fixed workspaces already consume a meaningful fraction of the 380 KB overall RAM budget. Any speedup must come from:

- reduced repeated work
- reduced algorithmic complexity
- improved IO patterns
- better separation between first-page rendering and full cache construction

not from larger buffers or new persistent caches in RAM.

## Main hidden complexity today

The current parser and cold-cache pipeline still contain avoidable complexity:

1. `parse_opf()` in `xteink-epub` still resolves each spine item through a manifest lookup pattern that is effectively `O(spine * manifest)` on larger books.
2. Cold open and full cache build are coupled. The system does not stop when page 0 is ready.
3. The cache format stores full text but does not store enough structure to cheaply resume later work from a known parser position.
4. The current cache build path writes through a buffered sink, but it still behaves like a single long blocking operation from the perspective of first-page latency.

## Approaches considered

### Approach 1: parser and cold-cache optimization first, then split first-page rendering from full cache build

This keeps behavior stable first, then improves UX once the parser cost is lower.

Pros:

- lowest-risk path to measurable gains
- gives concrete profiling data before changing behavior
- preserves current correctness while removing obvious complexity

Cons:

- likely not enough alone to achieve 10x device cold-open latency
- requires two phases

### Approach 2: parser-only optimization, no behavior change

This focuses entirely on reducing full cold-parse cost.

Pros:

- smallest behavior change
- easiest to validate incrementally

Cons:

- unlikely to deliver 10x time-to-first-page on device
- still blocks on cache construction before page 0

### Approach 3: immediate behavior split with background or resumable cache build

This attacks the user-visible latency first.

Pros:

- biggest direct UX win

Cons:

- more moving parts
- harder to reason about without parser baseline and profiling first

## Chosen design

Use Approach 1 in two stages:

1. reduce parser and cache-build cost without changing cold-open behavior
2. separate first-page rendering from full cache construction so the device can show page 0 quickly and extend the cache later

This is the only path that balances risk, measurement, and the 10x requirement.

## Stage 1: parser and cache-build efficiency

### Objectives

- remove avoidable repeated scans in OPF/spine processing
- reduce per-event parser overhead
- improve cold-cache build efficiency without changing external behavior
- add measurements so future changes are judged against real baselines

### Planned changes

#### 1. Single-pass manifest indexing

`parse_opf()` should stop rescanning the OPF manifest for every `itemref`.

Instead:

- perform one pass over the manifest
- collect only the XHTML items needed for the spine
- resolve `id -> href/media-type` through a compact fixed-buffer index
- keep all storage fixed-size and bounded to preserve `no_alloc` and memory limits

This directly targets the current `O(spine * manifest)` behavior.

#### 2. Reduce per-event parser overhead

`next_event()` should avoid repeated work that is invariant for long stretches of parsing.

Examples:

- remove the per-event copy of `chapter_dir`
- move any repeated chapter-local state derivation to `load_current_chapter()`
- reduce re-slicing and redundant bookkeeping inside the hot event loop

#### 3. Improve cache-build write pattern only where it reduces real work

The cache writer is already buffered, so Stage 1 should avoid speculative rewrites here. The goal is to reduce upstream work first, not to churn the IO adapter.

### Stage 1 success criteria

- all existing tests remain green
- host cold-parse stress test improves measurably
- no increase in embedded memory budget
- no rendering or cache correctness regressions

## Stage 2: split first-page latency from full cache build

### Objectives

- make page 0 appear quickly on a cache miss
- avoid blocking the user on full-book cache creation
- preserve eventual full-text cache behavior for fast later page turns

### Planned changes

#### 1. Partial cache state

Extend the cache metadata to track partial build state, for example:

- bytes of `content.txt` already built
- current spine index
- enough parser/progress information to resume from a known boundary

The resume point should be chapter-granular, not token-granular, to keep complexity bounded.

#### 2. Cold open renders only until requested page is ready

On a cache miss:

- parse until page `target_page` is renderable
- flush emitted text to cache incrementally
- stop the blocking cold-open path once page 0 is ready
- persist resume metadata

#### 3. Later actions extend cache instead of restarting from spine 0

When the user asks for later pages:

- if cached text is sufficient, use the existing fast cached path
- if not, resume cache build from the last known chapter boundary
- append more text and update progress/index metadata

### Stage 2 success criteria

- device time-to-first-page improves dramatically on cache miss
- later page turns still use the fast cache path
- repeated cold opens do not rebuild from scratch
- memory use stays within the current device guardrails

## Test and measurement strategy

### Host regression coverage

Keep and extend host tests so they catch the issues before flashing:

- all EPUB fixtures must not fail with `OutOfSpace` on first-page render
- large cold-parse fixture must succeed on the full-cache path
- parser-level runtime-workspace test must cover the known large fixture

### New profiling-focused tests

Add measurement helpers or ignored benchmark-style tests that report:

- OPF parse time for large fixtures
- full cold parse/build time for the large fixture
- first-page-only render time for the same fixture

The goal is not microbenchmark purity; it is stable before/after comparisons inside this repo.

### Device validation

Device validation should use the existing firmware path and log:

- cold-open time to first page
- whether cache miss or cache hit path was used
- whether later page turns resume cache work or reuse cache directly

## File impact

Primary files:

- `crates/xteink-epub/src/lib.rs`
- `crates/xteink-render/src/epub.rs`
- `crates/xteink-fs/src/reader.rs`
- `crates/xteink-fs/src/cache.rs`
- `crates/xteink-render/tests/epub_render.rs`
- `crates/xteink-epub/tests/task3.rs`

Likely no structural changes are needed outside these files for Stage 1. Stage 2 will expand the cache metadata and reader flow but should still stay within the same modules.

## Risks

### Parser correctness regressions

Manifest/spine optimization changes parser internals in a hot path. This is the main Stage 1 risk. Countermeasure: keep tests green after each small change and add targeted parser tests before large edits.

### Cache resume correctness

Stage 2 introduces statefulness into the cache-building process. A bad resume model could duplicate or skip content. Countermeasure: chapter-boundary resume only, explicit metadata versioning, and regression tests that compare resumed output against full cold parse.

### Missing the 10x target

A strict parser-only optimization pass is unlikely to deliver 10x device cold-open latency by itself. The design assumes Stage 2 is required for the user-visible target.

## Recommended execution order

1. Add profiling and parser-focused regression tests.
2. Implement Stage 1 parser complexity reductions.
3. Re-measure host baseline.
4. If parser gains are real and tests stay green, implement Stage 2 resumable cold-open behavior.
5. Re-measure host and device-facing latency.
