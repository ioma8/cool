# Offset-Window Pagination And Progress Redesign

## Summary

Replace the current mixed page-number/prefix-cache EPUB reader model with a strict byte-offset model.

The new reader state is defined by page offsets into cached linearized text:

- `previous_page_start_offset`
- `current_page_start_offset`
- `next_page_start_offset`

The authoritative cached book data becomes:

- `content.txt`: full linearized cached text for the book
- `meta.txt`: cache validity and `content_length`
- `chapters.idx`: chapter start byte offsets into `content.txt`
- `progress.bin`: previous/current/next page offsets

`pages.idx` is removed entirely.

This document now reflects the implemented cache behavior and on-disk formats.

## Problem

The current implementation mixes incompatible assumptions:

- cache build writes only a prefix of the book
- pagination later treats that prefix as if it were a complete paginated corpus
- resume state mixes byte offsets, page numbers, and partial pagination artifacts
- progress display is derived from unstable intermediate data

This produces the observed failure mode:

- page 0 can show `0%`
- page 1 can jump to a much larger percentage
- page 2 can jump near `99%`
- later pages can render blank because cached text has already reached EOF

This is not a small arithmetic bug. It is a data-model problem.

## Goals

- Make pagination correctness depend only on byte offsets in `content.txt`
- Make progress derive from the current rendered page start offset and total cached content length
- Keep chapter jumps fast through `chapters.idx`
- Make next/previous page turns deterministic and easy to reason about
- Remove speculative page indexing and page-number-based persistence

## Non-Goals

- Fast random jumps to arbitrary far page numbers
- Global full-book page numbering as a source of truth
- Retaining compatibility with `pages.idx`

## Authoritative Model

### Cached Files

#### `content.txt`

Full linearized cached text for the book, including pagination control markers already used by cached replay.

Requirements:

- represents the entire cached book, not only a prefix
- is the sole source for page rendering after cache build
- uses the existing cached replay markers already emitted by `xteink-render`
- all byte offsets used by reader state and chapter metadata refer into this file

#### `meta.txt`

Stores:

- `version`
- `source_size`
- `content_length`
- `build_complete`
- `next_chapter_index`
- layout signature fields already used for cache invalidation

Requirements:

- `content_length` must equal actual `content.txt` length
- layout changes invalidate the cache

#### `chapters.idx`

Stores chapter start byte offsets into `content.txt`.

Format:

- flat binary file
- each record is one little-endian `u64`
- record `n` is the start byte offset of chapter `n` in `content.txt`

Requirements:

- entries are chapter-start offsets in cached text
- offsets are strictly increasing
- first entry is `0`
- no page numbers are stored here

#### `progress.bin`

Format:

- flat binary file
- exactly 24 bytes when present
- three little-endian `u64` values in this order:
  - `previous_page_start_offset`
  - `current_page_start_offset`
  - `next_page_start_offset`

Requirements:

- these offsets define the current reader window
- page number is not persisted as authoritative state
- invalid or missing progress falls back to zeroed offsets

### Reader Truth

The reader position is defined only by `current_page_start_offset`.

Derived values:

- displayed progress
- current rendered framebuffer
- next page boundary
- previous page boundary

Page number, if still shown anywhere internally, is derived and disposable.

## Navigation Semantics

### Open Book

1. Load or build `content.txt`, `meta.txt`, and `chapters.idx`
2. Load `progress.bin`
3. Render from `current_page_start_offset`
4. Recompute `next_page_start_offset` from the rendered page
5. Preserve `previous_page_start_offset` if valid

Default cold-open state:

- `previous = 0`
- `current = 0`
- `next = computed from first rendered page`

### Next Page

1. Set `previous = current`
2. Set `current = next`
3. Render from new `current`
4. Compute new `next`
5. Persist all three offsets

### Previous Page

Preferred path:

1. If `previous_page_start_offset` is valid and `< current`, render from it
2. During that render, recompute its own previous/current/next window
3. Persist recomputed offsets

