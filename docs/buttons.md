# Button Input Specification

This document describes the current button input implementation in `firmware/src/main.rs` and `xteink-buttons/src/lib.rs`.

## Physical Inputs

The firmware handles seven buttons:

- Back
- Confirm
- Left
- Right
- Up
- Down
- Power

## Electrical Interface

### ADC-backed buttons

The firmware uses two analog inputs with 11 dB attenuation:

- GPIO1 on ADC1 for `Back`, `Confirm`, `Left`, `Right`
- GPIO2 on ADC1 for `Up`, `Down`

ADC setup:

- peripheral: `ADC1`
- attenuation: `11 dB`
- read method: blocking one-shot reads via `nb::block!(adc.read_oneshot(...))`
- read fallback on error: `9999`

### Digital button

The power button is read as a digital input:

- GPIO3
- input mode with pull-up enabled
- active level: low

If `GPIO3` reads low, the `Power` button is considered pressed.

## ADC Thresholds

No-button threshold for both ADC channels:

- any value greater than `3800` is treated as no button pressed

### GPIO1 / ADC group 1

Current ranges are implemented as:

- `3100 < value <= 3800` -> `Back`
- `2090 < value <= 3100` -> `Confirm`
- `750 < value <= 2090` -> `Left`
- `0 < value <= 750` -> `Right`

Important edge behavior:

- `value = 0` does not match any button because the comparison is strictly greater than the lower bound
- `value > 3800` is treated as no button

### GPIO2 / ADC group 2

Current ranges are implemented as:

- `1120 < value <= 3800` -> `Up`
- `0 < value <= 1120` -> `Down`

Important edge behavior:

- `value = 0` does not match any button
- `value > 3800` is treated as no button

## Button Enumeration and Bit Layout

Each button is assigned a fixed bit in `ButtonState.state`:

- bit 0 -> `Back`
- bit 1 -> `Confirm`
- bit 2 -> `Left`
- bit 3 -> `Right`
- bit 4 -> `Up`
- bit 5 -> `Down`
- bit 6 -> `Power`

`ButtonState` is an `u8` bitfield.

## Main Loop Sampling

The main loop performs the following on each iteration:

1. Create a fresh empty `ButtonState`
2. Read GPIO1 ADC value
3. Map that value to at most one button from the group-1 ranges
4. Read GPIO2 ADC value
5. Map that value to at most one button from the group-2 ranges
6. Read GPIO3 and add `Power` if the pin is low
7. Compare the current state to the previous state

The loop then waits 20 ms before the next iteration, unless a new button press was detected, in which case an additional 150 ms delay is inserted.

## Press Detection Model

The firmware uses edge detection rather than level-triggered handling.

Newly pressed buttons are computed as:

- `newly_pressed = current_state & !last_state`

This means:

- holding a button does not repeatedly trigger actions
- a button only triggers when it transitions from not-pressed to pressed
- simultaneous presses can exist in the bitfield, but the loop only handles one of them per iteration

## Press Priority

When more than one button is marked pressed in `newly_pressed`, the firmware selects the first one using this fixed priority order:

1. Back
2. Confirm
3. Left
4. Right
5. Up
6. Down
7. Power

This is bit-order priority, not time-of-arrival priority.

## On-Press Behavior

When a new button press is detected:

1. `press_count` is incremented
2. the button name and count are printed to the debug console
3. the current ADC readings are printed to the debug console
4. a text line `N: ButtonName` is drawn into the framebuffer
5. a fast e-ink refresh is triggered
6. `last_activity` is reset for sleep timeout tracking
7. the loop waits 150 ms for basic debounce

## Display Logging Coordinates

The button log starts at:

- X = 10
- Y = 80

Each detected press advances Y by 20 pixels.

If Y reaches or exceeds `480`, the firmware:

1. clears the framebuffer to white
2. draws `Button Press Log (continued)` at `(10, 10)`
3. performs a full refresh
4. resets the next line position to Y = 40

This wrap rule is based on `MAX_LINES = 24` and a 20-pixel line step.

## Debug Output

The firmware prints periodic debug state approximately every 100 loop iterations.

With the current 20 ms loop delay, this is roughly every 2 seconds and prints:

- raw ADC1 value
- raw ADC2 value
- current button-state bitfield

## Current Limitations

- Each ADC input can resolve only one button from its resistor ladder at a time.
- Simultaneous presses from different groups can exist, but only the highest-priority newly pressed button is acted on in a given loop iteration.
- The firmware does not currently apply averaging, hysteresis, or calibration to ADC readings.
- `value = 0` on either ADC channel maps to no button because of the lower-bound comparison rule in the current implementation.
