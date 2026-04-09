# Byte-Offset EPUB Reader Cache Design

## Goal

Make EPUB reading use one simple cached-text model across simulator and firmware:

- `content.txt` is the single source of truth for reading
- `chapters.idx` stores chapter start byte offsets into `content.txt`
- `pages.idx` stores page start byte offsets into `content.txt`
- `progress.bin` stores the current reading byte offset
- progress percentage is derived from `current_byte_offset / content_length`

This must preserve the current RAM limits, keep the shared renderer path, and make resume/progress/page-turn behavior reliable.

## Why the Current Design Is Wrong

The current cache path still mixes several concepts:

- parsed EPUB source state
- partial cache-prefix state
- page-count heuristics
- chapter-based progress estimates
- reopen behavior derived from page/chapter metadata instead of byte position

That complexity is the root cause of the repeated progress and resume bugs. The reader should not need to reason about EPUB chapter state after text has been cached.

## Chosen Approach

Use a single cached-text reading model with three sidecar files:

- `content.txt`
- `chapters.idx`
- `pages.idx`

and one progress file:

- `progress.bin`

Cold open should still show page `0` as early as possible. The rest of the book can continue caching afterward, but all reading and pagination must come from `content.txt`, not from re-reading EPUB chapter structure.

## On-Disk Cache Format

Each book cache directory under `/.cool/<book-id>/` contains:

- `content.txt`
  - normalized linear UTF-8 text stream for the whole book
  - this is the only source used for page rendering after bytes exist in cache
- `chapters.idx`
  - fixed-size records of chapter start byte offsets in `content.txt`
  - used for rebuild/recovery and optional future tooling
- `pages.idx`
  - fixed-size records of page start byte offsets in `content.txt`
  - one record per page boundary for the current layout/version
- `progress.bin`
  - current reading byte offset in `content.txt`
  - may also include current page number as a non-authoritative hint
- `meta.txt`
  - cache version
  - source size
  - `content_length`
  - build completion flag
  - current cache-build checkpoint
  - layout signature fields needed to invalidate stale `pages.idx`

`<book-id>` stays compatible with the existing scheme:

- derive it from the full logical source path plus entry name
- sanitize to uppercase ASCII alnum/underscore
- if longer than the component limit, shorten to the existing hashed `Bxxxxxxx` form

This preserves simulator/firmware cache-path compatibility and keeps migration bounded to cache-version changes, not naming changes.

## Text Normalization Rules

`content.txt` stores the same normalized text stream currently emitted by the shared EPUB parser/paginator boundary.

Normalization rules:

- UTF-8 encoding only
- paragraph text is emitted as plain text bytes
- line breaks are encoded with a single newline byte `\\n`
- paragraph breaks are encoded with two newline bytes `\\n\\n`
- explicit page-break markers are never stored in `content.txt`
- unsupported tags emit no bytes
- image alt text is emitted exactly where the shared parser currently emits it
- no additional whitespace collapsing happens after bytes are written to `content.txt`

Byte offsets in `chapters.idx`, `pages.idx`, and `progress.bin` always refer to byte positions in this normalized UTF-8 stream.

## File Formats

### `content.txt`

- encoding: UTF-8
- append-only while cache is incomplete
- final size in bytes is recorded as `content_length` in `meta.txt`

### `chapters.idx`

- binary file
- little-endian
- record size: 8 bytes
- each record:
  - `u64 chapter_start_byte_offset`
- append one record when a new chapter begins in `content.txt`
- atomicity rule: append the chapter offset before appending the chapter’s normalized bytes

### `pages.idx`

- binary file
- little-endian
- record size: 8 bytes
- each record:
  - `u64 page_start_byte_offset`
- record `0` is always byte offset `0`
- append one record when a page boundary is discovered by paginating `content.txt`
- atomicity rule: only append a page record after the preceding page bytes already exist in `content.txt`

### `progress.bin`

- binary file
- little-endian
- fixed size: 16 bytes
- fields in order:
  - `u64 current_byte_offset`
  - `u32 current_page_hint`
  - `u32 reserved`
- `current_page_hint` is advisory only
- `current_byte_offset` is authoritative
- write rule: replace atomically after a successful page render

### `meta.txt`

- UTF-8 text
- newline-delimited `key=value`
- authoritative fields:
  - `version`
  - `source_size`
  - `content_length`
  - `build_complete`
  - `next_chapter_index`
  - `layout_sig_version`
  - `layout_sig_width`
  - `layout_sig_height`
  - `layout_sig_content_height`
  - `layout_sig_font`
  - `layout_sig_paginator`
- write rule:
  - overwrite only after `content.txt` and any newly appended index records are flushed

## Reading Model

### Authoritative Rules

- Page rendering always reads from `content.txt`
- Page boundaries always come from `pages.idx` when available
- Resume always starts from saved byte offset in `progress.bin`
- Progress percentage always uses `saved_byte_offset / content_length`

There is no page/chapter heuristic progress path in the final model.

### Reopen

On reopen:

1. read `progress.bin`
2. map saved byte offset to the greatest page start byte offset less than or equal to the saved offset
3. paginate from that byte offset in `content.txt`
4. save updated byte offset after rendering/page turn