Fallback path:

1. Find nearest stable checkpoint:
   chapter start from `chapters.idx`, or `0`
2. Scan forward page by page until reaching the page immediately before current
3. Persist recomputed previous/current/next window

### Jump To Chapter

1. Read target chapter start from `chapters.idx`
2. Set `current = chapter_start`
3. Set `previous = 0` or nearest earlier chapter start if available
4. Render from `current`
5. Compute `next`
6. Persist all three offsets

Implementation note:

- the cache now writes real chapter start offsets during the full-book parse
- old placeholder behavior that padded `chapters.idx` with zeroes is removed

## Progress Semantics

Progress is based on the current rendered page start offset relative to cached text length.

Rule:

- if `current_page_start_offset == 0`, show `0%`
- otherwise compute `floor(current_page_start_offset * 100 / content_length)`
- clamp non-terminal pages to `99%`
- show `100%` only when the rendered page is terminal and `next_page_start_offset` reaches EOF

This intentionally makes progress represent the start of the currently displayed page, not a guessed midpoint or page-count-based estimate.

## Implemented Reader Behavior

The reader remains page-number-addressable at some outer app/session call sites, but the persisted truth is now offset-based:

- reopen uses `progress.bin.current_page_start_offset`
- rendered progress is computed from byte offsets, not page numbers
- saved `previous/current/next` offsets are updated after each render
- direct page-number requests are internally resolved by scanning cached page boundaries from `content.txt`

This is intentionally transitional. Storage truth and progress semantics are already offset-based even where some UI/controller APIs still pass page numbers.

## Cache Build Redesign

The build step must produce a complete cache, not a prefix.

Required behavior:

- linearize the full EPUB into `content.txt`
- write complete `chapters.idx`
- write `meta.txt` only after content length is final
- let first render write `progress.bin` from the rendered page window

This removes the current architecture where partial cache generation is mistaken for a complete pagination basis.

## Simplifications

The redesign intentionally removes:

- `pages.idx`
- persisted current page hint as truth
- progress derived from partial-book parse state
- correctness dependence on full-book page counting

## Failure Handling

### Missing Or Invalid Progress

Fallback to:

- `previous = 0`
- `current = 0`
- `next = recomputed from first page render`

### Missing Or Invalid Chapter Index

Rebuild cache artifacts before allowing chapter jumps.

### Layout Signature Change

Invalidate:

- `content.txt`
- `chapters.idx`
- `progress.bin`

Then rebuild from source EPUB.

## Invariants

- every persisted offset must be within `0..=content_length`
- `current <= next`
- `previous <= current`
- if `current == 0`, progress must be `0%`
- `100%` is only valid for the terminal page
- `content_length` must match actual `content.txt` file length
- `chapters.idx` offsets must refer into `content.txt`

## Testing Strategy

Tests should verify the new model directly instead of encoding the broken page-number assumptions.

Required tests:

- render first page from offset `0` yields `current=0`
- first page progress is exactly `0%`
- consecutive next-page renders produce strictly increasing `current` offsets
- consecutive next-page renders do not produce blank pages before EOF
- previous-page render returns exactly to the prior page start offset
- chapter jump starts rendering from the expected chapter offset
- terminal page is the only page allowed to report `100%`
- invalid progress file falls back safely to zero state

## Migration Notes

- remove `pages.idx` support from cache path definitions and reader code
- update progress encoding to store three offsets
- replace page-number resume logic with offset-window persistence
- update simulator and firmware integration to use offset-based render results
- any remaining page number should be treated as UI-local, not persisted truth

## Recommendation

Implement the redesign in small iterations:

1. remove `pages.idx` and change persisted progress to offset-window state
2. switch render flow to offset-centric APIs
3. make cache build write full `content.txt` and `chapters.idx`
4. reintroduce tests around offset-window correctness and progress semantics

This design is simpler than the current system and directly matches the desired product behavior.
