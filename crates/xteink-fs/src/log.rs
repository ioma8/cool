macro_rules! logln {
    ($($arg:tt)*) => {{
        #[cfg(all(target_arch = "riscv32", feature = "debug-logging"))]
        {
            let _ = esp_println::println!($($arg)*);
        }
        #[cfg(not(all(target_arch = "riscv32", feature = "debug-logging")))]
        {
        }
    }};
}

pub(crate) use logln;
