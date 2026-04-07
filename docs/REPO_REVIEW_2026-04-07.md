# Repository Review: Stability, Maintainability, and Clarity

Date: 2026-04-07

## Scope

This review is based on the tracked workspace sources, manifests, docs, and available tests. The current worktree is dirty, so findings below focus on structural issues that are visible regardless of in-progress edits.

## Executive Summary

The repository already has the right high-level shape for an embedded product: parsing, display, storage, input, browser, and firmware integration are separated into crates. The weak spots are mostly in execution, not intent:

- the host verification path is fragile and currently broken for normal workspace commands
- the firmware integration layer is too large and owns too many responsibilities
- the EPUB and display pipeline relies on raw-pointer and global-workspace patterns that make correctness harder to reason about
- filesystem code currently mixes storage, cache policy, rendering orchestration, platform details, and debug logging
- test coverage is concentrated in lower-risk areas while the highest-risk paths remain lightly defended

The fastest stability win is to fix the build and test topology. The biggest maintainability win is to split the firmware and EPUB/render pipeline into safer, smaller units with clearer ownership.

## Major Weak Spots

### 1. Verification workflow is brittle and partially broken

Evidence:

- [`.cargo/config.toml`](/Users/jakubkolcar/customs/cool/.cargo/config.toml) forces a workspace-wide embedded default target and `build-std = ["core"]`.
- [`firmware/Cargo.toml`](/Users/jakubkolcar/customs/cool/firmware/Cargo.toml) is always a workspace member, even for host test runs.
- `cargo test --workspace --target aarch64-apple-darwin` fails because `esp-hal` rejects host builds.
- `cargo test -p xteink-epub --target aarch64-apple-darwin` fails with duplicate `core` lang items because the workspace-level `build-std = ["core"]` leaks into host testing.
- [`scripts/run-tests-host.sh`](/Users/jakubkolcar/customs/cool/scripts/run-tests-host.sh) works around this by enumerating crates manually, which is a symptom of an unhealthy default verification path.

Why this matters:

- It is too easy to think the workspace is healthy when only a narrow script path works.
- CI and local validation are likely to drift further as more crates are added.
- Developers will avoid running broad checks if the defaults are unreliable.

Recommended changes:

1. Remove the workspace-global host/embedded coupling from [`.cargo/config.toml`](/Users/jakubkolcar/customs/cool/.cargo/config.toml).
2. Move embedded target selection into explicit scripts, `cargo aliases`, or package-level config for firmware-only commands.
3. Gate firmware-only crates behind target-specific CI jobs instead of keeping them on the default host verification path.
4. Add a single documented verification entrypoint that runs:
   - host unit tests for pure crates
   - firmware `cargo check` for the embedded target
   - a small number of integration smoke tests

### 2. `firmware/src/main.rs` is acting as the application, controller, state machine, and HAL adapter at once

Evidence:

- [`firmware/src/main.rs`](/Users/jakubkolcar/customs/cool/firmware/src/main.rs) is 921 lines.
- It owns peripheral initialization, ADC sampling, event capture, browser state, reader state, display refresh scheduling, and screen rendering.
- It keeps shared mutable app state in the global `APP_DIRECTORY_PAGE` mutex and uses `unsafe` mutation through `lock_mut` at [main.rs:79](/Users/jakubkolcar/customs/cool/firmware/src/main.rs:79).
- It also contains direct register-level ADC access at [main.rs:155](/Users/jakubkolcar/customs/cool/firmware/src/main.rs:155) and a plain `.unwrap()` during SPI init at [main.rs:211](/Users/jakubkolcar/customs/cool/firmware/src/main.rs:211).

Why this matters:

- Most product behavior changes will continue to pile into one file.
- It is hard to test UI and navigation rules without the hardware setup path.
- Safety and behavior regressions are difficult to isolate because control flow is spread across async tasks plus global state.

Recommended changes:

