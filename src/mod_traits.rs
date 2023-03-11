use embedded_hal_1::delay::DelayUs;

use crate::mod_params::*;

/// Functions implemented for an embedded framework for an MCU/LoRa chip combination
/// to allow this crate to control the LoRa chip.
pub trait InterfaceVariant {
    /// Select the LoRa chip for an operation
    fn set_nss_low(&mut self) -> Result<(), RadioError>;
    /// De-select the LoRa chip after an operation
    fn set_nss_high(&mut self) -> Result<(), RadioError>;
    /// Reset the LoRa chip
    fn reset(&mut self, delay: &mut impl DelayUs) -> Result<(), RadioError>;
    /// Wait for the LoRa chip to become available for an operation
    fn wait_on_busy(&mut self) -> Result<(), RadioError>;
    /// Wait for the LoRa chip to indicate an event has occurred
    fn await_irq(&mut self) -> Result<(), RadioError>;
    /// Enable an antenna used for receive operations, disabling other antennas
    fn enable_rf_switch_rx(&mut self) -> Result<(), RadioError>;
    /// Enable an antenna used for send operations, disabling other antennas
    fn enable_rf_switch_tx(&mut self) -> Result<(), RadioError>;
    /// Disable all antennas
    fn disable_rf_switch(&mut self) -> Result<(), RadioError>;
}

/// Functions implemented for a specific kind of LoRa chip, called internally by the outward facing
/// LoRa physical layer API
pub trait RadioKind {
    /// Get the specific type of the LoRa chip (for example, Sx1262)
    fn get_radio_type(&mut self) -> RadioType;
    /// Reset the loRa chip
    fn reset(&mut self, delay: &mut impl DelayUs) -> Result<(), RadioError>;
    /// Ensure the LoRa chip is in the appropriate state to allow operation requests
    fn ensure_ready(&mut self, mode: RadioMode) -> Result<(), RadioError>;
    /// Perform any necessary antenna initialization
    fn init_rf_switch(&mut self) -> Result<(), RadioError>;
    /// Place the LoRa chip in standby mode
    fn set_standby(&mut self) -> Result<(), RadioError>;
    /// Place the LoRa chip in power-saving mode
    fn set_sleep(&mut self, delay: &mut impl DelayUs) -> Result<bool, RadioError>;
    /// Perform operations to set a multi-protocol chip as a LoRa chip
    fn set_lora_modem(&mut self, enable_public_network: bool) -> Result<(), RadioError>;
    /// Perform operations to set the LoRa chip oscillator
    fn set_oscillator(&mut self) -> Result<(), RadioError>;
    /// Set the LoRa chip voltage regulator mode
    fn set_regulator_mode(&mut self) -> Result<(), RadioError>;
    /// Set the LoRa chip send and receive buffer base addresses
    fn set_tx_rx_buffer_base_address(&mut self, tx_base_addr: usize, rx_base_addr: usize) -> Result<(), RadioError>;
    /// Perform any necessary LoRa chip power setup prior to a send operation
    fn set_tx_power_and_ramp_time(
        &mut self,
        output_power: i32,
        mdltn_params: Option<&ModulationParams>,
        tx_boosted_if_possible: bool,
        is_tx_prep: bool,
    ) -> Result<(), RadioError>;
    /// Update the LoRa chip retention list to support warm starts from sleep
    fn update_retention_list(&mut self) -> Result<(), RadioError>;
    /// Set the LoRa chip modulation parameters prior to using a communication channel
    fn set_modulation_params(&mut self, mdltn_params: &ModulationParams) -> Result<(), RadioError>;
    /// Set the LoRa chip packet parameters prior to sending or receiving packets
    fn set_packet_params(&mut self, pkt_params: &PacketParams) -> Result<(), RadioError>;
    /// Set the LoRa chip to support a given communication channel frequency
    fn calibrate_image(&mut self, frequency_in_hz: u32) -> Result<(), RadioError>;
    /// Set the frequency for a communication channel
    fn set_channel(&mut self, frequency_in_hz: u32) -> Result<(), RadioError>;
    /// Set a payload for a subsequent send operation
    fn set_payload(&mut self, payload: &[u8]) -> Result<(), RadioError>;
    /// Perform a send operation
    fn do_tx(&mut self, timeout_in_ms: u32) -> Result<(), RadioError>;
    /// Set up to perform a receive operation (single-shot, continuous, or duty cycle)
    fn do_rx(
        &mut self,
        rx_pkt_params: &PacketParams,
        duty_cycle_params: Option<&DutyCycleParams>,
        rx_continuous: bool,
        rx_boosted_if_supported: bool,
        symbol_timeout: u16,
        rx_timeout_in_ms: u32,
    ) -> Result<(), RadioError>;
    /// Get an available packet made available as the result of a receive operation
    fn get_rx_payload(&mut self, rx_pkt_params: &PacketParams, receiving_buffer: &mut [u8]) -> Result<u8, RadioError>;
    /// Get the RSSI and SNR for the packet made available as the result of a receive operation
    fn get_rx_packet_status(&mut self) -> Result<PacketStatus, RadioError>;
    /// Perform a channel activity detection operation
    fn do_cad(&mut self, mdltn_params: &ModulationParams, rx_boosted_if_supported: bool) -> Result<(), RadioError>;
    /// Set the LoRa chip to provide notification of specific events based on radio state
    fn set_irq_params(&mut self, radio_mode: Option<RadioMode>) -> Result<(), RadioError>;
    /// Process LoRa chip notifications of events
    fn process_irq(
        &mut self,
        radio_mode: RadioMode,
        rx_continuous: bool,
        cad_activity_detected: Option<&mut bool>,
    ) -> Result<(), RadioError>;
}
