# EPUB cache + fast parse design (v2)

## Scope
- Accelerate EPUB open/render path on-device by introducing SD-card cache artifacts.
- Keep parsing core memory-efficient and no-alloc in normal operation.
- Cache artifacts stored under `/.cool` on SD card, path-derived per EPUB.

## Constraints observed from current code
- `xteink-fs` owns SD access and currently calls `xteink-display::SSD1677Display::render_epub_page` through `xteink-epub` parser.
- Parser currently reparses the ZIP central directory repeatedly while iterating chapters.
- Render loop rebuilds layout from source on every page open.

## Target cache layout (path-derived)
For EPUB at `/MYBOOKS/PET.JA~1.EPU` with cache directory key `<safe-name>`:
- `/.cool/<safe-name>/meta.json` — metadata and cache validity fields.
- `/.cool/<safe-name>/content.txt` — plain UTF-8 normalized text extracted from spine order.
- `/.cool/<safe-name>/progress.bin` — last read page (u32 LE), plus optional future fields.

### Safe-name
- Derived from EPUB relative path inside SD: replace any non-`[A-Za-z0-9._-]` char with `_`, collapse repeats.
- Use full normalized path-derived name (no hashing).
- Keep short (e.g. <= 64 bytes) by truncating a tail-safe suffix if needed.

## Validity rules
A cached entry is valid when:
- `source_size` in metadata equals `source.length()`.
- `version` matches binary layout (`1`).
- Metadata and content files are readable and parsable.
- If invalid, full source parse is done, cache rebuilt, and progress reset to `0`.

## Rendering behavior with cache
- `render_epub_from_entry` and `render_epub_page_from_entry` first try cache path.
- On cache hit:
  - Use `content.txt` text stream renderer (lightweight layout from cached text). 
  - No ZIP/container/opf/xhtml parse for page render.
- On cache miss:
  - Parse EPUB normally via existing parser.
  - Write `content.txt` plus `meta.json` (and optional `progress.bin=0`) after successful parse.

## Performance improvements (parser)
- Remove repeated ZIP central-directory reparsing during chapter iteration.
- Keep `archive.parse(...)` once per `Epub::open` + catalog prepare path.
- Keep parser state and buffers as-is; no new heap in parser path.

## Data model and compatibility
- Content is plain text as requested (`content.txt`).
- Image tags write alt text into cache when available.
- Unsupported structural constructs become whitespace separators:
  - Table-like tags use double-newline markers.

## Minimal API changes
- Add caching module in `xteink-fs` that resolves cache directory, validates metadata, and writes cache artifacts.
- Extend display with `render_cached_text` helper that streams text and supports page indexing.
- Add test-only helpers for:
  - cache key derivation
  - metadata write/read
  - cache-hit parser bypass behavior in a mocked/host-like flow

## Failure modes
- Invalid cache files: ignore cache, rebuild from source.
- Cache write/read failures: continue with live parse and emit readable error only if parsing fails.
- Any read/write I/O faults map to existing `EpubError` or `FsError` as close as possible.

## TDD plan (host test first)
- Add tests for cache key derivation and metadata serialization.
- Add tests that verify cache invalidation on size mismatch.
- Keep existing EPUB parse tests intact; add at least one smoke path for cache-hit short-circuit behavior.
