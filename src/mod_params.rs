use core::fmt::Debug;

use thiserror::Error;
/// Errors types reported during LoRa physical layer processing
#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, PartialEq, Error)]
#[allow(dead_code, missing_docs)]
pub enum RadioError {
    #[error("SPI error")]
    SPI,
    #[error("NSS error")]
    NSS,
    #[error("Reset error")]
    Reset,
    #[error("RFSwitchRx error")]
    RfSwitchRx,
    #[error("RFSwitchTx error")]
    RfSwitchTx,
    #[error("Busy error")]
    Busy,
    #[error("IRQ error")]
    Irq,
    #[error("DIO1 error")]
    DIO1,
    #[error("Delay error")]
    DelayError,
    #[error("Op error:{0}")]
    OpError(u8),
    #[error("Invalid base address:{0},{1}")]
    InvalidBaseAddress(usize, usize),
    #[error("Payload size unexpected:{0}")]
    PayloadSizeUnexpected(usize),
    #[error("Payload size mismatch: {0} != {1}")]
    PayloadSizeMismatch(usize, usize),
    #[error("Invalid symbol timeout")]
    InvalidSymbolTimeout,
    #[error("Retention list exceeded")]
    RetentionListExceeded,
    #[error("Unavailable spreading factor")]
    UnavailableSpreadingFactor,
    #[error("Unavailable bandwidth")]
    UnavailableBandwidth,
    #[error("Unavailable coding rate")]
    UnavailableCodingRate,
    #[error("Invalid bandwidth for frequency")]
    InvalidBandwidthForFrequency,
    #[error("Invalid Explicit Header request for SF6")]
    InvalidSF6ExplicitHeaderRequest,
    #[error("Invalid output power")]
    InvalidOutputPower,
    #[error("Invalid output power for frequency")]
    InvalidOutputPowerForFrequency,
    #[error("Header error")]
    HeaderError,
    #[error("CRC Error unexpected")]
    CRCErrorUnexpected,
    #[error("CRC Error on receive")]
    CRCErrorOnReceive,
    #[error("Transmit timeout")]
    TransmitTimeout,
    #[error("Receive timeout")]
    ReceiveTimeout,
    #[error("Timeout unexpected")]
    TimeoutUnexpected,
    #[error("Transmit done unexpected")]
    TransmitDoneUnexpected,
    #[error("Receive done unexpected")]
    ReceiveDoneUnexpected,
    #[error("Duty Cycle unsupported")]
    DutyCycleUnsupported,
    #[error("RX Continuous Duty Cycle unsupported")]
    DutyCycleRxContinuousUnsupported,
    #[error("CAD unexpected")]
    CADUnexpected,
}

/// Status for a received packet
#[derive(Clone, Copy)]
#[allow(missing_docs)]
pub struct PacketStatus {
    pub rssi: i16,
    pub snr: i16,
}

/// LoRa chips supported by this crate
#[derive(Clone, Copy, PartialEq)]
#[allow(missing_docs)]
pub enum RadioType {
    SX1261,
    SX1262,
    STM32WLSX1262,
    SX1276,
    SX1277,
    SX1278,
    SX1279,
}

/// The state of the radio
#[derive(Clone, Copy, PartialEq)]
#[allow(missing_docs)]
pub enum RadioMode {
    Sleep,                    // sleep mode
    Standby,                  // standby mode
    FrequencySynthesis,       // frequency synthesis mode
    Transmit,                 // transmit mode
    Receive,                  // receive mode
    ReceiveDutyCycle,         // receive duty cycle mode
    ChannelActivityDetection, // channel activity detection mode
}

/// Valid spreading factors for one or more LoRa chips supported by this crate
#[derive(Clone, Copy, PartialEq)]
#[allow(missing_docs)]
pub enum SpreadingFactor {
    _5,
    _6,
    _7,
    _8,
    _9,
    _10,
    _11,
    _12,
}

/// Valid bandwidths for one or more LoRa chips supported by this crate
#[derive(Clone, Copy, PartialEq)]
#[allow(missing_docs)]
pub enum Bandwidth {
    _7KHz,
    _10KHz,
    _15KHz,
    _20KHz,
    _31KHz,
    _41KHz,
    _62KHz,
    _125KHz,
    _250KHz,
    _500KHz,
}

impl Bandwidth {
    /// Convert to Hertz
    pub fn value_in_hz(self) -> u32 {
        match self {
            Bandwidth::_7KHz => 7810u32,
            Bandwidth::_10KHz => 10420u32,
            Bandwidth::_15KHz => 15630u32,
            Bandwidth::_20KHz => 20830u32,
            Bandwidth::_31KHz => 31250u32,
            Bandwidth::_41KHz => 41670u32,
            Bandwidth::_62KHz => 62500u32,
            Bandwidth::_125KHz => 125000u32,
            Bandwidth::_250KHz => 250000u32,
            Bandwidth::_500KHz => 500000u32,
        }
    }
}

/// Valid coding rates for one or more LoRa chips supported by this crate
#[derive(Clone, Copy)]
#[allow(missing_docs)]
pub enum CodingRate {
    _4_5,
    _4_6,
    _4_7,
    _4_8,
}

/// Modulation parameters for a send and/or receive communication channel
pub struct ModulationParams {
    pub(crate) spreading_factor: SpreadingFactor,
    pub(crate) bandwidth: Bandwidth,
    pub(crate) coding_rate: CodingRate,
    pub(crate) low_data_rate_optimize: u8,
    pub(crate) frequency_in_hz: u32,
}

/// Packet parameters for a send or receive communication channel
pub struct PacketParams {
    pub(crate) preamble_length: u16,  // number of LoRa symbols in the preamble
    pub(crate) implicit_header: bool, // if the header is explicit, it will be transmitted in the LoRa packet, but is not transmitted if the header is implicit (known fixed length)
    pub(crate) payload_length: u8,
    pub(crate) crc_on: bool,
    pub(crate) iq_inverted: bool,
}

impl PacketParams {
    pub(crate) fn set_payload_length(&mut self, payload_length: usize) -> Result<(), RadioError> {
        if payload_length > 255 {
            return Err(RadioError::PayloadSizeUnexpected(payload_length));
        }
        self.payload_length = payload_length as u8;
        Ok(())
    }
}

/// Receive duty cycle parameters
#[derive(Clone, Copy)]
#[allow(missing_docs)]
pub struct DutyCycleParams {
    pub rx_time: u32,    // receive interval
    pub sleep_time: u32, // sleep interval
}