If `pages.idx` is incomplete but `content.txt` contains enough bytes to satisfy the requested page, extend `pages.idx` by paginating forward in `content.txt`, not by re-reading EPUB structure.

## Cold Cache-Build Flow

On a cache miss:

1. open EPUB source
2. parse chapters in order
3. normalize text and append it to `content.txt`
4. append the corresponding chapter start offsets to `chapters.idx`
5. as soon as enough bytes exist to render page `0`, render page `0`
6. continue building the remaining cache incrementally afterward
7. append discovered page starts to `pages.idx` as pagination advances

The critical separation is:

- EPUB parsing is only a producer into `content.txt`
- page rendering is only a consumer of `content.txt`

## Incremental Build Model

Because page `0` must appear early, cache construction cannot block on the entire book.

The cache metadata therefore needs one simple incremental-build checkpoint:

- next chapter index still to append into `content.txt`

This is the only EPUB-specific rebuild state that remains after the refactor. It is used only while the cache is incomplete.

Once `content.txt` is complete:

- EPUB parsing state is no longer relevant to reading
- all reading state is byte-offset based

## Page Rendering

Page rendering should work in two modes:

### 1. Cached-page render

Used whenever `pages.idx` already has the target page start.

Flow:

1. seek to page start byte offset in `content.txt`
2. render until the next page boundary
3. if rendering reveals the next page start and it is not yet in `pages.idx`, append it

### 2. Cached-text pagination extension

Used when `content.txt` exists but `pages.idx` does not yet cover the requested page.

Flow:

1. seek to the nearest known page start in `pages.idx`
2. paginate forward using `content.txt`
3. append newly discovered page offsets to `pages.idx`
4. stop once the target page is rendered

This still reads only from `content.txt`.

## Progress Model

Progress uses bytes, not chapters or page counts.

Definitions:

- `content_length`: total bytes in completed `content.txt`
- `current_byte_offset`: start byte of the currently rendered page

Progress formula:

- `percent = current_byte_offset / content_length`

Behavior:

- monotonic once `content.txt` is complete
- stable across reopen
- stable across simulator and firmware
- independent of chapter sizes

If the book is not fully cached yet:

- the exact total denominator is not yet known, so the reader footer must not show the final book percentage
- instead, UI shows a distinct “building cache” state with no numeric percent
- once `content.txt` is complete, the footer switches to exact byte-based percentage and remains exact thereafter

## Shared Boundaries

### `xteink-fs`

Owns:

- cache file naming and metadata
- cache probe / cache miss / cache incomplete decisions
- EPUB-to-cache build orchestration
- progress persistence

### `xteink-render`

Owns:

- pagination over `content.txt`
- page rendering from cached text
- page-boundary discovery for `pages.idx`

It should not need EPUB chapter heuristics for reading progress.

### simulator and firmware

Both must use the same `xteink-fs` reader/cache orchestration and the same `xteink-render` paginator.

Only low-level file/directory APIs may differ.

## Migration / Cache Invalidation

This design changes the meaning of cache metadata and page/progress state.

Required:

- bump cache version
- invalidate older cache directories automatically
- rebuild `content.txt`, `chapters.idx`, `pages.idx`, and `progress.bin` under the new model

`pages.idx` must be invalidated whenever any layout-signature field changes. The layout signature is:

- cache version
- display width
- display height
- reader content height
- font metrics/version used for pagination
- paginator algorithm version

If any of these differ from `meta.txt`, `pages.idx` is stale and must be rebuilt from `content.txt`.

## Error Handling

If any cache sidecar is missing or inconsistent:

- treat the cache as incomplete or stale
- do not trust `progress.bin`
- rebuild the missing index or the entire cache as needed

Specific rules:

- missing `content.txt` means cache miss
- missing `pages.idx` means cached text exists but pagination index must be rebuilt from `content.txt`
- missing `chapters.idx` during an incomplete build means EPUB rebuild must restart from chapter `0`
- stale `meta.txt` invalidates all sidecars

## Memory Constraints

The refactor must not increase device memory usage.

Allowed strategies:

- reuse existing fixed parser workspace while cache is incomplete
- reuse the existing paginator/text buffer for reading from `content.txt`
- write indexes incrementally to disk

Not allowed:

- loading the full cached text into RAM
- adding large new persistent buffers
- adding allocator-dependent runtime logic on device

## Testing Strategy

### Renderer / Pagination

- cached page render from `content.txt` matches direct early-page behavior
- page boundaries remain continuous with no missing lines
- page discovery appends stable `pages.idx` offsets

### Reader / Cache

- cold miss creates `content.txt`, `chapters.idx`, `pages.idx`, `meta.txt`, `progress.bin`
- reopen resumes from saved byte offset
- progress is monotonic for early pages of `Decisive`
- progress does not spike or drop when cache extends
- stale cache version is ignored

### Shared behavior

- simulator and firmware both use the same cache-backed reader path
- embedded build still passes within the current RAM budget

## Recommendation

Implement this by removing chapter/page heuristic progress and replacing it with a strict two-phase model:

1. EPUB parser builds cached text and chapter offsets
2. reader renders and resumes from cached text and page offsets only

That gives the simplest reliable behavior and aligns the implementation with the user-visible mental model.
