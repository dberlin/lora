mod radio_kind_params;

use defmt::info;
use embedded_hal_1::delay::DelayUs;
use embedded_hal_1::spi::*;
use radio_kind_params::*;

use crate::mod_params::RadioError::*;
use crate::mod_params::*;
use crate::{InterfaceVariant, RadioKind, SpiInterface};

// Syncwords for public and private networks
const LORA_MAC_PUBLIC_SYNCWORD: u16 = 0x3444; // corresponds to sx127x 0x34
const LORA_MAC_PRIVATE_SYNCWORD: u16 = 0x1424; // corresponds to sx127x 0x12

// Maximum number of registers that can be added to the retention list
const MAX_NUMBER_REGS_IN_RETENTION: u8 = 4;

// Internal frequency of the radio
const SX126X_XTAL_FREQ: u32 = 32000000;

// Scaling factor used to perform fixed-point operations
const SX126X_PLL_STEP_SHIFT_AMOUNT: u32 = 14;

// PLL step - scaled with SX126X_PLL_STEP_SHIFT_AMOUNT
const SX126X_PLL_STEP_SCALED: u32 = SX126X_XTAL_FREQ >> (25 - SX126X_PLL_STEP_SHIFT_AMOUNT);

// Maximum value for parameter symbNum
const SX126X_MAX_LORA_SYMB_NUM_TIMEOUT: u8 = 248;

// Time required for the TCXO to wakeup [ms].
const BRD_TCXO_WAKEUP_TIME: u32 = 10;

impl ModulationParams {
    /// Create modulation parameters specific to the LoRa chip kind and type
    pub fn new_for_sx1261_2(
        spreading_factor: SpreadingFactor,
        bandwidth: Bandwidth,
        coding_rate: CodingRate,
        frequency_in_hz: u32,
    ) -> Result<Self, RadioError> {
        // Parameter validation
        spreading_factor_value(spreading_factor)?;
        bandwidth_value(bandwidth)?;
        coding_rate_value(coding_rate)?;
        if ((bandwidth == Bandwidth::_250KHz) || (bandwidth == Bandwidth::_500KHz)) && (frequency_in_hz < 400_000_000) {
            return Err(RadioError::InvalidBandwidthForFrequency);
        }

        let mut low_data_rate_optimize = 0x00u8;
        if (((spreading_factor == SpreadingFactor::_11) || (spreading_factor == SpreadingFactor::_12))
            && (bandwidth == Bandwidth::_125KHz))
            || ((spreading_factor == SpreadingFactor::_12) && (bandwidth == Bandwidth::_250KHz))
        {
            low_data_rate_optimize = 0x01u8;
        }
        Ok(Self {
            spreading_factor,
            bandwidth,
            coding_rate,
            low_data_rate_optimize,
            frequency_in_hz,
        })
    }
}

impl PacketParams {
    /// Create packet parameters specific to the LoRa chip kind and type
    pub fn new_for_sx1261_2(
        mut preamble_length: u16,
        implicit_header: bool,
        payload_length: u8,
        crc_on: bool,
        iq_inverted: bool,
        modulation_params: &ModulationParams,
    ) -> Result<Self, RadioError> {
        if ((modulation_params.spreading_factor == SpreadingFactor::_5)
            || (modulation_params.spreading_factor == SpreadingFactor::_6))
            && (preamble_length < 12)
        {
            preamble_length = 12;
        }

        Ok(Self {
            preamble_length,
            implicit_header,
            payload_length,
            crc_on,
            iq_inverted,
        })
    }
}

/// Base for the RadioKind implementation for the LoRa chip kind and type
pub struct SX1261_2<SPI, IV> {
    radio_type: RadioType,
    intf: SpiInterface<SPI, IV>,
}

