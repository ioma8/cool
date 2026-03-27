//! SD card protocol constants and errors.

use core::fmt;

pub const INIT_CLOCK_HZ: u32 = 400_000;
pub const DATA_CLOCK_HZ: u32 = 40_000_000;

pub const CMD0: u8 = 0x00;
pub const CMD8: u8 = 0x08;
pub const CMD9: u8 = 0x09;
pub const CMD12: u8 = 0x0C;
pub const CMD17: u8 = 0x11;
pub const CMD18: u8 = 0x12;
pub const CMD24: u8 = 0x18;
pub const CMD25: u8 = 0x19;
pub const CMD55: u8 = 0x37;
pub const CMD58: u8 = 0x3A;
pub const CMD59: u8 = 0x3B;
pub const ACMD23: u8 = 0x17;
pub const ACMD41: u8 = 0x29;

pub const R1_READY_STATE: u8 = 0x00;
pub const R1_IDLE_STATE: u8 = 0x01;
pub const R1_ILLEGAL_COMMAND: u8 = 0x04;

pub const DATA_START_BLOCK: u8 = 0xFE;
pub const STOP_TRAN_TOKEN: u8 = 0xFD;
pub const WRITE_MULTIPLE_TOKEN: u8 = 0xFC;
pub const DATA_RES_MASK: u8 = 0x1F;
pub const DATA_RES_ACCEPTED: u8 = 0x05;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardType {
    SD1,
    SD2,
    SDHC,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SdSpiOptions {
    pub use_crc: bool,
}

impl Default for SdSpiOptions {
    fn default() -> Self {
        Self { use_crc: true }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Error<SpiE, PinE> {
    Spi(SpiE),
    Pin(PinE),
    TimeoutCommand(u8),
    TimeoutACommand(u8),
    TimeoutReadBuffer,
    TimeoutWaitNotBusy,
    Cmd58Error,
    RegisterReadError,
    CrcError(u16, u16),
    ReadError,
    WriteError,
    CardNotFound,
    BadState,
    Unsupported,
}

impl<SpiE: fmt::Debug, PinE: fmt::Debug> fmt::Debug for Error<SpiE, PinE> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spi(_) => f.debug_tuple("Spi").finish(),
            Self::Pin(_) => f.debug_tuple("Pin").finish(),
            Self::TimeoutCommand(cmd) => f.debug_tuple("TimeoutCommand").field(cmd).finish(),
            Self::TimeoutACommand(cmd) => f.debug_tuple("TimeoutACommand").field(cmd).finish(),
            Self::TimeoutReadBuffer => f.write_str("TimeoutReadBuffer"),
            Self::TimeoutWaitNotBusy => f.write_str("TimeoutWaitNotBusy"),
            Self::Cmd58Error => f.write_str("Cmd58Error"),
            Self::RegisterReadError => f.write_str("RegisterReadError"),
            Self::CrcError(found, expected) => f
                .debug_tuple("CrcError")
                .field(found)
                .field(expected)
                .finish(),
            Self::ReadError => f.write_str("ReadError"),
            Self::WriteError => f.write_str("WriteError"),
            Self::CardNotFound => f.write_str("CardNotFound"),
            Self::BadState => f.write_str("BadState"),
            Self::Unsupported => f.write_str("Unsupported"),
        }
    }
}

pub fn crc7(data: &[u8]) -> u8 {
    let mut crc = 0u8;
    for mut byte in data.iter().copied() {
        for _ in 0..8 {
            crc <<= 1;
            if ((byte & 0x80) ^ (crc & 0x80)) != 0 {
                crc ^= 0x09;
            }
            byte <<= 1;
        }
    }
    (crc << 1) | 1
}

