use core::cell::RefCell;

use embedded_hal::{delay::DelayNs, digital::OutputPin};
use embedded_sdmmc::{Block, BlockCount, BlockDevice, BlockIdx};

use crate::{
    SpiTransport,
    proto::{
        ACMD23, ACMD41, CMD0, CMD8, CMD9, CMD12, CMD13, CMD17, CMD18, CMD24, CMD25, CMD55,
        CMD58, CMD59, CardType, DATA_CLOCK_HZ, DATA_RES_ACCEPTED, DATA_RES_MASK,
        DATA_START_BLOCK, Error, INIT_CLOCK_HZ, R1_IDLE_STATE, R1_ILLEGAL_COMMAND,
        R1_READY_STATE, STOP_TRAN_TOKEN, SdSpiOptions, WRITE_MULTIPLE_TOKEN, crc7,
    },
};

pub struct SdSpiCard<SPI, CS, PWR, DELAY, SpiE, PinE>
where
    SPI: SpiTransport<Error = SpiE>,
    CS: OutputPin<Error = PinE>,
    PWR: OutputPin<Error = PinE>,
    DELAY: DelayNs,
{
    inner: RefCell<Inner<SPI, CS, PWR, DELAY, SpiE, PinE>>,
}

pub struct Inner<SPI, CS, PWR, DELAY, SpiE, PinE>
where
    SPI: SpiTransport<Error = SpiE>,
    CS: OutputPin<Error = PinE>,
    PWR: OutputPin<Error = PinE>,
    DELAY: DelayNs,
{
    pub(crate) spi: SPI,
    pub(crate) cs: CS,
    pub(crate) power: PWR,
    pub(crate) delay: DELAY,
    pub(crate) card_type: Option<CardType>,
    pub(crate) options: SdSpiOptions,
}

impl<SPI, CS, PWR, DELAY, SpiE, PinE> SdSpiCard<SPI, CS, PWR, DELAY, SpiE, PinE>
where
    SPI: SpiTransport<Error = SpiE>,
    CS: OutputPin<Error = PinE>,
    PWR: OutputPin<Error = PinE>,
    DELAY: DelayNs,
{
    pub fn new(spi: SPI, cs: CS, power: PWR, delay: DELAY, options: SdSpiOptions) -> Self {
        Self {
            inner: RefCell::new(Inner {
                spi,
                cs,
                power,
                delay,
                card_type: None,
                options,
            }),
        }
    }

    pub fn begin(&self) -> Result<CardType, Error<SpiE, PinE>> {
        let mut inner = self.inner.borrow_mut();
        inner.acquire()
    }

    #[cfg(test)]
    pub fn into_inner_for_test(self) -> Inner<SPI, CS, PWR, DELAY, SpiE, PinE> {
        self.inner.into_inner()
    }

    pub unsafe fn mark_card_as_init(&self, card_type: CardType) {
        self.inner.borrow_mut().card_type = Some(card_type);
    }
}

impl<SPI, CS, PWR, DELAY, SpiE, PinE> BlockDevice for SdSpiCard<SPI, CS, PWR, DELAY, SpiE, PinE>
where
    SPI: SpiTransport<Error = SpiE>,
    CS: OutputPin<Error = PinE>,
    PWR: OutputPin<Error = PinE>,
    DELAY: DelayNs,
    SpiE: core::fmt::Debug,
    PinE: core::fmt::Debug,
{
    type Error = Error<SpiE, PinE>;

    fn read(&self, blocks: &mut [Block], start_block_idx: BlockIdx) -> Result<(), Self::Error> {
        let mut inner = self.inner.borrow_mut();
        inner.ensure_init()?;
        inner.read_blocks(blocks, start_block_idx)
    }

    fn write(&self, _blocks: &[Block], _start_block_idx: BlockIdx) -> Result<(), Self::Error> {
        let mut inner = self.inner.borrow_mut();
        inner.ensure_init()?;
        inner.write_blocks(_blocks, _start_block_idx)
    }

    fn num_blocks(&self) -> Result<BlockCount, Self::Error> {
        let mut inner = self.inner.borrow_mut();
        inner.ensure_init()?;
        inner.num_blocks()
    }
}

