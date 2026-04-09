# Grayscale Antialiasing Design

**Date:** 2026-04-09

**Goal**

Implement 4-shade grayscale antialiasing across the shared renderer, web simulator, desktop simulator, and hardware display path in one coordinated change, while keeping the design easy to extend to 16 shades later.

## Requirements

- Preserve grayscale glyph coverage during font generation instead of collapsing coverage to 1-bit.
- Replace the current 1-bit shared framebuffer with a multi-bit render target.
- Use the shared framebuffer in all runtimes:
  - web simulator
  - desktop simulator
  - hardware display path
- Support 4 shades in the first version.
- Make the architecture easy to switch to 16 shades later.
- Avoid a hardware full-refresh-only solution.
- Use a hardware multi-pass fast-update approximation for the initial grayscale implementation.

## Non-Goals

- Perfect hardware grayscale fidelity.
- A native SSD1677 waveform-driven grayscale implementation.
- Independent renderer forks per runtime.
- A text-only grayscale overlay path.

## Chosen Approach

Adopt a shared 2-bit framebuffer, preserve quantized grayscale glyph coverage in generated font assets, and implement per-runtime output adapters:

- web: direct 4-shade canvas output
- desktop: direct 4-shade window output
- hardware: 4-shade approximation via multiple fast update passes derived from the shared framebuffer

This is the smallest architecture that satisfies the runtime-sharing requirement and leaves a clean upgrade path to 16 shades later.

## Alternatives Considered

### 1. Shared 2-bit framebuffer with runtime adapters

Chosen.

Pros:
- one shared render model
- direct path to 16 shades later
- matches the requirement to change hardware, web, and desktop together

Cons:
- broad change touching core renderer and hardware adapter
- hardware quality will still be approximation-based

### 2. Grayscale only in web and desktop, 1-bit hardware fallback

Rejected.

Pros:
- much lower risk

Cons:
- violates the “same change” requirement in substance
- would require later rework to retrofit hardware

### 3. Separate grayscale text overlay on top of the existing 1-bit framebuffer

Rejected.

Pros:
- faster prototype for text only

Cons:
- wrong long-term shape for full-frame grayscale
- awkward to extend to 16 shades
- adds composition complexity without solving the core buffer limitation

## Architecture

### 1. Shared grayscale framebuffer

Replace the current `xteink_render::Framebuffer` storage with a compact shade buffer abstraction.

Initial target:
- 4 shades
- 2 bits per pixel

Design requirement:
- the packing/unpacking logic must be isolated so the framebuffer can later move from 2 bits per pixel to 4 bits per pixel with minimal API churn

Core API direction:
- `set_shade(x, y, shade)`
- `shade_at(x, y)`
- fill/clear helpers expressed in shade levels rather than binary color

Compatibility strategy:
- preserve a small number of helper methods for code that conceptually wants black/white
- implement those helpers in terms of the grayscale API rather than maintaining two framebuffer models

### 2. Font asset generation

The Bookerly build step currently renders grayscale coverage via FreeType and then collapses it to 1-bit.

Change:
- quantize source coverage into 4 shade levels
- store per-glyph shade data instead of 1-bit packed rows

The generator should define quantization and storage in a way that can later support 16 shades by changing:
- quantization thresholds
- bits per stored pixel
- packing/unpacking helpers

Text shaping, glyph metrics, and pagination stay unchanged.

### 3. Shared rasterization

Glyph drawing in `xteink-render` should write grayscale coverage into the shared framebuffer.

Scope:
- text rendering
- wrapped text rendering
- EPUB pagination rendering

Blending policy for v1:
- treat glyph shades as overwrite-on-white for text paths already assuming white page backgrounds
- do not introduce generalized alpha composition unless required by existing drawing code

This keeps the first version smaller while still producing grayscale antialiasing.

### 4. Web adapter

The web simulator should translate framebuffer shade values directly into RGBA output.

Mapping for 4 shades:
- level 0: white
- level 1: light gray
- level 2: dark gray
- level 3: black

The web output path should consume the shared grayscale framebuffer directly and must not re-threshold it back to black/white.

### 5. Desktop adapter

