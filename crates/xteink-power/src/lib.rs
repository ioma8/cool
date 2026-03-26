#![no_std]

#[cfg(test)]
extern crate std;

/// Idle timeout before the firmware enters deep sleep.
pub const AWAKE_TIMEOUT_MS: u64 = 5 * 60 * 1000;

/// Simplified wake source used by the host-testable policy logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeCause {
    Undefined,
    Gpio,
    Other,
}

/// Simplified reset reason used by the host-testable policy logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetReason {
    ChipPowerOn,
    CoreDeepSleep,
    Other,
}

/// Derived boot classification used by the firmware.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeupReason {
    PowerButton,
    AfterFlash,
    AfterUSBPower,
    Other,
}

/// Classify the boot reason from wake source, reset reason, and USB presence.
pub fn classify_wakeup_reason(
    wake_cause: WakeCause,
    reset_reason: Option<ResetReason>,
    usb_connected: bool,
) -> WakeupReason {
    match (wake_cause, reset_reason, usb_connected) {
        (WakeCause::Undefined, Some(ResetReason::ChipPowerOn), false) => WakeupReason::PowerButton,
        (WakeCause::Gpio, Some(ResetReason::CoreDeepSleep), true) => WakeupReason::PowerButton,
        (WakeCause::Undefined, None, true) => WakeupReason::AfterFlash,
        (WakeCause::Undefined, Some(ResetReason::ChipPowerOn), true) => WakeupReason::AfterUSBPower,
        _ => WakeupReason::Other,
    }
}

/// Returns `true` once the idle timeout has been reached.
pub fn should_enter_sleep(elapsed_ms: u64) -> bool {
    elapsed_ms >= AWAKE_TIMEOUT_MS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_power_button_boot_without_usb() {
        let reason =
            classify_wakeup_reason(WakeCause::Undefined, Some(ResetReason::ChipPowerOn), false);

        assert_eq!(reason, WakeupReason::PowerButton);
    }

    #[test]
    fn classifies_power_button_gpio_wake_with_usb() {
        let reason =
            classify_wakeup_reason(WakeCause::Gpio, Some(ResetReason::CoreDeepSleep), true);

        assert_eq!(reason, WakeupReason::PowerButton);
    }

    #[test]
    fn classifies_after_flash_boot() {
        let reason = classify_wakeup_reason(WakeCause::Undefined, None, true);

        assert_eq!(reason, WakeupReason::AfterFlash);
    }

    #[test]
    fn classifies_usb_power_boot() {
        let reason =
            classify_wakeup_reason(WakeCause::Undefined, Some(ResetReason::ChipPowerOn), true);

        assert_eq!(reason, WakeupReason::AfterUSBPower);
    }

    #[test]
    fn classifies_unknown_combinations_as_other() {
        let reason = classify_wakeup_reason(WakeCause::Other, Some(ResetReason::Other), false);

        assert_eq!(reason, WakeupReason::Other);
    }

    #[test]
    fn timeout_helper_matches_five_minute_policy() {
        assert!(!should_enter_sleep(AWAKE_TIMEOUT_MS - 1));
        assert!(should_enter_sleep(AWAKE_TIMEOUT_MS));
    }
}
