//! AUTOSAR E2E building blocks, sender side (generic, public): CRC8H2F and Profile 22 protect with a 16-entry Data ID
//! list (CRC in byte 0, counter in the low nibble of byte 1, CRC over bytes 1…n−1 then `DataIDList[counter]`), plus
//! the status and state names shared with the receiver. The receiver side (P22 check, life counter check, E2E state
//! machine) belongs to the private oracle (docs/distribution.md §1).

/// `Crc_CalculateCRC8H2F`: polynomial 0x2F, start value 0xFF, final XOR 0xFF.
pub fn crc8h2f(data: &[u8]) -> u8 {
    crc8h2f_update(0xFF, data) ^ 0xFF
}

fn crc8h2f_update(mut crc: u8, data: &[u8]) -> u8 {
    for &b in data {
        crc ^= b;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 { (crc << 1) ^ 0x2F } else { crc << 1 };
        }
    }
    crc
}

/// Profile 22 CRC of a frame: bytes 1…len−1, then the Data ID byte.
pub fn p22_crc(frame: &[u8], data_id: u8) -> u8 {
    let reg = crc8h2f_update(0xFF, frame.get(1..).unwrap_or(&[]));
    crc8h2f_update(reg, &[data_id]) ^ 0xFF
}

/// Writes the counter (low nibble of byte 1) and the CRC (byte 0). `frame.len()` ≥ 2.
pub fn p22_protect(frame: &mut [u8], counter: u8, data_ids: &[u8; 16]) {
    let c = counter & 0x0F;
    frame[1] = (frame[1] & 0xF0) | c;
    frame[0] = p22_crc(frame, data_ids[c as usize]);
}

/// Status of one check (names of the AUTOSAR P22 statuses; the same classification is used for life counters).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Ok,
    OkSomeLost,
    Repeated,
    WrongSequence,
    NoNewData,
    Error,
}

impl CheckStatus {
    /// The frame brings a fresh value (OK or OKSOMELOST).
    pub fn is_fresh(self) -> bool {
        matches!(self, CheckStatus::Ok | CheckStatus::OkSomeLost)
    }
}

/// States of the E2E state machine (DEINIT is left by `new`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmState {
    NoData,
    Init,
    Valid,
    Invalid,
}

/// Parameters of the state machine (AUTOSAR names), read from the pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SmConfig {
    pub window_size_valid: u8,
    pub window_size_init: u8,
    pub window_size_invalid: u8,
    pub max_error_state_valid: u8,
    pub max_error_state_init: u8,
    pub max_error_state_invalid: u8,
    pub min_ok_state_valid: u8,
    pub min_ok_state_init: u8,
    pub min_ok_state_invalid: u8,
    pub clear_to_invalid: bool,
}

impl SmConfig {
    /// AUTOSAR constraints on the parameters.
    pub fn check(&self) -> Result<(), String> {
        let c = self;
        let ok = c.window_size_valid >= 1
            && c.window_size_init >= 1
            && c.window_size_invalid >= 1
            && c.window_size_init <= c.window_size_valid
            && c.window_size_invalid <= c.window_size_valid
            && c.max_error_state_valid >= c.max_error_state_init
            && c.max_error_state_init >= c.max_error_state_invalid
            && c.min_ok_state_valid >= 1
            && c.min_ok_state_valid <= c.min_ok_state_init
            && c.min_ok_state_init <= c.min_ok_state_invalid
            && c.min_ok_state_init as u16 + c.max_error_state_init as u16 <= c.window_size_init as u16
            && c.min_ok_state_valid as u16 + c.max_error_state_valid as u16 <= c.window_size_valid as u16
            && c.min_ok_state_invalid as u16 + c.max_error_state_invalid as u16 <= c.window_size_invalid as u16;
        if ok {
            Ok(())
        } else {
            Err("paramètres de la machine d'états incohérents".to_string())
        }
    }
}
