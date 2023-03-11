use embedded_hal_1::spi::SpiBus;

use crate::mod_params::RadioError;
use crate::mod_params::RadioError::*;
use crate::mod_traits::InterfaceVariant;

pub(crate) struct SpiInterface<SPI, IV> {
    pub(crate) spi: SPI,
    pub(crate) iv: IV,
}

impl<SPI, IV> SpiInterface<SPI, IV>
where
    SPI: SpiBus<u8> + 'static,
    IV: InterfaceVariant + 'static,
{
    pub fn new(spi: SPI, iv: IV) -> Self {
        Self { spi, iv }
    }

    // Write one or more buffers to the radio.
    pub fn write(&mut self, write_buffers: &[&[u8]], is_sleep_command: bool) -> Result<(), RadioError> {
        self.iv.set_nss_low()?;
        for buffer in write_buffers {
            let write_result = self.spi.write(buffer).map_err(|_| SPI);
            let flush_result = self.spi.flush().map_err(|_| SPI);
            if write_result != Ok(()) {
                let _err = self.iv.set_nss_high();
                write_result?;
            } else if flush_result != Ok(()) {
                let _err = self.iv.set_nss_high();
                flush_result?;
            }
        }
        self.iv.set_nss_high()?;

        if !is_sleep_command {
            self.iv.wait_on_busy()?;
        }
        Ok(())
    }

    // Request a read, filling the provided buffer.
    pub fn read(
        &mut self,
        write_buffers: &[&[u8]],
        read_buffer: &mut [u8],
        read_length: Option<u8>,
    ) -> Result<(), RadioError> {
        let mut input = [0u8];

        self.iv.set_nss_low()?;
        for buffer in write_buffers {
            let write_result = self.spi.write(buffer).map_err(|_| SPI);
            let flush_result = self.spi.flush().map_err(|_| SPI);
            if write_result != Ok(()) {
                let _err = self.iv.set_nss_high();
                write_result?;
            } else if flush_result != Ok(()) {
                let _err = self.iv.set_nss_high();
                flush_result?;
            }
        }

        let number_to_read = match read_length {
            Some(len) => len as usize,
            None => read_buffer.len(),
        };

        for item in read_buffer.iter_mut().take(number_to_read) {
            let transfer_result = self.spi.transfer(&mut input, &[0x00]).map_err(|_| SPI);
            let flush_result = self.spi.flush().map_err(|_| SPI);
            if transfer_result != Ok(()) {
                let _err = self.iv.set_nss_high();
                transfer_result?;
            } else if flush_result != Ok(()) {
                let _err = self.iv.set_nss_high();
                flush_result?;
            }
            *item = input[0];
        }
        self.iv.set_nss_high()?;

        self.iv.wait_on_busy()?;

        Ok(())
    }

    // Request a read with status, filling the provided buffer and returning the status.
    pub fn read_with_status(&mut self, write_buffers: &[&[u8]], read_buffer: &mut [u8]) -> Result<u8, RadioError> {
        let mut status = [0u8];
        let mut input = [0u8];

        self.iv.set_nss_low()?;
        for buffer in write_buffers {
            let write_result = self.spi.write(buffer).map_err(|_| SPI);
            let flush_result = self.spi.flush().map_err(|_| SPI);
            if write_result != Ok(()) {
                let _err = self.iv.set_nss_high();
                write_result?;
            } else if flush_result != Ok(()) {
                let _err = self.iv.set_nss_high();
                flush_result?;
            }
        }

        let transfer_result = self.spi.transfer(&mut status, &[0x00]).map_err(|_| SPI);
        let flush_result = self.spi.flush().map_err(|_| SPI);
        if transfer_result != Ok(()) {
            let _err = self.iv.set_nss_high();
            transfer_result?;
        } else if flush_result != Ok(()) {
            let _err = self.iv.set_nss_high();
            flush_result?;
        }

        for item in read_buffer.iter_mut() {
            let transfer_result = self.spi.transfer(&mut input, &[0x00]).map_err(|_| SPI);
            let flush_result = self.spi.flush().map_err(|_| SPI);
            if transfer_result != Ok(()) {
                let _err = self.iv.set_nss_high();
                transfer_result?;
            } else if flush_result != Ok(()) {
                let _err = self.iv.set_nss_high();
                flush_result?;
            }
            *item = input[0];
        }
        self.iv.set_nss_high()?;

        self.iv.wait_on_busy()?;

        Ok(status[0])
    }
}
