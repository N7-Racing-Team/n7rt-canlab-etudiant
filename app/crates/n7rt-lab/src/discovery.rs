//! Serial port discovery: ST-LINK virtual COM ports, identified by USB VID/PID and probe serial
//! number. No port name is ever assumed: `COM9`, `/dev/ttyACM0` and `/dev/cu.usbmodem*` are only
//! what the OS reports today.

use serde::Serialize;

pub const ST_VID: u16 = 0x0483;

/// ST-LINK probes that expose a virtual COM port (PIDs from ST's drivers and OpenOCD).
pub const STLINK_PIDS: [(u16, &str); 8] = [
    (0x374B, "STLINK-V2-1"),
    (0x3752, "STLINK-V2-1"),
    (0x374E, "STLINK-V3E"),
    (0x374F, "STLINK-V3"),
    (0x3753, "STLINK-V3 (2 VCP)"),
    (0x3754, "STLINK-V3"),
    (0x3755, "STLINK-V3"),
    (0x3757, "STLINK-V3PWR"),
];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PortInfo {
    pub port: String,
    pub vid: Option<u16>,
    pub pid: Option<u16>,
    /// ST-LINK serial number (stable across re-plugs, unlike the port name).
    pub serial: Option<String>,
    pub product: Option<String>,
    /// Probe model when the VID/PID is a known ST-LINK.
    pub probe: Option<&'static str>,
}

impl PortInfo {
    pub fn is_stlink(&self) -> bool {
        self.probe.is_some()
    }

    /// Short label for the UI: probe, serial and port.
    pub fn label(&self) -> String {
        match (self.probe, &self.serial) {
            (Some(p), Some(sn)) => format!("{p} {sn} ({})", self.port),
            (Some(p), None) => format!("{p} ({})", self.port),
            _ => self.port.clone(),
        }
    }
}

pub fn probe_name(vid: u16, pid: u16) -> Option<&'static str> {
    if vid != ST_VID {
        return None;
    }
    STLINK_PIDS.iter().find(|(p, _)| *p == pid).map(|(_, n)| *n)
}

/// Every serial port the OS reports, ST-LINK ports first.
pub fn scan() -> Result<Vec<PortInfo>, String> {
    let ports = serialport::available_ports().map_err(|e| format!("énumération des ports série impossible : {e}"))?;
    let mut out: Vec<PortInfo> = ports
        .into_iter()
        .map(|p| match p.port_type {
            serialport::SerialPortType::UsbPort(u) => PortInfo {
                port: p.port_name,
                vid: Some(u.vid),
                pid: Some(u.pid),
                serial: u.serial_number,
                product: u.product,
                probe: probe_name(u.vid, u.pid),
            },
            _ => PortInfo { port: p.port_name, vid: None, pid: None, serial: None, product: None, probe: None },
        })
        .collect();
    // macOS lists each device twice; /dev/cu.* is the one to open (no DCD wait).
    let cu: Vec<String> = out.iter().filter_map(|p| p.port.strip_prefix("/dev/cu.").map(str::to_owned)).collect();
    out.retain(|p| p.port.strip_prefix("/dev/tty.").map_or(true, |rest| !cu.iter().any(|c| c == rest)));
    out.sort_by(|a, b| b.is_stlink().cmp(&a.is_stlink()).then_with(|| a.port.cmp(&b.port)));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stlink_identification() {
        assert_eq!(probe_name(0x0483, 0x374E), Some("STLINK-V3E"));
        assert_eq!(probe_name(0x0483, 0x5740), None);
        assert_eq!(probe_name(0x1234, 0x374E), None);
    }
}