1. Introduce a small `AppState` and `AppController` layer that owns:
   - current path
   - browser page state
   - reader state
   - pending refresh state
2. Move hardware setup into a dedicated `board` or `hal_adapter` module.
3. Convert button-to-action and action-to-state transitions into pure functions so they can be host tested.
4. Remove the global directory-page store and pass explicit state through the controller.

### 3. The EPUB/rendering pipeline currently depends on patterns that are hard to audit

Evidence:

- [`crates/xteink-display/src/lib.rs`](/Users/jakubkolcar/customs/cool/crates/xteink-display/src/lib.rs) is 1502 lines and owns both low-level panel behavior and EPUB pagination/rendering.
- It uses a global mutable workspace with `static mut`, `MaybeUninit`, and critical-section guarded initialization around [lib.rs:100](/Users/jakubkolcar/customs/cool/crates/xteink-display/src/lib.rs:100).
- [`crates/xteink-epub/src/lib.rs`](/Users/jakubkolcar/customs/cool/crates/xteink-epub/src/lib.rs) is 1807 lines and converts `ReaderBuffers` into raw pointers, then repeatedly reconstructs mutable references from those pointers in `next_event` around [lib.rs:166](/Users/jakubkolcar/customs/cool/crates/xteink-epub/src/lib.rs:166).
- The archive is reparsed in multiple places, including `prepare_catalog` and `load_current_chapter`.

Why this matters:

- The current shape is likely compensating for borrow-checker pressure rather than reflecting a clear domain model.
- Safety arguments are implicit and spread across two crates.
- This raises the cost of changing buffer sizes, parse flow, or cache behavior without introducing subtle regressions.

Recommended changes:

1. Introduce an explicit `EpubSession` or `ReaderSession` type that owns the parser state and workspace buffers safely.
2. Split `xteink-display` into:
   - panel driver and refresh scheduling
   - text layout and pagination
   - EPUB-to-render adapter
3. Split `xteink-epub` into:
   - ZIP/archive index
   - OPF/catalog resolver
   - chapter stream/parser
4. Make reparsing/indexing behavior explicit and cacheable at the session level.

### 4. The filesystem crate currently carries too many concerns

Evidence:

- [`crates/xteink-fs/src/browser.rs`](/Users/jakubkolcar/customs/cool/crates/xteink-fs/src/browser.rs) handles directory paging, cache probing, cached progress, EPUB rendering orchestration, and display refresh mode selection.
- [`crates/xteink-fs/src/low_level.rs`](/Users/jakubkolcar/customs/cool/crates/xteink-fs/src/low_level.rs) contains platform-specific SPI transport setup, SD mounting, path traversal, file open logic, and many `esp_println!` calls.
- [`crates/xteink-fs/Cargo.toml`](/Users/jakubkolcar/customs/cool/crates/xteink-fs/Cargo.toml) advertises an `embedded` feature, but the crate still effectively serves as both embedded adapter and higher-level application service.

Why this matters:

- `xteink-fs` is no longer just a storage crate.
- It is harder to reuse or test the cache and browsing behavior independently from the platform adapter.
- Verbose embedded logging inside core file operations increases noise and couples diagnostics to implementation details.

Recommended changes:

1. Split `xteink-fs` into:
   - a pure storage/cache policy crate
   - an embedded SD adapter crate
2. Move cache orchestration out of the low-level file access path.
3. Replace unconditional `esp_println!` calls with a thin logging facade or feature-gated tracing.
4. Make cache invalidation and progress persistence a documented service boundary, not incidental behavior inside rendering helpers.

### 5. Test coverage is not aligned with risk

Evidence:

