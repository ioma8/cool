# EPUB parser crate design (no_std, low-memory, firmware parser)

## 1. Goal and constraints
- Implement a firmware-ready EPUB parser in a new workspace crate `xteink-epub`.
- The parser runs in `no_std` firmware and supports future SD-card input via a storage-agnostic source abstraction.
- Initial behavior is text rendering support with lightweight markup preservation, not image rasterization.
- Priority: low memory usage and zero-copy where feasible; no unnecessary heap allocation in parser core.
- EPUB validation and regression will use files in `test/epubs`.

## 2. Scope for this phase
- Parse EPUB container (ZIP) with deflate-compressed and stored entries.
- Resolve `META-INF/container.xml` to find `content.opf`.
- Parse OPF manifest and spine in order.
- Parse XHTML chapter content for a markup-preserving event stream (`<p>`, headings, `<br>`, `<img>`, text).
- Expose minimal APIs for incremental consumption suitable for firmware rendering logic.
- Do not implement SD card transport in this crate; SD access remains a separate future crate.

## 3. Crate layout and interfaces
- New crate path: `crates/xteink-epub/`
- Public API:
  - `pub trait EpubSource { fn len(&self) -> usize; fn read_at(&self, offset: u64, dst: &mut [u8]) -> Result<usize, EpubError>; }`
  - `pub struct Epub<'a, S: EpubSource> { source: S, manifest: EpubManifest, spine: SpinedOrder, ... }`
  - `pub struct EpubReader<'a, S: EpubSource> { source: &'a S, cursor: ReaderCursor, state: ParseState, ... }` (or equivalent)
  - `pub fn EpubReader::open(source: S) -> Result<Self, EpubError>`
  - `pub fn next_block(&mut self, out: &mut EpubEventSink) -> Result<Option<EpubEvent<'a>>, EpubError>`
- Output model:
  - `EpubEvent<'a>` variants:
    - `Text(&'a str)`
    - `ParagraphStart`, `ParagraphEnd`
    - `HeadingStart(u8)`, `HeadingEnd`
    - `LineBreak`
    - `Image { src: &'a str, alt: Option<&'a str> }`
    - `TableStart`, `TableEnd`, `UnsupportedTag`
- Source and decompression buffers are caller-supplied to keep parser heap-agnostic.

## 4. Parsing architecture
- **ZIP layer**
  - Read EOCD from end of file.
  - Parse central directory records and list entry metadata (name, compression method, offsets, sizes).
  - Locate needed entries by name lookup.
- **OPF layer**
  - Inflate `container.xml` and parse `rootfile` entry.
  - Inflate and parse `content.opf`, extract package base path, manifest item ids/hrefs/media-types, and spine order.
- **XHTML layer**
  - For each spine item in order, inflate XHTML stream and parse tags.
  - Emit events while flattening nested structure into linear markup events.

## 5. Memory model
- Default parser operation uses no heap allocation.
- Internal parsing uses caller-provided scratch buffers for deflate output and parser workspaces.
- XML parsing and EPUB traversal operate on borrowed slices when valid (`&str` tied to scratch buffer lifetime).
- Explicit `EpubError::OutOfSpace` if a caller-provided buffer is too small.

## 6. Dependencies and no_std plan
- Use `quick-xml` for XML parsing.
- Use `zlib-rs` for Deflate decompression of ZIP entries.
- Keep dependencies `default-features = false` with `no_std` compatible features.
- Use `core` only in library by default; tests may enable `alloc`/`std`.

## 7. Error model
- `EpubError` enum: `Zip`, `Xml`, `Utf8`, `Compression`, `Io`, `OutOfSpace`, `InvalidFormat`, `Unsupported`.
- Errors always carry recoverable context where possible.

## 8. Test-driven workflow
- Add tests for each behavior before implementation:
  1. Container discovery and entry lookup against `test/epubs` samples.
  2. OPF parsing yields expected manifest/spine order.
  3. XHTML events preserve paragraph/heading/line-break/image markers.
  4. Non-text media-only EPUB path handles gracefully.
  5. Compressed/uncompressed entries both parse.
- Use `#[test]` fixtures by including sample epubs through crate tests.

## 9. Acceptance criteria
- `cargo check` succeeds for workspace with crate included.
- New crate API can list and iterate spine items as stream events with low allocations.
- Parser can extract readable ordered content from at least one EPUB in `test/epubs` and emit the expected basic markup-preserving events.
