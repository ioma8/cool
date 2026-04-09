#![no_std]

pub const DEVICE_TOTAL_RAM_BYTES: usize = 380 * 1024;
pub const DEVICE_STACK_RESERVE_BYTES: usize = 128 * 1024;
pub const DEVICE_TRANSIENT_HEADROOM_BYTES: usize = 32 * 1024;
pub const DEVICE_PERSISTENT_BUDGET_BYTES: usize =
    DEVICE_TOTAL_RAM_BYTES - DEVICE_STACK_RESERVE_BYTES - DEVICE_TRANSIENT_HEADROOM_BYTES;

pub const DISPLAY_DRIVER_EPUB_WORKSPACE_LIMIT_BYTES: usize = 64 * 1024;
pub const SHARED_RENDER_EPUB_WORKSPACE_LIMIT_BYTES: usize = 160 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceMemoryFootprint {
    pub device_bytes: usize,
    pub device_heap_bytes: usize,
    pub host_only_bytes: usize,
}

impl DeviceMemoryFootprint {
    #[must_use]
    pub const fn new(device_bytes: usize) -> Self {
        Self::with_breakdown(device_bytes, 0, 0)
    }

    #[must_use]
    pub const fn with_breakdown(
        device_bytes: usize,
        device_heap_bytes: usize,
        host_only_bytes: usize,
    ) -> Self {
        Self {
            device_bytes,
            device_heap_bytes,
            host_only_bytes,
        }
    }

    #[must_use]
    pub const fn with_host_overhead(device_bytes: usize, host_only_bytes: usize) -> Self {
        Self::with_breakdown(device_bytes, 0, host_only_bytes)
    }

    #[must_use]
    pub const fn fits_device_budget(self) -> bool {
        self.device_bytes <= DEVICE_PERSISTENT_BUDGET_BYTES
    }

    #[must_use]
    pub const fn remaining_device_bytes(self) -> usize {
        DEVICE_PERSISTENT_BUDGET_BYTES.saturating_sub(self.device_bytes)
    }

    #[must_use]
    pub const fn used_device_permille(self) -> usize {
        if DEVICE_PERSISTENT_BUDGET_BYTES == 0 {
            0
        } else {
            self.device_bytes.saturating_mul(1000) / DEVICE_PERSISTENT_BUDGET_BYTES
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persistent_budget_is_lower_than_total_ram() {
        assert!(DEVICE_PERSISTENT_BUDGET_BYTES < DEVICE_TOTAL_RAM_BYTES);
        assert_eq!(
            DEVICE_PERSISTENT_BUDGET_BYTES,
            DEVICE_TOTAL_RAM_BYTES - DEVICE_STACK_RESERVE_BYTES - DEVICE_TRANSIENT_HEADROOM_BYTES
        );
    }

    #[test]
    fn footprint_reports_remaining_device_bytes() {
        let footprint = DeviceMemoryFootprint::with_breakdown(100 * 1024, 32 * 1024, 512 * 1024);
        assert!(footprint.fits_device_budget());
        assert_eq!(
            footprint.remaining_device_bytes(),
            DEVICE_PERSISTENT_BUDGET_BYTES - 100 * 1024
        );
        assert_eq!(footprint.device_heap_bytes, 32 * 1024);
    }
}
