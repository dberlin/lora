#![warn(missing_docs)]
#![doc = include_str!("../README.md")]

/// The read/write interface between an embedded framework/MCU combination and a LoRa chip
pub(crate) mod interface;
/// Parameters used across the lora crate to support various use cases
pub mod mod_params;
/// Traits implemented externally or internally to support control of LoRa chips
pub mod mod_traits;
/// Specific implementation to support Semtech Sx126x chips
pub mod sx1261_2;
/// Specific implementation to support Semtech Sx127x chips
pub mod sx1276_7_8_9;

use embedded_hal_1::delay::DelayUs;
use interface::*;
use log::trace;
use mod_params::*;
use mod_traits::*;

/// Provides the physical layer API to support LoRa chips
pub struct LoRa<RK> {
    radio_kind: RK,
    radio_mode: RadioMode,
    rx_continuous: bool,
    image_calibrated: bool,
}

impl<RK> LoRa<RK>
where
    RK: RadioKind + 'static,
{
    /// Build and return a new instance of the LoRa physical layer API to control an initialized LoRa radio
    pub fn new(radio_kind: RK, enable_public_network: bool, delay: &mut impl DelayUs) -> Result<Self, RadioError> {
        trace!("LoRa interface creation");
        let mut lora = Self {
            radio_kind,
            radio_mode: RadioMode::Sleep,
            rx_continuous: false,
            image_calibrated: false,
        };
        trace!("Lora init!");
        lora.init(enable_public_network, delay)?;

        Ok(lora)
    }

    /// Create modulation parameters for a communication channel
    pub fn create_modulation_params(
        &mut self,
        spreading_factor: SpreadingFactor,
        bandwidth: Bandwidth,
        coding_rate: CodingRate,
        frequency_in_hz: u32,
    ) -> Result<ModulationParams, RadioError> {
        match self.radio_kind.get_radio_type() {
            RadioType::SX1261 | RadioType::SX1262 | RadioType::STM32WLSX1262 => {
                ModulationParams::new_for_sx1261_2(spreading_factor, bandwidth, coding_rate, frequency_in_hz)
            }
            RadioType::SX1276 | RadioType::SX1277 | RadioType::SX1278 | RadioType::SX1279 => {
                ModulationParams::new_for_sx1276_7_8_9(spreading_factor, bandwidth, coding_rate, frequency_in_hz)
            }
        }
    }

    /// Create packet parameters for a send operation on a communication channel
    pub fn create_tx_packet_params(
        &mut self,
        preamble_length: u16,
        implicit_header: bool,
        crc_on: bool,
        iq_inverted: bool,
        modulation_params: &ModulationParams,
    ) -> Result<PacketParams, RadioError> {
        match self.radio_kind.get_radio_type() {
            RadioType::SX1261 | RadioType::SX1262 | RadioType::STM32WLSX1262 => PacketParams::new_for_sx1261_2(
                preamble_length,
                implicit_header,
                0,
                crc_on,
                iq_inverted,
                modulation_params,
            ),
            RadioType::SX1276 | RadioType::SX1277 | RadioType::SX1278 | RadioType::SX1279 => {
                PacketParams::new_for_sx1276_7_8_9(
                    preamble_length,
                    implicit_header,
                    0,
                    crc_on,
                    iq_inverted,
                    modulation_params,
                )
            }
        }
    }

    /// Create packet parameters for a receive operation on a communication channel
    pub fn create_rx_packet_params(
        &mut self,
        preamble_length: u16,
        implicit_header: bool,
        max_payload_length: u8,
        crc_on: bool,
        iq_inverted: bool,
        modulation_params: &ModulationParams,
    ) -> Result<PacketParams, RadioError> {
        match self.radio_kind.get_radio_type() {
            RadioType::SX1261 | RadioType::SX1262 | RadioType::STM32WLSX1262 => PacketParams::new_for_sx1261_2(
                preamble_length,
                implicit_header,
                max_payload_length,
                crc_on,
                iq_inverted,
                modulation_params,
            ),
            RadioType::SX1276 | RadioType::SX1277 | RadioType::SX1278 | RadioType::SX1279 => {
                PacketParams::new_for_sx1276_7_8_9(
                    preamble_length,
                    implicit_header,
                    max_payload_length,
                    crc_on,
                    iq_inverted,
                    modulation_params,
                )
            }
        }
    }

    /// Initialize a Semtech chip as the radio for LoRa physical layer communications
    pub fn init(&mut self, enable_public_network: bool, delay: &mut impl DelayUs) -> Result<(), RadioError> {
        trace!("Resetting!");
        self.image_calibrated = false;
        self.radio_kind.reset(delay)?;
        trace!("Ensure ready");
        self.radio_kind.ensure_ready(self.radio_mode)?;
        trace!("Init RF switch");
        self.radio_kind.init_rf_switch()?;
        trace!("Set standby");
        self.radio_kind.set_standby()?;
        self.radio_mode = RadioMode::Standby;
        self.rx_continuous = false;
        trace!("Set lora modem");
        self.radio_kind.set_lora_modem(enable_public_network)?;
        trace!("Set oscillator");
        self.radio_kind.set_oscillator()?;
        trace!("set regulator mode");
        self.radio_kind.set_regulator_mode()?;
        trace!("set rx/tx buffer base");
        self.radio_kind.set_tx_rx_buffer_base_address(0, 0)?;
        trace!("set tx power and ramp time");
        self.radio_kind.set_tx_power_and_ramp_time(0, None, false, false)?;
        trace!("set irq params");
        self.radio_kind.set_irq_params(Some(self.radio_mode))?;
        trace!("update retention list");
        self.radio_kind.update_retention_list()
    }

    /// Place the LoRa physical layer in low power mode, using warm start if the Semtech chip supports it
    pub fn sleep(&mut self, delay: &mut impl DelayUs) -> Result<(), RadioError> {
        if self.radio_mode != RadioMode::Sleep {
            self.radio_kind.ensure_ready(self.radio_mode)?;
            let warm_start_enabled = self.radio_kind.set_sleep(delay)?;
            if !warm_start_enabled {
                self.image_calibrated = false;
            }
            self.radio_mode = RadioMode::Sleep;
        }
        Ok(())
    }

    /// Prepare the Semtech chip for a send operation
    pub fn prepare_for_tx(
        &mut self,
        mdltn_params: &ModulationParams,
        output_power: i32,
        tx_boosted_if_possible: bool,
    ) -> Result<(), RadioError> {
        self.rx_continuous = false;
        self.radio_kind.ensure_ready(self.radio_mode)?;
        if self.radio_mode != RadioMode::Standby {
            self.radio_kind.set_standby()?;
            self.radio_mode = RadioMode::Standby;
        }
        self.radio_kind.set_modulation_params(mdltn_params)?;
        self.radio_kind
            .set_tx_power_and_ramp_time(output_power, Some(mdltn_params), tx_boosted_if_possible, true)
    }

    /// Execute a send operation
    pub fn tx(
        &mut self,
        mdltn_params: &ModulationParams,
        tx_pkt_params: &mut PacketParams,
        buffer: &[u8],
        timeout_in_ms: u32,
    ) -> Result<(), RadioError> {
        self.rx_continuous = false;
        self.radio_kind.ensure_ready(self.radio_mode)?;
        if self.radio_mode != RadioMode::Standby {
            self.radio_kind.set_standby()?;
            self.radio_mode = RadioMode::Standby;
        }

        tx_pkt_params.set_payload_length(buffer.len())?;
        self.radio_kind.set_packet_params(tx_pkt_params)?;
        if !self.image_calibrated {
            self.radio_kind.calibrate_image(mdltn_params.frequency_in_hz)?;
            self.image_calibrated = true;
        }
        self.radio_kind.set_channel(mdltn_params.frequency_in_hz)?;
        self.radio_kind.set_payload(buffer)?;
        self.radio_mode = RadioMode::Transmit;
        self.radio_kind.set_irq_params(Some(self.radio_mode))?;
        self.radio_kind.do_tx(timeout_in_ms)?;
        match self.radio_kind.process_irq(self.radio_mode, self.rx_continuous, None) {
            Ok(()) => Ok(()),
            Err(err) => {
                self.radio_kind.ensure_ready(self.radio_mode)?;
                self.radio_kind.set_standby()?;
                self.radio_mode = RadioMode::Standby;
                Err(err)
            }
        }
    }

    /// Prepare the Semtech chip for a receive operation (single shot, continuous, or duty cycled) and initiate the operation
    pub fn prepare_for_rx(
        &mut self,
        mdltn_params: &ModulationParams,
        rx_pkt_params: &PacketParams,
        duty_cycle_params: Option<&DutyCycleParams>,
        rx_continuous: bool,
        rx_boosted_if_supported: bool,
        symbol_timeout: u16,
        rx_timeout_in_ms: u32,
    ) -> Result<(), RadioError> {
        self.rx_continuous = rx_continuous;
        self.radio_kind.ensure_ready(self.radio_mode)?;
        if self.radio_mode != RadioMode::Standby {
            self.radio_kind.set_standby()?;
            self.radio_mode = RadioMode::Standby;
        }

        self.radio_kind.set_modulation_params(mdltn_params)?;
        self.radio_kind.set_packet_params(rx_pkt_params)?;
        if !self.image_calibrated {
            self.radio_kind.calibrate_image(mdltn_params.frequency_in_hz)?;
            self.image_calibrated = true;
        }
        self.radio_kind.set_channel(mdltn_params.frequency_in_hz)?;
        self.radio_mode = match duty_cycle_params {
            Some(&_duty_cycle) => RadioMode::ReceiveDutyCycle,
            None => RadioMode::Receive,
        };
        self.radio_kind.set_irq_params(Some(self.radio_mode))?;
        self.radio_kind.do_rx(
            rx_pkt_params,
            duty_cycle_params,
            self.rx_continuous,
            rx_boosted_if_supported,
            symbol_timeout,
            rx_timeout_in_ms,
        )
    }

    /// Obtain the results of a read operation
    pub fn rx(
        &mut self,
        rx_pkt_params: &PacketParams,
        receiving_buffer: &mut [u8],
    ) -> Result<(u8, PacketStatus), RadioError> {
        match self.radio_kind.process_irq(self.radio_mode, self.rx_continuous, None) {
            Ok(()) => {
                let received_len = self.radio_kind.get_rx_payload(rx_pkt_params, receiving_buffer)?;
                let rx_pkt_status = self.radio_kind.get_rx_packet_status()?;
                Ok((received_len, rx_pkt_status))
            }
            Err(err) => {
                // if in rx continuous mode, allow the caller to determine whether to keep receiving
                if !self.rx_continuous {
                    self.radio_kind.ensure_ready(self.radio_mode)?;
                    self.radio_kind.set_standby()?;
                    self.radio_mode = RadioMode::Standby;
                }
                Err(err)
            }
        }
    }

    /// Prepare the Semtech chip for a channel activity detection operation and initiate the operation
    pub fn prepare_for_cad(
        &mut self,
        mdltn_params: &ModulationParams,
        rx_boosted_if_supported: bool,
    ) -> Result<(), RadioError> {
        self.rx_continuous = false;
        self.radio_kind.ensure_ready(self.radio_mode)?;
        if self.radio_mode != RadioMode::Standby {
            self.radio_kind.set_standby()?;
            self.radio_mode = RadioMode::Standby;
        }

        self.radio_kind.set_modulation_params(mdltn_params)?;
        if !self.image_calibrated {
            self.radio_kind.calibrate_image(mdltn_params.frequency_in_hz)?;
            self.image_calibrated = true;
        }
        self.radio_kind.set_channel(mdltn_params.frequency_in_hz)?;
        self.radio_mode = RadioMode::ChannelActivityDetection;
        self.radio_kind.set_irq_params(Some(self.radio_mode))?;
        self.radio_kind.do_cad(mdltn_params, rx_boosted_if_supported)
    }

    /// Obtain the results of a channel activity detection operation
    pub fn cad(&mut self) -> Result<bool, RadioError> {
        let mut cad_activity_detected = false;
        match self
            .radio_kind
            .process_irq(self.radio_mode, self.rx_continuous, Some(&mut cad_activity_detected))
        {
            Ok(()) => Ok(cad_activity_detected),
            Err(err) => {
                self.radio_kind.ensure_ready(self.radio_mode)?;
                self.radio_kind.set_standby()?;
                self.radio_mode = RadioMode::Standby;
                Err(err)
            }
        }
    }
}
