macro_rules! logln {
    ($($arg:tt)*) => {{
        #[cfg(target_arch = "riscv32")]
        {
            let _ = esp_println::println!($($arg)*);
        }
        #[cfg(not(target_arch = "riscv32"))]
        {
        }
    }};
}

pub(crate) use logln;