impl<SPI, CS, PWR, DELAY, SpiE, PinE> Inner<SPI, CS, PWR, DELAY, SpiE, PinE>
where
    SPI: SpiTransport<Error = SpiE>,
    CS: OutputPin<Error = PinE>,
    PWR: OutputPin<Error = PinE>,
    DELAY: DelayNs,
{
    fn ensure_init(&mut self) -> Result<(), Error<SpiE, PinE>> {
        if self.card_type.is_none() {
            self.acquire()?;
        }
        Ok(())
    }

    fn acquire(&mut self) -> Result<CardType, Error<SpiE, PinE>> {
        self.power.set_high().map_err(Error::Pin)?;
        self.cs.set_high().map_err(Error::Pin)?;
        self.spi.set_clock_hz(INIT_CLOCK_HZ).map_err(Error::Spi)?;
        self.spi.write_bytes(&[0xFF; 10]).map_err(Error::Spi)?;
        self.cs.set_low().map_err(Error::Pin)?;

        let mut timeout = Timeout::new(10_000);

        loop {
            match self.card_command(CMD0, 0) {
                Ok(R1_IDLE_STATE) => break,
                Ok(_) => {}
                Err(Error::TimeoutCommand(CMD0)) => {
                    self.spi.write_bytes(&[0xFF; 0xFF]).map_err(Error::Spi)?;
                }
                Err(err) => return Err(err),
            }
            timeout.delay(&mut self.delay, Error::CardNotFound)?;
        }

        if self.options.use_crc {
            if self.card_command(CMD59, 1)? != R1_IDLE_STATE {
                return Err(Error::CardNotFound);
            }
        }

        let mut card_type = if self.card_command(CMD8, 0x1AA)? & R1_ILLEGAL_COMMAND != 0 {
            Some(CardType::SD1)
        } else {
            let mut response = [0xFF; 4];
            self.spi.transfer_bytes(&mut response).map_err(Error::Spi)?;
            if response[3] != 0xAA {
                return Err(Error::CardNotFound);
            }
            Some(CardType::SD2)
        };
        let arg = if card_type == Some(CardType::SD1) {
            0
        } else {
            0x4000_0000
        };

        let mut timeout = Timeout::new(10_000);
        while self.card_acmd(ACMD41, arg)? != R1_READY_STATE {
            timeout.delay(&mut self.delay, Error::TimeoutACommand(ACMD41))?;
        }

        if card_type == Some(CardType::SD2) {
            if self.card_command(CMD58, 0)? != 0 {
                return Err(Error::Cmd58Error);
            }
            let mut ocr = [0xFF; 4];
            self.spi.transfer_bytes(&mut ocr).map_err(Error::Spi)?;
            if (ocr[0] & 0xC0) == 0xC0 {
                card_type = Some(CardType::SDHC);
            }
        }

        self.cs.set_high().map_err(Error::Pin)?;
        self.spi.set_clock_hz(DATA_CLOCK_HZ).map_err(Error::Spi)?;
        self.card_type = card_type;
        Ok(card_type.unwrap())
    }

    fn read_blocks(
        &mut self,
        blocks: &mut [Block],
        start_block_idx: BlockIdx,
    ) -> Result<(), Error<SpiE, PinE>> {
        self.cs.set_low().map_err(Error::Pin)?;
        let start_idx = self.block_address(start_block_idx)?;
        let result = if blocks.len() == 1 {
            self.card_command(CMD17, start_idx)?;
            self.read_data(&mut blocks[0].contents)
        } else {
            self.card_command(CMD18, start_idx)?;
            for block in blocks.iter_mut() {
                self.read_data(&mut block.contents)?;
            }
            self.card_command(CMD12, 0).map(|_| ())
        };
        self.cs.set_high().map_err(Error::Pin)?;
        result?;
        Ok(())
    }

    fn write_blocks(
        &mut self,
        blocks: &[Block],
        start_block_idx: BlockIdx,
    ) -> Result<(), Error<SpiE, PinE>> {
        self.cs.set_low().map_err(Error::Pin)?;
        let start_idx = self.block_address(start_block_idx)?;
        let result = if blocks.len() == 1 {
            self.card_command(CMD24, start_idx)?;
            self.write_data(DATA_START_BLOCK, &blocks[0].contents)?;
            self.wait_not_busy(Timeout::new(10_000))?;
            if self.card_command(CMD13, 0)? != 0x00 {
                Err(Error::WriteError)
            } else if self.read_byte()? != 0x00 {
                Err(Error::WriteError)
            } else {
                Ok(())
            }
        } else {
            self.card_acmd(ACMD23, blocks.len() as u32)?;
            self.wait_not_busy(Timeout::new(10_000))?;
            self.card_command(CMD25, start_idx)?;
            for block in blocks.iter() {
                self.wait_not_busy(Timeout::new(10_000))?;
                self.write_data(WRITE_MULTIPLE_TOKEN, &block.contents)?;
            }
            self.wait_not_busy(Timeout::new(10_000))?;
            self.write_byte(STOP_TRAN_TOKEN)?;
            Ok(())
        };
        self.cs.set_high().map_err(Error::Pin)?;
        result
    }

    fn num_blocks(&mut self) -> Result<BlockCount, Error<SpiE, PinE>> {
        let blocks = self.read_csd_blocks()?;
        Ok(BlockCount(blocks))
    }

    fn block_address(&self, idx: BlockIdx) -> Result<u32, Error<SpiE, PinE>> {
        match self.card_type {
            Some(CardType::SD1 | CardType::SD2) => Ok(idx.0 * 512),
            Some(CardType::SDHC) => Ok(idx.0),
            None => Err(Error::CardNotFound),
        }
    }

    fn read_csd_blocks(&mut self) -> Result<u32, Error<SpiE, PinE>> {
        self.cs.set_low().map_err(Error::Pin)?;
        let mut csd = [0xFF; 16];
        let blocks = match self.card_type {
            Some(CardType::SD1) => {
                if self.card_command(CMD9, 0)? != 0 {
                    self.cs.set_high().map_err(Error::Pin)?;
                    return Err(Error::RegisterReadError);
                }
                self.read_data(&mut csd)?;
                let mut parsed = embedded_sdmmc::sdcard::proto::CsdV1::new();
                parsed.data.copy_from_slice(&csd);
                Ok(parsed.card_capacity_blocks())
            }
            Some(CardType::SD2 | CardType::SDHC) => {
                if self.card_command(CMD9, 0)? != 0 {
                    self.cs.set_high().map_err(Error::Pin)?;
                    return Err(Error::RegisterReadError);
                }
                self.read_data(&mut csd)?;
                let mut parsed = embedded_sdmmc::sdcard::proto::CsdV2::new();
                parsed.data.copy_from_slice(&csd);
                Ok(parsed.card_capacity_blocks())
            }
            None => Err(Error::CardNotFound),
        }?;
        self.cs.set_high().map_err(Error::Pin)?;
        Ok(blocks)
    }

    fn read_data(&mut self, buffer: &mut [u8]) -> Result<(), Error<SpiE, PinE>> {
        let mut delay = Timeout::new(10_000);
        let token = loop {
            let byte = self.read_byte()?;
            if byte != 0xFF {
                break byte;
            }
            delay.delay(&mut self.delay, Error::TimeoutReadBuffer)?;
        };
        if token != DATA_START_BLOCK {
            return Err(Error::ReadError);
        }

        buffer.fill(0xFF);
        self.spi.transfer_bytes(buffer).map_err(Error::Spi)?;

        let mut crc = [0xFF; 2];
        self.spi.transfer_bytes(&mut crc).map_err(Error::Spi)?;
        if self.options.use_crc {
            let received = u16::from_be_bytes(crc);
            let expected = crc16(buffer);
            if received != expected {
                return Err(Error::CrcError(received, expected));
            }
        }
        Ok(())
    }

    fn card_acmd(&mut self, command: u8, arg: u32) -> Result<u8, Error<SpiE, PinE>> {
        self.card_command(CMD55, 0)?;
        self.card_command(command, arg)
    }

    fn write_data(&mut self, token: u8, buffer: &[u8]) -> Result<(), Error<SpiE, PinE>> {
        self.write_byte(token)?;
        self.spi.write_bytes(buffer).map_err(Error::Spi)?;
        let crc_bytes = if self.options.use_crc {
            crc16(buffer).to_be_bytes()
        } else {
            [0xFF, 0xFF]
        };
        self.spi.write_bytes(&crc_bytes).map_err(Error::Spi)?;

        let status = self.read_byte()?;
        if (status & DATA_RES_MASK) != DATA_RES_ACCEPTED {
            Err(Error::WriteError)
        } else {
            Ok(())
        }
    }

    fn card_command(&mut self, command: u8, arg: u32) -> Result<u8, Error<SpiE, PinE>> {
        if command != CMD0 && command != CMD12 {
            self.wait_not_busy(Timeout::new(10_000))?;
        }

        let mut buf = [
            0x40 | command,
            (arg >> 24) as u8,
            (arg >> 16) as u8,
            (arg >> 8) as u8,
            arg as u8,
            0,
        ];
        buf[5] = crc7(&buf[..5]);
        self.spi.write_bytes(&buf).map_err(Error::Spi)?;

        if command == CMD12 {
            let _ = self.read_byte()?;
        }

        let mut delay = Timeout::new(10_000);
        loop {
            let result = self.read_byte()?;
            if (result & 0x80) == 0 {
                return Ok(result);
            }
            delay.delay(&mut self.delay, Error::TimeoutCommand(command))?;
        }
    }

    fn read_byte(&mut self) -> Result<u8, Error<SpiE, PinE>> {
        self.spi.transfer_byte(0xFF).map_err(Error::Spi)
    }

    fn write_byte(&mut self, byte: u8) -> Result<(), Error<SpiE, PinE>> {
        let _ = self.spi.transfer_byte(byte).map_err(Error::Spi)?;
        Ok(())
    }

    fn wait_not_busy(&mut self, mut delay: Timeout) -> Result<(), Error<SpiE, PinE>> {
        loop {
            if self.read_byte()? == 0xFF {
                return Ok(());
            }
            delay.delay(&mut self.delay, Error::TimeoutWaitNotBusy)?;
        }
    }
}

fn crc16(data: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            if (crc & 0x8000) != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

struct Timeout {
    retries_left: u32,
}

impl Timeout {
    const fn new(retries: u32) -> Self {
        Self {
            retries_left: retries,
        }
    }

    fn delay<D, SpiE, PinE>(
        &mut self,
        delay: &mut D,
        err: Error<SpiE, PinE>,
    ) -> Result<(), Error<SpiE, PinE>>
    where
        D: DelayNs,
    {
        if self.retries_left == 0 {
            Err(err)
        } else {
            delay.delay_us(10);
            self.retries_left -= 1;
            Ok(())
        }
    }
}

#[cfg(test)]
impl<SPI, CS, PWR, DELAY, SpiE, PinE> SdSpiCard<SPI, CS, PWR, DELAY, SpiE, PinE>
where
    SPI: SpiTransport<Error = SpiE>,
    CS: OutputPin<Error = PinE>,
    PWR: OutputPin<Error = PinE>,
    DELAY: DelayNs,
{
    pub fn inner_debug(&self) -> core::cell::RefMut<'_, Inner<SPI, CS, PWR, DELAY, SpiE, PinE>> {
        self.inner.borrow_mut()
    }
}
