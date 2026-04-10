# Reader Footer Chapter Context

## Summary

Extend the reader footer so it shows:

- the start of the current chapter title on the left
- current progress percent on the right

The chapter identity is derived from the persisted current page start offset and cached chapter metadata already stored in `chapters.idx`.

## Goals

- show the start of the current chapter title in the footer
- preserve the existing progress semantics on the right side
- guarantee all footer segments fit on one line with at least one character of spacing between them
- keep chapter/cache parsing in `xteink-fs`, not in `xteink-render`

## Non-Goals

- changing pagination semantics
- changing progress semantics
- adding extra persisted state for footer display
- adding ellipsis rendering for cropped titles

## Source Of Truth

Footer chapter context is derived from:

- `progress.bin.current_page_start_offset`
- `chapters.idx`

The active chapter is the last chapter record whose `start_offset <= current_page_start_offset`.

Derived footer fields:

- `chapter_title`: cached chapter title for the active chapter
- `progress_percent`: existing byte-offset-based progress percent

## Boundaries

### `xteink-fs`

Owns:

- reading `chapters.idx`
- resolving the active chapter from `current_page_start_offset`
- building a small footer view model with chapter title and progress

### `xteink-render`

Owns:

- laying out footer text segments within the available footer width
- cropping the title segment to fit after left and right segments are reserved

`xteink-render` should not learn about `chapters.idx` or cache-file formats.

## Footer Layout

Footer is a single line with three segments:

- left: chapter title, cropped if needed
- right: progress percent, for example `37%`

Layout rules:

1. Measure the rendered width of the left and right segments first.
2. Reserve at least one character cell of spacing between title and right.
3. The remaining width belongs to the title.
4. If the remaining width is zero or negative, omit the title.
5. If the title width exceeds the available width, hard-crop it to the maximum width that still preserves right and the required space.
6. If the title fits, render it unchanged.

Progress must take priority over the title.

## Chapter Resolution

Algorithm:

1. Load parsed chapter metadata from `chapters.idx`.
2. If chapter metadata exists, scan in order and keep the last chapter whose start offset is less than or equal to `current_page_start_offset`.
3. If no chapter matches but metadata exists, use the first chapter.
4. If chapter metadata is unavailable or invalid, omit the chapter title.

This keeps footer behavior deterministic and aligned with existing offset-based reader state.

## Failure Handling

- invalid or missing `chapters.idx`: show only progress
- empty chapter title: show only progress
- too little footer width: preserve progress, drop title

## Testing

- footer layout test proving title and progress fit with the required spacing
- chapter resolution test proving a page offset maps to the correct chapter title
- integration test proving the rendered footer includes the chapter title prefix and progress on a real cached EPUB fixture

## Recommendation

Implement the feature by resolving chapter footer data in `xteink-fs`, passing plain footer fields through the app layer, and using `xteink-render` for width-constrained footer layout. This keeps persistence and chapter lookup close to cache metadata while limiting `xteink-render` to layout concerns.