- There are solid unit tests in lower-level crates like [`crates/xteink-buttons/src/lib.rs`](/Users/jakubkolcar/customs/cool/crates/xteink-buttons/src/lib.rs), [`crates/xteink-browser/src/lib.rs`](/Users/jakubkolcar/customs/cool/crates/xteink-browser/src/lib.rs), and [`crates/xteink-input/tests/input_manager.rs`](/Users/jakubkolcar/customs/cool/crates/xteink-input/tests/input_manager.rs).
- The most complex files, especially [`firmware/src/main.rs`](/Users/jakubkolcar/customs/cool/firmware/src/main.rs), [`crates/xteink-display/src/lib.rs`](/Users/jakubkolcar/customs/cool/crates/xteink-display/src/lib.rs), and [`crates/xteink-epub/src/lib.rs`](/Users/jakubkolcar/customs/cool/crates/xteink-epub/src/lib.rs), have the highest behavioral density and the weakest isolated verification story.
- Current scripts separate host and embedded tests manually instead of expressing a stable test matrix at the workspace level.

Why this matters:

- The highest-risk product paths are exactly where regressions are most expensive to debug on device.
- Refactoring pressure will stay low if tests are expensive or hardware-dependent.

Recommended changes:

1. Add pure reducer-style tests for firmware UI state transitions.
2. Add parser progression tests around chapter boundaries, cache invalidation, and cancellation.
3. Add regression tests for cached-page rendering versus direct rendering for the same EPUB fixture.
4. Add at least one host smoke test that exercises browse -> open EPUB -> next page -> exit at the controller level.

### 6. Documentation and repo hygiene are drifting away from the actual architecture

Evidence:

- [`docs/PROJECT_OVERVIEW.md`](/Users/jakubkolcar/customs/cool/docs/PROJECT_OVERVIEW.md) describes focused crate boundaries, but actual orchestration is more entangled.
- [`docs/ZERO_COPY_GUIDE.md`](/Users/jakubkolcar/customs/cool/docs/ZERO_COPY_GUIDE.md) is a generic reference document rather than project documentation.
- [`.gitignore`](/Users/jakubkolcar/customs/cool/.gitignore) does not ignore `.cool/`, and the current worktree already contains `.cool/` build artifacts.

Why this matters:

- Project docs should reduce ambiguity, not add unrelated material.
- Untracked generated artifacts make review and debugging noisier.

Recommended changes:

1. Replace generic docs with project-specific design notes and short ADRs.
2. Document the intended crate boundaries and current exceptions explicitly.
3. Ignore local generated work directories like `.cool/` if they are expected.

## Prioritized Improvement Plan

### Phase 1: Stabilize the engineering loop

- Fix host versus embedded build configuration separation.
- Make one default verification command succeed on a clean checkout.
- Clean up generated artifact handling in `.gitignore`.

### Phase 2: Extract controllable application logic

- Pull app state and UI transitions out of `firmware/src/main.rs`.
- Add host tests for navigation, reader entry/exit, and paging.
- Remove global mutable directory-page state.

### Phase 3: Reduce unsafe and implicit ownership in the reader pipeline

- Introduce a safe session object for EPUB parsing/rendering.
- Separate panel-driver code from pagination/rendering code.
- Reduce raw-pointer reconstruction in parser entrypoints.

### Phase 4: Clarify storage and caching boundaries

- Split low-level SD access from cache and reader orchestration.
- Feature-gate logging and standardize error/reporting boundaries.
- Add targeted tests for cache correctness and invalidation.

## Suggested Success Criteria

- `cargo test` and `cargo check` paths are explicit, documented, and reliable for both host and embedded targets.
- `firmware/src/main.rs` becomes a thin wiring layer instead of a behavior hub.
- `xteink-display` and `xteink-epub` each have smaller modules with clear ownership and fewer safety comments.
- Cache behavior, parser progression, and reader navigation are covered by deterministic host tests.

## Short Version

If only three major changes are made, they should be:

1. Fix the build/test topology so host and embedded verification are first-class and reliable.
2. Break up `firmware/src/main.rs` into a testable controller plus hardware adapters.
3. Replace the current raw-pointer/global-workspace EPUB pipeline with a safe session-oriented design.