impl<SPI, IV> SX1261_2<SPI, IV>
where
    SPI: SpiBus<u8> + 'static,
    IV: InterfaceVariant + 'static,
{
    /// Create an instance of the RadioKind implementation for the LoRa chip kind and type
    pub fn new(radio_type: RadioType, spi: SPI, iv: IV) -> Self {
        let intf = SpiInterface::new(spi, iv);
        Self { radio_type, intf }
    }

    // Utility functions

    fn add_register_to_retention_list(&mut self, register: Register) -> Result<(), RadioError> {
        let mut buffer = [0x00u8; (1 + (2 * MAX_NUMBER_REGS_IN_RETENTION)) as usize];

        // Read the address and registers already added to the list
        self.intf.read(
            &[&[
                OpCode::ReadRegister.value(),
                Register::RetentionList.addr1(),
                Register::RetentionList.addr2(),
                0x00u8,
            ]],
            &mut buffer,
            None,
        )?;

        let number_of_registers = buffer[0];
        for i in 0..number_of_registers {
            if register.addr1() == buffer[(1 + (2 * i)) as usize] && register.addr2() == buffer[(2 + (2 * i)) as usize]
            {
                return Ok(()); // register already in list
            }
        }

        if number_of_registers < MAX_NUMBER_REGS_IN_RETENTION {
            buffer[0] += 1; // increment number of registers

            buffer[(1 + (2 * number_of_registers)) as usize] = register.addr1();
            buffer[(2 + (2 * number_of_registers)) as usize] = register.addr2();

            let register = [
                OpCode::WriteRegister.value(),
                Register::RetentionList.addr1(),
                Register::RetentionList.addr2(),
            ];
            self.intf.write(&[&register, &buffer], false)
        } else {
            Err(RadioError::RetentionListExceeded)
        }
    }

    // Set the number of symbols the radio will wait to validate a reception
    fn set_lora_symbol_num_timeout(&mut self, symbol_num: u16) -> Result<(), RadioError> {
        let mut exp = 0u8;
        let mut reg;
        let mut mant = ((core::cmp::min(symbol_num, SX126X_MAX_LORA_SYMB_NUM_TIMEOUT as u16) as u8) + 1) >> 1;
        while mant > 31 {
            mant = (mant + 3) >> 2;
            exp += 1;
        }
        reg = mant << ((2 * exp) + 1);

        let op_code_and_timeout = [OpCode::SetLoRaSymbTimeout.value(), reg];
        self.intf.write(&[&op_code_and_timeout], false)?;

        if symbol_num != 0 {
            reg = exp + (mant << 3);
            let register_and_timeout = [
                OpCode::WriteRegister.value(),
                Register::SynchTimeout.addr1(),
                Register::SynchTimeout.addr2(),
                reg,
            ];
            self.intf.write(&[&register_and_timeout], false)?;
        }

        Ok(())
    }

    fn set_pa_config(&mut self, pa_duty_cycle: u8, hp_max: u8, device_sel: u8, pa_lut: u8) -> Result<(), RadioError> {
        let op_code_and_pa_config = [OpCode::SetPAConfig.value(), pa_duty_cycle, hp_max, device_sel, pa_lut];
        self.intf.write(&[&op_code_and_pa_config], false)
    }

    fn timeout_1(timeout: u32) -> u8 {
        ((timeout >> 16) & 0xFF) as u8
    }
    fn timeout_2(timeout: u32) -> u8 {
        ((timeout >> 8) & 0xFF) as u8
    }
    fn timeout_3(timeout: u32) -> u8 {
        (timeout & 0xFF) as u8
    }

    fn convert_freq_in_hz_to_pll_step(freq_in_hz: u32) -> u32 {
        // Get integer and fractional parts of the frequency computed with a PLL step scaled value
        let steps_int = freq_in_hz / SX126X_PLL_STEP_SCALED;
        let steps_frac = freq_in_hz - (steps_int * SX126X_PLL_STEP_SCALED);

        (steps_int << SX126X_PLL_STEP_SHIFT_AMOUNT)
            + (((steps_frac << SX126X_PLL_STEP_SHIFT_AMOUNT) + (SX126X_PLL_STEP_SCALED >> 1)) / SX126X_PLL_STEP_SCALED)
    }
}

