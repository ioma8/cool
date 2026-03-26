# Sleep and Wake Specification

This document describes the current wakeup classification and deep-sleep behavior implemented in `firmware/src/main.rs` and `xteink-power/src/lib.rs`.

## Overview

The firmware uses ESP32-C3 deep sleep as its low-power mode.

There are three related behaviors:

- boot-time wakeup classification
- runtime idle timeout before sleeping
- display-controller shutdown before MCU deep sleep

## USB Detection at Boot

Before any wakeup classification, the firmware samples GPIO20:

- GPIO20 is configured as a plain input
- if GPIO20 is high, the firmware considers USB to be connected

If USB is detected, the firmware waits 500 ms before printing boot messages. This is only to give the USB-JTAG serial path time to become ready.

## Wakeup Classification Inputs

The function `get_wakeup_reason(usb_connected: bool)` classifies boot reason from:

- `wakeup_cause()`
- `reset_reason(Cpu::ProCpu)`
- USB-connected state from GPIO20

The result is one of:

- `PowerButton`
- `AfterFlash`
- `AfterUSBPower`
- `Other`

## Exact Wakeup Classification Rules

### `PowerButton`

Returned in either of these cases:

- wake cause = `SleepSource::Undefined`, reset reason = `Some(SocResetReason::ChipPowerOn)`, USB not connected
- wake cause = `SleepSource::Gpio`, reset reason = `Some(SocResetReason::CoreDeepSleep)`, USB connected

### `AfterFlash`

Returned when:

- wake cause = `SleepSource::Undefined`
- reset reason = `None`
- USB connected

The current firmware treats this as a normal boot path after flashing.

### `AfterUSBPower`

Returned when:

- wake cause = `SleepSource::Undefined`
- reset reason = `Some(SocResetReason::ChipPowerOn)`
- USB connected

The current firmware treats this as an unwanted cold boot caused only by USB power.

### `Other`

Returned for every other combination.

## Boot-Time Behavior per Wakeup Reason

### `AfterUSBPower`

The firmware does not continue normal application startup.

It performs:

1. print debug message
2. delay 100 ms
3. configure a timer wakeup source for 3600 seconds
4. enter MCU deep sleep immediately

This behavior prevents the device from staying awake solely because USB power is present.

### `AfterFlash`

The firmware proceeds normally:

- initializes display
- initializes button inputs
- enters the main loop

### `PowerButton`

The firmware proceeds normally.

### `Other`

The firmware also proceeds normally.

## Awake Timeout

The firmware maintains an idle timeout constant:

- `AWAKE_TIMEOUT_MS = 300000`
- equivalent to 5 minutes

At boot, `last_activity` is initialized to `Instant::now()`.

`last_activity` is updated only when a newly pressed button is detected.

## Runtime Idle Handling

During the main loop:

1. current button state is sampled
2. new button presses are detected
3. if a new button press occurs, `last_activity` is reset
4. elapsed idle time is computed from `last_activity.elapsed().as_millis()`

If elapsed idle time is greater than or equal to 300000 ms, the firmware enters its sleep path.

## Sleep Entry Sequence

When the idle timeout is reached, the firmware performs:

1. print `Idle timeout reached, entering deep sleep...`
2. call `display.deep_sleep()`
3. configure a timer wakeup source for 3600 seconds
4. print `Sleeping now...`
5. delay 100 ms
6. call `rtc.sleep_deep(&[&timer])`

The code comments mention waking on timer as a fallback or power button, but the current Rust code only explicitly configures the timer source at the point of `sleep_deep()`.

## Display State Before MCU Sleep

Immediately before MCU deep sleep, the firmware instructs the SSD1677 controller to enter its own deep-sleep mode through `display.deep_sleep()`.

If the internal display flag says the panel is on, the driver first performs a display power-down sequence:

1. send update control 1 (`0x21`) with `0x40`
2. send update control 2 (`0x22`) with `0x03`
3. send master activation (`0x20`)
4. wait while `BUSY` is high
5. mark the display as off

Then it always sends:

- deep sleep command `0x10`
- data byte `0x01`

This sequence does not clear the panel. The visible e-ink image is therefore expected to remain on screen during MCU deep sleep.

## Wake Sources and Current Assumptions

The code explicitly creates only a timer wakeup source:

- duration: 3600 seconds

In addition, the wakeup classification logic expects at least one GPIO-based wake path to exist because it recognizes:

- `SleepSource::Gpio` with `CoreDeepSleep` reset as a power-button wake case when USB is connected

That GPIO wake configuration is not set up explicitly in the current Rust source files in this repository snapshot. If GPIO wake works on hardware, it is coming from behavior outside the visible code path or from defaults established by the platform.

## Reset and Wake Diagnostics

On every boot, the firmware prints:

- reset reason
- wake cause
- derived `WakeupReason`

This debug output is the primary runtime diagnostic for validating whether the device woke from flash, USB power, or a user action.
