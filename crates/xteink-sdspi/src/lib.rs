#![cfg_attr(not(test), no_std)]

pub mod card;
pub mod proto;

pub trait SpiTransport {
    type Error;

    fn set_clock_hz(&mut self, hz: u32) -> Result<(), Self::Error>;
    fn transfer_byte(&mut self, byte: u8) -> Result<u8, Self::Error>;

    fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        for &byte in bytes {
            let _ = self.transfer_byte(byte)?;
        }
        Ok(())
    }

    fn transfer_bytes(&mut self, bytes: &mut [u8]) -> Result<(), Self::Error> {
        for byte in bytes {
            *byte = self.transfer_byte(*byte)?;
        }
        Ok(())
    }
}

pub use card::SdSpiCard;
pub use proto::SdSpiOptions;
pub use proto::{CardType, Error};

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use core::convert::Infallible;
    use embedded_hal::{delay::DelayNs, digital::{ErrorType, OutputPin}};
    use embedded_sdmmc::{Block, BlockDevice, BlockIdx};
    use std::collections::VecDeque;

    #[derive(Default)]
    struct MockSpi {
        speeds: Vec<u32>,
        tx: Vec<u8>,
        rx: VecDeque<u8>,
    }

    impl SpiTransport for MockSpi {
        type Error = Infallible;

        fn set_clock_hz(&mut self, hz: u32) -> Result<(), Self::Error> {
            self.speeds.push(hz);
            Ok(())
        }

        fn transfer_byte(&mut self, byte: u8) -> Result<u8, Self::Error> {
            self.tx.push(byte);
            Ok(self.rx.pop_front().unwrap_or(0xFF))
        }
    }

    #[derive(Default)]
    struct MockPin {
        history: Vec<bool>,
    }

    impl ErrorType for MockPin {
        type Error = Infallible;
    }

    impl OutputPin for MockPin {
        fn set_low(&mut self) -> Result<(), Self::Error> {
            self.history.push(false);
            Ok(())
        }

        fn set_high(&mut self) -> Result<(), Self::Error> {
            self.history.push(true);
            Ok(())
        }
    }

    #[derive(Default)]
    struct MockDelay;

    impl DelayNs for MockDelay {
        fn delay_ns(&mut self, _ns: u32) {}
    }

    #[test]
    fn acquire_sequence_matches_sd_spi_boot_flow() {
        let mut bytes = vec![0xFF; 57];
        bytes[16] = 0x01; // CMD0 -> idle
        bytes[24] = 0x05; // CMD8 -> illegal response bit set
        bytes[32] = 0x01; // CMD55 -> in idle
        bytes[40] = 0x01; // CMD41 -> in idle
        bytes[48] = 0x01; // CMD55 -> in idle
        bytes[56] = 0x00; // CMD41 -> ready
        let spi = MockSpi {
            rx: VecDeque::from(bytes),
            ..Default::default()
        };
        let card = SdSpiCard::new(
            spi,
            MockPin::default(),
            MockPin::default(),
            MockDelay,
            SdSpiOptions { use_crc: false },
        );

        let result = card.begin();
        assert!(result.is_ok());

        let inner = card.into_inner_for_test();
        assert_eq!(inner.spi.speeds, vec![400_000, 40_000_000]);
        assert!(inner.cs.history.contains(&false));
        assert!(inner.cs.history.contains(&true));
        assert!(inner.power.history.contains(&true));
    }

    #[test]
    fn single_block_read_uses_cmd17_and_returns_512_bytes() {
        let mut rx = VecDeque::new();
        rx.extend([0xFF; 7]);
        rx.push_back(0x00);
        rx.push_back(0xFF);
        rx.push_back(0xFE);
        rx.extend(0u8..=255);
        rx.extend(0u8..=255);
        rx.push_back(0x12);
        rx.push_back(0x34);

        let spi = MockSpi {
            rx,
            ..Default::default()
        };
        let card = SdSpiCard::new(
            spi,
            MockPin::default(),
            MockPin::default(),
            MockDelay,
            SdSpiOptions { use_crc: false },
        );
        unsafe {
            card.mark_card_as_init(CardType::SDHC);
        }

        let mut blocks = [Block::new()];
        let result = card.read(&mut blocks, BlockIdx(7));
        assert!(result.is_ok());
        assert_eq!(blocks[0].contents[0], 0);
        assert_eq!(blocks[0].contents[255], 255);
        assert_eq!(blocks[0].contents[256], 0);
        assert_eq!(blocks[0].contents[511], 255);

        let inner = card.into_inner_for_test();
        assert!(inner.spi.tx.iter().any(|&b| b == 0x51));
        assert!(inner.cs.history.contains(&false));
        assert!(inner.cs.history.contains(&true));
    }
}