impl<SPI, IV> RadioKind for SX1261_2<SPI, IV>
where
    SPI: SpiBus<u8> + 'static,
    IV: InterfaceVariant + 'static,
{
    fn get_radio_type(&mut self) -> RadioType {
        self.radio_type
    }

    fn reset(&mut self, delay: &mut impl DelayUs) -> Result<(), RadioError> {
        self.intf.iv.reset(delay)
    }

    // Wakeup the radio if it is in Sleep or ReceiveDutyCycle mode; otherwise, ensure it is not busy.
    fn ensure_ready(&mut self, mode: RadioMode) -> Result<(), RadioError> {
        if mode == RadioMode::Sleep || mode == RadioMode::ReceiveDutyCycle {
            let op_code_and_null = [OpCode::GetStatus.value(), 0x00u8];
            self.intf.write(&[&op_code_and_null], false)?;
        } else {
            self.intf.iv.wait_on_busy()?;
        }
        Ok(())
    }

    // Use DIO2 to control an RF Switch, depending on the radio type.
    fn init_rf_switch(&mut self) -> Result<(), RadioError> {
        if self.get_radio_type() != RadioType::STM32WLSX1262 {
            let op_code_and_indicator = [OpCode::SetRFSwitchMode.value(), true as u8];
            self.intf.write(&[&op_code_and_indicator], false)?;
        }
        Ok(())
    }

    // Use standby mode RC (not XOSC).
    fn set_standby(&mut self) -> Result<(), RadioError> {
        let op_code_and_standby_mode = [OpCode::SetStandby.value(), StandbyMode::RC.value()];
        self.intf.write(&[&op_code_and_standby_mode], false)?;
        self.intf.iv.disable_rf_switch()
    }

    fn set_sleep(&mut self, delay: &mut impl DelayUs) -> Result<bool, RadioError> {
        self.intf.iv.disable_rf_switch()?;
        let sleep_params = SleepParams {
            wakeup_rtc: false,
            reset: false,
            warm_start: true,
        };
        let op_code_and_sleep_params = [OpCode::SetSleep.value(), sleep_params.value()];
        self.intf.write(&[&op_code_and_sleep_params], true)?;
        delay.delay_ms(2).map_err(|_| DelayError)?;

        Ok(sleep_params.warm_start) // indicate if warm start enabled
    }

    /// Configure the radio for LoRa and a public/private network.
    fn set_lora_modem(&mut self, enable_public_network: bool) -> Result<(), RadioError> {
        let op_code_and_packet_type = [OpCode::SetPacketType.value(), PacketType::LoRa.value()];
        self.intf.write(&[&op_code_and_packet_type], false)?;
        if enable_public_network {
            let register_and_syncword = [
                OpCode::WriteRegister.value(),
                Register::LoRaSyncword.addr1(),
                Register::LoRaSyncword.addr2(),
                ((LORA_MAC_PUBLIC_SYNCWORD >> 8) & 0xFF) as u8,
                (LORA_MAC_PUBLIC_SYNCWORD & 0xFF) as u8,
            ];
            self.intf.write(&[&register_and_syncword], false)?;
        } else {
            let register_and_syncword = [
                OpCode::WriteRegister.value(),
                Register::LoRaSyncword.addr1(),
                Register::LoRaSyncword.addr2(),
                ((LORA_MAC_PRIVATE_SYNCWORD >> 8) & 0xFF) as u8,
                (LORA_MAC_PRIVATE_SYNCWORD & 0xFF) as u8,
            ];
            self.intf.write(&[&register_and_syncword], false)?;
        }

        Ok(())
    }

    fn set_oscillator(&mut self) -> Result<(), RadioError> {
        let voltage = TcxoCtrlVoltage::Ctrl1V7.value() & 0x07; // voltage used to control the TCXO on/off from DIO3
        let timeout = BRD_TCXO_WAKEUP_TIME << 6; // duration allowed for TCXO to reach 32MHz
        let op_code_and_tcxo_control = [
            OpCode::SetTCXOMode.value(),
            voltage,
            Self::timeout_1(timeout),
            Self::timeout_2(timeout),
            Self::timeout_3(timeout),
        ];
        self.intf.write(&[&op_code_and_tcxo_control], false)
    }

    // Set the power regulators operating mode to DC_DC.  Using only LDO implies that the Rx/Tx current is doubled.
    fn set_regulator_mode(&mut self) -> Result<(), RadioError> {
        let op_code_and_regulator_mode = [OpCode::SetRegulatorMode.value(), RegulatorMode::UseDCDC.value()];
        self.intf.write(&[&op_code_and_regulator_mode], false)
    }

    fn set_tx_rx_buffer_base_address(&mut self, tx_base_addr: usize, rx_base_addr: usize) -> Result<(), RadioError> {
        if tx_base_addr > 255 || rx_base_addr > 255 {
            return Err(RadioError::InvalidBaseAddress(tx_base_addr, rx_base_addr));
        }
        let op_code_and_base_addrs = [
            OpCode::SetBufferBaseAddress.value(),
            tx_base_addr as u8,
            rx_base_addr as u8,
        ];
        self.intf.write(&[&op_code_and_base_addrs], false)
    }

    // Set parameters associated with power for a send operation. Currently, over current protection (OCP) uses the default set automatically after set_pa_config()
    //   output_power            desired RF output power (dBm)
    //   mdltn_params            needed for a power vs channel frequency validation
    //   tx_boosted_if_possible  not pertinent for sx126x
    //   is_tx_prep              indicates which ramp up time to use
    fn set_tx_power_and_ramp_time(
        &mut self,
        output_power: i32,
        mdltn_params: Option<&ModulationParams>,
        _tx_boosted_if_possible: bool,
        is_tx_prep: bool,
    ) -> Result<(), RadioError> {
        let tx_params_power;
        let ramp_time = match is_tx_prep {
            true => RampTime::Ramp40Us,   // for instance, prior to TX or CAD
            false => RampTime::Ramp200Us, // for instance, on initialization
        };

        if self.radio_type == RadioType::SX1261 {
            if !(-17..=15).contains(&output_power) {
                return Err(RadioError::InvalidOutputPower);
            }
            if output_power == 15 {
                if let Some(m_p) = mdltn_params {
                    if m_p.frequency_in_hz < 400_000_000 {
                        return Err(RadioError::InvalidOutputPowerForFrequency);
                    }
                }
            }

            match output_power {
                15 => {
                    self.set_pa_config(0x06, 0x00, 0x01, 0x01)?;
                    tx_params_power = 14;
                }
                14 => {
                    self.set_pa_config(0x04, 0x00, 0x01, 0x01)?;
                    tx_params_power = 14;
                }
                10 => {
                    self.set_pa_config(0x01, 0x00, 0x01, 0x01)?;
                    tx_params_power = 14;
                }
                _ => {
                    self.set_pa_config(0x04, 0x00, 0x01, 0x01)?;
                    tx_params_power = output_power as u8;
                }
            }
        } else {
            if !(-9..=22).contains(&output_power) {
                return Err(RadioError::InvalidOutputPower);
            }
            // Provide better resistance of the SX1262 Tx to antenna mismatch (see DS_SX1261-2_V1.2 datasheet chapter 15.2)
            let mut tx_clamp_cfg = [0x00u8];
            self.intf.read(
                &[&[
                    OpCode::ReadRegister.value(),
                    Register::TxClampCfg.addr1(),
                    Register::TxClampCfg.addr2(),
                    0x00u8,
                ]],
                &mut tx_clamp_cfg,
                None,
            )?;
            tx_clamp_cfg[0] |= 0x0F << 1;
            let register_and_tx_clamp_cfg = [
                OpCode::WriteRegister.value(),
                Register::TxClampCfg.addr1(),
                Register::TxClampCfg.addr2(),
                tx_clamp_cfg[0],
            ];
            self.intf.write(&[&register_and_tx_clamp_cfg], false)?;

            match output_power {
                22 => {
                    self.set_pa_config(0x04, 0x07, 0x00, 0x01)?;
                    tx_params_power = 22;
                }
                20 => {
                    self.set_pa_config(0x03, 0x05, 0x00, 0x01)?;
                    tx_params_power = 22;
                }
                17 => {
                    self.set_pa_config(0x02, 0x03, 0x00, 0x01)?;
                    tx_params_power = 22;
                }
                14 => {
                    self.set_pa_config(0x02, 0x02, 0x00, 0x01)?;
                    tx_params_power = 22;
                }
                _ => {
                    self.set_pa_config(0x04, 0x07, 0x00, 0x01)?;
                    tx_params_power = output_power as u8;
                }
            }
        }

        let op_code_and_tx_params = [OpCode::SetTxParams.value(), tx_params_power, ramp_time.value()];
        self.intf.write(&[&op_code_and_tx_params], false)
    }

    fn update_retention_list(&mut self) -> Result<(), RadioError> {
        self.add_register_to_retention_list(Register::RxGain)?;
        self.add_register_to_retention_list(Register::TxModulation)
    }

    fn set_modulation_params(&mut self, mdltn_params: &ModulationParams) -> Result<(), RadioError> {
        let spreading_factor_val = spreading_factor_value(mdltn_params.spreading_factor)?;
        let bandwidth_val = bandwidth_value(mdltn_params.bandwidth)?;
        let coding_rate_val = coding_rate_value(mdltn_params.coding_rate)?;
        let op_code_and_mod_params = [
            OpCode::SetModulationParams.value(),
            spreading_factor_val,
            bandwidth_val,
            coding_rate_val,
            mdltn_params.low_data_rate_optimize,
        ];
        self.intf.write(&[&op_code_and_mod_params], false)?;

        // Handle modulation quality with the 500 kHz LoRa bandwidth (see DS_SX1261-2_V1.2 datasheet chapter 15.1)
        let mut tx_mod = [0x00u8];
        self.intf.read(
            &[&[
                OpCode::ReadRegister.value(),
                Register::TxModulation.addr1(),
                Register::TxModulation.addr2(),
                0x00u8,
            ]],
            &mut tx_mod,
            None,
        )?;
        if mdltn_params.bandwidth == Bandwidth::_500KHz {
            let register_and_tx_mod_update = [
                OpCode::WriteRegister.value(),
                Register::TxModulation.addr1(),
                Register::TxModulation.addr2(),
                tx_mod[0] & (!(1 << 2)),
            ];
            self.intf.write(&[&register_and_tx_mod_update], false)
        } else {
            let register_and_tx_mod_update = [
                OpCode::WriteRegister.value(),
                Register::TxModulation.addr1(),
                Register::TxModulation.addr2(),
                tx_mod[0] | (1 << 2),
            ];
            self.intf.write(&[&register_and_tx_mod_update], false)
        }
    }

    fn set_packet_params(&mut self, pkt_params: &PacketParams) -> Result<(), RadioError> {
        let op_code_and_pkt_params = [
            OpCode::SetPacketParams.value(),
            ((pkt_params.preamble_length >> 8) & 0xFF) as u8,
            (pkt_params.preamble_length & 0xFF) as u8,
            pkt_params.implicit_header as u8,
            pkt_params.payload_length,
            pkt_params.crc_on as u8,
            pkt_params.iq_inverted as u8,
        ];
        self.intf.write(&[&op_code_and_pkt_params], false)
    }

    // Calibrate the image rejection based on the given frequency
    fn calibrate_image(&mut self, frequency_in_hz: u32) -> Result<(), RadioError> {
        let mut cal_freq = [0x00u8, 0x00u8];

        if frequency_in_hz > 900000000 {
            cal_freq[0] = 0xE1;
            cal_freq[1] = 0xE9;
        } else if frequency_in_hz > 850000000 {
            cal_freq[0] = 0xD7;
            cal_freq[1] = 0xDB;
        } else if frequency_in_hz > 770000000 {
            cal_freq[0] = 0xC1;
            cal_freq[1] = 0xC5;
        } else if frequency_in_hz > 460000000 {
            cal_freq[0] = 0x75;
            cal_freq[1] = 0x81;
        } else if frequency_in_hz > 425000000 {
            cal_freq[0] = 0x6B;
            cal_freq[1] = 0x6F;
        }

        let op_code_and_cal_freq = [OpCode::CalibrateImage.value(), cal_freq[0], cal_freq[1]];
        self.intf.write(&[&op_code_and_cal_freq], false)
    }

    fn set_channel(&mut self, frequency_in_hz: u32) -> Result<(), RadioError> {
        let freq_in_pll_steps = Self::convert_freq_in_hz_to_pll_step(frequency_in_hz);
        let op_code_and_pll_steps = [
            OpCode::SetRFFrequency.value(),
            ((freq_in_pll_steps >> 24) & 0xFF) as u8,
            ((freq_in_pll_steps >> 16) & 0xFF) as u8,
            ((freq_in_pll_steps >> 8) & 0xFF) as u8,
            (freq_in_pll_steps & 0xFF) as u8,
        ];
        self.intf.write(&[&op_code_and_pll_steps], false)
    }

    fn set_payload(&mut self, payload: &[u8]) -> Result<(), RadioError> {
        let op_code_and_offset = [OpCode::WriteBuffer.value(), 0x00u8];
        self.intf.write(&[&op_code_and_offset, payload], false)
    }

    fn do_tx(&mut self, timeout_in_ms: u32) -> Result<(), RadioError> {
        self.intf.iv.enable_rf_switch_tx()?;

        let op_code_and_timeout = [
            OpCode::SetTx.value(),
            Self::timeout_1(timeout_in_ms),
            Self::timeout_2(timeout_in_ms),
            Self::timeout_3(timeout_in_ms),
        ];
        self.intf.write(&[&op_code_and_timeout], false)
    }

    fn do_rx(
        &mut self,
        rx_pkt_params: &PacketParams,
        duty_cycle_params: Option<&DutyCycleParams>,
        rx_continuous: bool,
        rx_boosted_if_supported: bool,
        symbol_timeout: u16,
        rx_timeout_in_ms: u32,
    ) -> Result<(), RadioError> {
        let mut symbol_timeout_final = symbol_timeout;
        let mut rx_timeout_in_ms_final = rx_timeout_in_ms << 6;

        if let Some(&_duty_cycle) = duty_cycle_params {
            if rx_continuous {
                return Err(RadioError::DutyCycleRxContinuousUnsupported);
            } else {
                symbol_timeout_final = 0;
            }
        }

        self.intf.iv.enable_rf_switch_rx()?;

        if rx_continuous {
            symbol_timeout_final = 0;
            rx_timeout_in_ms_final = 0x00ffffffu32;
        }

        let mut rx_gain_final = 0x94u8;
        // if Rx boosted, set max LNA gain, increase current by ~2mA for around ~3dB in sensitivity
        if rx_boosted_if_supported {
            rx_gain_final = 0x96u8;
        }

        // stop the Rx timer on header/syncword detection rather than preamble detection
        let op_code_and_false_flag = [OpCode::SetStopRxTimerOnPreamble.value(), 0x00u8];
        self.intf.write(&[&op_code_and_false_flag], false)?;

        self.set_lora_symbol_num_timeout(symbol_timeout_final)?;

        // Optimize the Inverted IQ Operation (see DS_SX1261-2_V1.2 datasheet chapter 15.4)
        let mut iq_polarity = [0x00u8];
        self.intf.read(
            &[&[
                OpCode::ReadRegister.value(),
                Register::IQPolarity.addr1(),
                Register::IQPolarity.addr2(),
                0x00u8,
            ]],
            &mut iq_polarity,
            None,
        )?;
        if rx_pkt_params.iq_inverted {
            let register_and_iq_polarity = [
                OpCode::WriteRegister.value(),
                Register::IQPolarity.addr1(),
                Register::IQPolarity.addr2(),
                iq_polarity[0] & (!(1 << 2)),
            ];
            self.intf.write(&[&register_and_iq_polarity], false)?;
        } else {
            let register_and_iq_polarity = [
                OpCode::WriteRegister.value(),
                Register::IQPolarity.addr1(),
                Register::IQPolarity.addr2(),
                iq_polarity[0] | (1 << 2),
            ];
            self.intf.write(&[&register_and_iq_polarity], false)?;
        }

        let register_and_rx_gain = [
            OpCode::WriteRegister.value(),
            Register::RxGain.addr1(),
            Register::RxGain.addr2(),
            rx_gain_final,
        ];
        self.intf.write(&[&register_and_rx_gain], false)?;

        match duty_cycle_params {
            Some(&duty_cycle) => {
                let op_code_and_duty_cycle = [
                    OpCode::SetRxDutyCycle.value(),
                    Self::timeout_1(duty_cycle.rx_time),
                    Self::timeout_2(duty_cycle.rx_time),
                    Self::timeout_3(duty_cycle.rx_time),
                    Self::timeout_1(duty_cycle.sleep_time),
                    Self::timeout_2(duty_cycle.sleep_time),
                    Self::timeout_3(duty_cycle.sleep_time),
                ];
                self.intf.write(&[&op_code_and_duty_cycle], false)
            }
            None => {
                let op_code_and_timeout = [
                    OpCode::SetRx.value(),
                    Self::timeout_1(rx_timeout_in_ms_final),
                    Self::timeout_2(rx_timeout_in_ms_final),
                    Self::timeout_3(rx_timeout_in_ms_final),
                ];
                self.intf.write(&[&op_code_and_timeout], false)
            }
        }
    }

    fn get_rx_payload(&mut self, rx_pkt_params: &PacketParams, receiving_buffer: &mut [u8]) -> Result<u8, RadioError> {
        let op_code = [OpCode::GetRxBufferStatus.value()];
        let mut rx_buffer_status = [0x00u8; 2];
        let read_status = self.intf.read_with_status(&[&op_code], &mut rx_buffer_status)?;
        if OpStatusErrorMask::is_error(read_status) {
            return Err(RadioError::OpError(read_status));
        }

        let mut payload_length_buffer = [0x00u8];
        if rx_pkt_params.implicit_header {
            self.intf.read(
                &[&[
                    OpCode::ReadRegister.value(),
                    Register::PayloadLength.addr1(),
                    Register::PayloadLength.addr2(),
                    0x00u8,
                ]],
                &mut payload_length_buffer,
                None,
            )?;
        } else {
            payload_length_buffer[0] = rx_buffer_status[0];
        }

        let payload_length = payload_length_buffer[0];
        let offset = rx_buffer_status[1];

        if (payload_length as usize) > receiving_buffer.len() {
            Err(RadioError::PayloadSizeMismatch(
                payload_length as usize,
                receiving_buffer.len(),
            ))
        } else {
            self.intf.read(
                &[&[OpCode::ReadBuffer.value(), offset, 0x00u8]],
                receiving_buffer,
                Some(payload_length),
            )?;
            Ok(payload_length)
        }
    }

    fn get_rx_packet_status(&mut self) -> Result<PacketStatus, RadioError> {
        let op_code = [OpCode::GetPacketStatus.value()];
        let mut pkt_status = [0x00u8; 3];
        let read_status = self.intf.read_with_status(&[&op_code], &mut pkt_status)?;
        if OpStatusErrorMask::is_error(read_status) {
            return Err(RadioError::OpError(read_status));
        }
        // check this ???
        let rssi = ((-(pkt_status[0] as i32)) >> 1) as i16;
        let snr = (((pkt_status[1] as i8) + 2) >> 2) as i16;
        let _signal_rssi = ((-(pkt_status[2] as i32)) >> 1) as i16; // unused currently

        Ok(PacketStatus { rssi, snr })
    }

    fn do_cad(&mut self, mdltn_params: &ModulationParams, rx_boosted_if_supported: bool) -> Result<(), RadioError> {
        self.intf.iv.enable_rf_switch_rx()?;

        let mut rx_gain_final = 0x94u8;
        // if Rx boosted, set max LNA gain, increase current by ~2mA for around ~3dB in sensitivity
        if rx_boosted_if_supported {
            rx_gain_final = 0x96u8;
        }

        let register_and_rx_gain = [
            OpCode::WriteRegister.value(),
            Register::RxGain.addr1(),
            Register::RxGain.addr2(),
            rx_gain_final,
        ];
        self.intf.write(&[&register_and_rx_gain], false)?;

        // See:
        //  https://lora-developers.semtech.com/documentation/tech-papers-and-guides/channel-activity-detection-ensuring-your-lora-packets-are-sent/how-to-ensure-your-lora-packets-are-sent-properly
        // for default values used here.
        let spreading_factor_val = spreading_factor_value(mdltn_params.spreading_factor)?;
        let op_code_and_cad_params = [
            OpCode::SetCADParams.value(),
            CADSymbols::_8.value(),      // number of symbols for detection
            spreading_factor_val + 13u8, // limit for detection of SNR peak
            10u8,                        // minimum symbol recognition
            0x00u8,                      // CAD exit mode without listen-before-send or subsequent receive processing
            0x00u8,                      // no timeout
            0x00u8,
            0x00u8,
        ];
        self.intf.write(&[&op_code_and_cad_params], false)?;

        let op_code_for_set_cad = [OpCode::SetCAD.value()];
        self.intf.write(&[&op_code_for_set_cad], false)
    }

    // Set the IRQ mask and DIO masks
    fn set_irq_params(&mut self, radio_mode: Option<RadioMode>) -> Result<(), RadioError> {
        let mut irq_flags_mask: IrqFlags = IrqFlags::empty();
        let mut dio1_flags_mask: IrqFlags = IrqFlags::empty();
        let dio2_flags_mask: IrqFlags = IrqFlags::empty();
        let dio3_flags_mask: IrqFlags = IrqFlags::empty();

        match radio_mode {
            Some(RadioMode::Standby) => {
                irq_flags_mask = IrqFlags::all();
                dio1_flags_mask = IrqFlags::all();
            }
            Some(RadioMode::Transmit) => {
                irq_flags_mask = IrqFlags::TX_DONE | IrqFlags::RX_TX_TIMEOUT;
                dio1_flags_mask = IrqFlags::TX_DONE | IrqFlags::RX_TX_TIMEOUT;
            }
            Some(RadioMode::Receive) | Some(RadioMode::ReceiveDutyCycle) => {
                irq_flags_mask = IrqFlags::all();
                dio1_flags_mask = IrqFlags::all();
            }
            Some(RadioMode::ChannelActivityDetection) => {
                irq_flags_mask = IrqFlags::CAD_DONE | IrqFlags::CAD_ACTIVITY_DETECTED;
                dio1_flags_mask = IrqFlags::CAD_DONE | IrqFlags::CAD_ACTIVITY_DETECTED;
            }
            _ => {}
        }
        let irq_mask = irq_flags_mask.bits();
        let dio1_mask = dio1_flags_mask.bits();
        let dio2_mask = dio2_flags_mask.bits();
        let dio3_mask = dio3_flags_mask.bits();
        let op_code_and_masks = [
            OpCode::CfgDIOIrq.value(),
            ((irq_mask >> 8) & 0x00FF) as u8,
            (irq_mask & 0x00FF) as u8,
            ((dio1_mask >> 8) & 0x00FF) as u8,
            (dio1_mask & 0x00FF) as u8,
            ((dio2_mask >> 8) & 0x00FF) as u8,
            (dio2_mask & 0x00FF) as u8,
            ((dio3_mask >> 8) & 0x00FF) as u8,
            (dio3_mask & 0x00FF) as u8,
        ];
        self.intf.write(&[&op_code_and_masks], false)
    }

    /// Process the radio irq
    fn process_irq(
        &mut self,
        radio_mode: RadioMode,
        rx_continuous: bool,
        cad_activity_detected: Option<&mut bool>,
    ) -> Result<(), RadioError> {
        loop {
            info!("process_irq loop entered");

            self.intf.iv.await_irq()?;
            let op_code = [OpCode::GetIrqStatus.value()];
            let mut irq_status = [0x00u8, 0x00u8];
            let read_status = self.intf.read_with_status(&[&op_code], &mut irq_status)?;
            if OpStatusErrorMask::is_error(read_status) {
                return Err(RadioError::OpError(read_status));
            }
            let irq_flags = ((irq_status[0] as u16) << 8) | (irq_status[1] as u16);
            let op_code_and_irq_status = [OpCode::ClrIrqStatus.value(), irq_status[0], irq_status[1]];
            self.intf.write(&[&op_code_and_irq_status], false)?;

            info!("process_irq satisfied: irq_flags = {:x}", irq_flags);

            // check for errors and unexpected interrupt masks (based on radio mode)
            return match IrqFlags::from_bits_truncate(irq_flags) {
                header_error if header_error.contains(IrqFlags::HEADER_ERROR) => Err(RadioError::HeaderError),
                crc_error if crc_error.contains(IrqFlags::CRC_ERROR) => {
                    if (radio_mode == RadioMode::Receive) | (radio_mode == RadioMode::ReceiveDutyCycle) {
                        Err(RadioError::CRCErrorOnReceive)
                    } else {
                        Err(RadioError::CRCErrorUnexpected)
                    }
                }
                rx_tx_timeout if rx_tx_timeout.contains(IrqFlags::RX_TX_TIMEOUT) => {
                    if radio_mode == RadioMode::Transmit {
                        Err(RadioError::TransmitTimeout)
                    } else if (radio_mode == RadioMode::Receive) | (radio_mode == RadioMode::ReceiveDutyCycle) {
                        Err(RadioError::ReceiveTimeout)
                    } else {
                        Err(RadioError::TimeoutUnexpected)
                    }
                }
                unexpected_txdone
                    if unexpected_txdone.contains(IrqFlags::TX_DONE) && (radio_mode != RadioMode::Transmit) =>
                {
                    Err(RadioError::TransmitDoneUnexpected)
                }

                unexpected_rxdone
                    if unexpected_rxdone.contains(IrqFlags::RX_DONE)
                        && !((radio_mode == RadioMode::Receive) || (radio_mode == RadioMode::ReceiveDutyCycle)) =>
                {
                    Err(RadioError::ReceiveDoneUnexpected)
                }
                unexpected_cad
                    if unexpected_cad.intersects(IrqFlags::CAD_ACTIVITY_DETECTED | IrqFlags::CAD_DONE)
                        && (radio_mode != RadioMode::ChannelActivityDetection) =>
                {
                    Err(RadioError::CADUnexpected)
                }
                header_valid if header_valid.contains(IrqFlags::HEADER_VALID) => {
                    info!("HeaderValid");
                    Ok(())
                }
                preamble_detected if preamble_detected.contains(IrqFlags::PREAMBLE_DETECTED) => {
                    info!("PreambleDetected");
                    Ok(())
                }
                syncword_valid if syncword_valid.contains(IrqFlags::SYNCWORD_VALID) => {
                    info!("SyncwordValid");
                    Ok(())
                }
                tx_done if tx_done.contains(IrqFlags::TX_DONE) => Ok(()),
                rx_done if rx_done.contains(IrqFlags::RX_DONE) => {
                    if !rx_continuous {
                        // implicit header mode timeout behavior (see DS_SX1261-2_V1.2 datasheet chapter 15.3)
                        let register_and_clear = [
                            OpCode::WriteRegister.value(),
                            Register::RTCCtrl.addr1(),
                            Register::RTCCtrl.addr2(),
                            0x00u8,
                        ];
                        self.intf.write(&[&register_and_clear], false)?;

                        let mut evt_clr = [0x00u8];
                        self.intf.read(
                            &[&[
                                OpCode::ReadRegister.value(),
                                Register::EvtClr.addr1(),
                                Register::EvtClr.addr2(),
                                0x00u8,
                            ]],
                            &mut evt_clr,
                            None,
                        )?;
                        evt_clr[0] |= 1 << 1;
                        let register_and_evt_clear = [
                            OpCode::WriteRegister.value(),
                            Register::EvtClr.addr1(),
                            Register::EvtClr.addr2(),
                            evt_clr[0],
                        ];
                        self.intf.write(&[&register_and_evt_clear], false)?;
                    }
                    Ok(())
                }
                cad_done if cad_done.contains(IrqFlags::CAD_DONE) => {
                    if let Some(cad_bool) = cad_activity_detected {
                        *cad_bool = cad_done.contains(IrqFlags::CAD_ACTIVITY_DETECTED);
                    }
                    Ok(())
                }
                // if an interrupt occurred for other than an error or operation completion (currently, PreambleDetected, SyncwordValid, and HeaderValid
                // are in that category), loop to wait again
                _ => continue,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    // use super::*;

    #[test]
    // -17 (0xEF) to +14 (0x0E) dBm by step of 1 dB if low power PA is selected
    // -9 (0xF7) to +22 (0x16) dBm by step of 1 dB if high power PA is selected
    fn power_level_negative_value_conversion() {
        let mut i32_val: i32 = -17;
        assert_eq!(i32_val as u8, 0xefu8);
        i32_val = -9;
        assert_eq!(i32_val as u8, 0xf7u8);
    }
}