The desktop simulator should map framebuffer shades directly to 32-bit window pixels using the same 4-shade ramp as the web path.

The desktop simulator should remain a faithful visual debugging target for shared rendering behavior.

### 6. Hardware adapter

The hardware path is the highest-risk area.

Constraints:
- same shared grayscale framebuffer as all other runtimes
- avoid a full-refresh-only implementation

Chosen v1 strategy:
- derive per-shade pass masks from the shared framebuffer
- emit multiple fast update passes that approximate 4 shades

This is an approximation, not a native grayscale waveform implementation.

The hardware adapter should keep the pass-generation logic isolated from the shared renderer. The renderer produces shade values; the display driver decides how to approximate them.

## Data Flow

1. Bookerly build script generates quantized grayscale glyph coverage.
2. Shared shaping/layout code computes glyph positions exactly as today.
3. Shared renderer writes shade levels into the grayscale framebuffer.
4. Runtime adapters consume the same framebuffer:
   - web -> canvas grayscale pixels
   - desktop -> minifb grayscale pixels
   - hardware -> multi-pass fast-update masks

## Extensibility To 16 Shades

The design must isolate all shade-count assumptions behind a small set of components:

- framebuffer pixel packing/unpacking
- glyph coverage packing/unpacking
- shade ramp lookup tables
- hardware pass-generation strategy

The rest of the renderer should speak in terms of abstract shade values and not hardcode 4-shade assumptions.

## File-Level Design

### Shared renderer and font pipeline

- `crates/xteink-render/build.rs`
  - generate quantized grayscale glyph data instead of 1-bit glyph bitmaps
- `crates/xteink-render/src/bookerly.rs`
  - update glyph data access structures if needed for grayscale coverage
- `crates/xteink-render/src/lib.rs`
  - replace the binary framebuffer representation and update drawing methods
- `crates/xteink-render/tests/text_render.rs`
  - validate generated glyph coverage against quantized FreeType output
- `crates/xteink-render/tests/framebuffer.rs`
  - validate pixel packing and shade semantics

### Desktop simulator

- `simulator/src/window.rs`
  - render 4-shade pixels directly
- `simulator/src/runtime.rs`
  - no logic change expected beyond continuing to pass the new shared framebuffer through

### Web simulator

- `web-simulator/src/lib.rs`
  - convert grayscale framebuffer values into RGBA output
- `web/app.js`
  - no architectural rendering change expected beyond continuing to drive the wasm app

### Hardware display path

- `crates/xteink-display/src/lib.rs`
  - add grayscale pass generation and multi-pass fast update scheduling
- `firmware` display call sites
  - keep existing call patterns if possible, but route through the grayscale-capable adapter

## Error Handling

- Renderer should validate shade bounds in debug/test paths.
- Hardware grayscale path should fail safely to the existing binary path if a pass schedule cannot be applied.
- Web and desktop should not silently re-threshold the buffer.

## Testing Strategy

### Unit tests

- framebuffer packing/unpacking for 2-bit pixels
- glyph coverage quantization behavior
- text rendering writes multiple shade values, not only black/white

### Integration tests

- EPUB render tests continue to pass with the grayscale framebuffer
- web wasm build still succeeds
- desktop simulator path still compiles and renders

### Visual/smoke verification

- compare sample text output in desktop and web for the same page
- verify hardware approximation produces visibly distinct shade steps without forcing full refresh

## Risks

### 1. Hardware quality risk

The SSD1677 approximation may show ghosting or weak separation between shades.

Mitigation:
- isolate hardware pass logic
- keep grayscale approximation configurable
- preserve a binary fallback path

### 2. Memory footprint growth

Moving from 1-bit to 2-bit framebuffer storage increases buffer size materially.

Mitigation:
- quantify device footprint after the framebuffer change
- keep packing compact from the start

### 3. Cross-runtime divergence

Web/desktop may look good while hardware looks materially worse.

Mitigation:
- keep one renderer and separate only the final output adapter stage

## Success Criteria

- text in web and desktop is visibly cleaner with grayscale antialiasing
- hardware displays 4 distinguishable shade levels via fast-update approximation
- no renderer forks are introduced
- the code structure makes 16-shade work a local extension rather than a redesign
