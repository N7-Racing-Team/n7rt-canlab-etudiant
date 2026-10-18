//! Automated end-to-end verification.
//!
//! Two independent checks:
//! - **display**: every row the UI rendered (cell text read back from the DOM) is parsed and
//!   compared with the frame actually sent and the `RESULT` actually received from the board
//!   (`Lab::record`), bit for bit for published values;
//! - **oracle**: the board's answers equal what the SDK template's example `student_decoder.c`
//!   computes (slot 0 = `len`, slot 1 = `data[0]`, or a reject of slot 1 when `len` = 0). This is
//!   the public example shipped to every student, not mission content; it only proves that the
//!   frames really went through `student_on_frame()` on the board.
//!
//! The cell formats are the ones of `ui/src/format.ts`.

use std::collections::{HashMap, VecDeque};

use serde::{Deserialize, Serialize};

use crate::model::{EntryView, FrameRow, LogRow};
use n7rt_link::{can_data_length, CAN_BRS, CAN_ESI, CAN_EXT, CAN_FD, CAN_RTR};

/// One frame row as rendered by the UI: the text content of each cell.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DisplayedRow {
    pub session: String,
    pub seq: String,
    pub id: String,
    pub kind: String,
    pub dlc: String,
    pub data: String,
    pub result: String,
    pub publications: String,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct Verification {
    pub displayed: usize,
    pub matched: usize,
    pub mismatches: Vec<String>,
    pub oracle_checked: usize,
    pub oracle_mismatches: Vec<String>,
}

impl Verification {
    pub fn ok(&self) -> bool {
        self.mismatches.is_empty() && self.oracle_mismatches.is_empty() && self.displayed > 0
    }
}

/// Entries the SDK example `student_decoder.c` publishes for a delivered frame.
pub fn example_model(row: &FrameRow) -> Vec<EntryView> {
    let data = hex_bytes(&row.data).unwrap_or_default();
    let mut out = vec![EntryView { slot: 0, kind: 0, bits: (data.len() as f32).to_bits() }];
    out.push(match data.first() {
        Some(&b) => EntryView { slot: 1, kind: 0, bits: (b as f32).to_bits() },
        None => EntryView { slot: 1, kind: 1, bits: 0 },
    });
    out
}

/// `Some(explanation)` when an answered frame differs from the example model.
fn oracle_mismatch(r: &FrameRow) -> Option<String> {
    let want = example_model(r);
    (r.entries != want || r.result_flags != 0).then(|| {
        format!(
            "session {:08X} trame {} : carte {:?} (flags {}), modèle exemple {:?}",
            r.session_id, r.frame_seq, r.entries, r.result_flags, want
        )
    })
}

/// Checks every answered frame of `record` against the example model.
pub fn check_oracle(record: &[FrameRow], v: &mut Verification) {
    for r in record.iter().filter(|r| r.outcome == "result" && r.status == 0) {
        v.oracle_checked += 1;
        if let Some(m) = oracle_mismatch(r) {
            v.oracle_mismatches.push(m);
        }
    }
}

/// What crossed the link, kept for the automated verification at constant memory: the last
/// `KEEP_ROWS` frames (the UI never shows more than 200) and the oracle applied to every
/// answered frame as it completes, whatever the length of the run.
#[derive(Debug, Default)]
pub struct Recorder {
    pub rows: VecDeque<FrameRow>,
    pub logs: VecDeque<LogRow>,
    pub total_rows: u64,
    pub total_logs: u64,
    pub oracle_checked: usize,
    pub oracle_mismatches: Vec<String>,
}

impl Recorder {
    pub const KEEP_ROWS: usize = 8192;
    pub const KEEP_LOGS: usize = 1024;
    const MAX_MISMATCHES: usize = 50;

    pub fn row(&mut self, r: &FrameRow) {
        self.total_rows += 1;
        if r.outcome == "result" && r.status == 0 {
            self.oracle_checked += 1;
            if let Some(m) = oracle_mismatch(r) {
                if self.oracle_mismatches.len() < Self::MAX_MISMATCHES {
                    self.oracle_mismatches.push(m);
                }
            }
        }
        if self.rows.len() == Self::KEEP_ROWS {
            self.rows.pop_front();
        }
        self.rows.push_back(r.clone());
    }

    pub fn log(&mut self, l: &LogRow) {
        self.total_logs += 1;
        if self.logs.len() == Self::KEEP_LOGS {
            self.logs.pop_front();
        }
        self.logs.push_back(l.clone());
    }
}

fn hex_bytes(s: &str) -> Option<Vec<u8>> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

fn parse_flags(kind: &str) -> Option<u8> {
    let mut flags = 0;
    for tok in kind.split_whitespace() {
        flags |= match tok {
            "STD" => 0,
            "EXT" => CAN_EXT,
            "RTR" => CAN_RTR,
            "FD" => CAN_FD,
            "BRS" => CAN_BRS,
            "ESI" => CAN_ESI,
            _ => return None,
        };
    }
    Some(flags)
}

/// `"s0=8 · s1=17"`, `"s1=rejet"`, `"—"`.
fn parse_publications(text: &str) -> Option<Vec<EntryView>> {
    let text = text.trim();
    if text == "—" {
        return Some(Vec::new());
    }
    text.split('·')
        .map(|part| {
            let (slot, value) = part.trim().strip_prefix('s')?.split_once('=')?;
            let slot: u8 = slot.parse().ok()?;
            if value == "rejet" {
                return Some(EntryView { slot, kind: 1, bits: 0 });
            }
            let v: f32 = value.parse().ok()?;
            Some(EntryView { slot, kind: 0, bits: v.to_bits() })
        })
        .collect()
}

/// Text of the result cell for a row (see `ui/src/format.ts`, `resultText`).
pub fn result_text(r: &FrameRow) -> String {
    match r.outcome {
        "result" => {
            let mut s = if r.status == 0 { "OK".to_owned() } else { "non livrée".to_owned() };
            if r.result_flags & 0x01 != 0 {
                s.push_str(" +débordement");
            }
            if r.result_flags & 0x02 != 0 {
                s.push_str(" +budget");
            }
            s
        }
        "nack" => format!("NACK {}", r.nack.as_deref().unwrap_or("?")),
        "lost" => "perdue".into(),
        "aborted" => "abandonnée".into(),
        other => other.into(),
    }
}

/// Compares the rendered rows with the record of what really crossed the link.
pub fn verify_display(record: &[FrameRow], displayed: &[DisplayedRow]) -> Verification {
    let by_key: HashMap<(u32, u32), &FrameRow> = record.iter().map(|r| ((r.session_id, r.frame_seq), r)).collect();
    let mut v = Verification { displayed: displayed.len(), ..Verification::default() };
    for d in displayed {
        let key = (u32::from_str_radix(d.session.trim(), 16).ok(), d.seq.trim().parse::<u32>().ok());
        let (Some(session), Some(seq)) = key else {
            v.mismatches.push(format!("ligne illisible : {d:?}"));
            continue;
        };
        let Some(r) = by_key.get(&(session, seq)) else {
            v.mismatches.push(format!("session {session:08X} trame {seq} affichée mais absente des échanges réels"));
            continue;
        };
        let mut errs = Vec::new();
        let id = d.id.trim().strip_prefix("0x").and_then(|h| u32::from_str_radix(h, 16).ok());
        if id != Some(r.can_id) {
            errs.push(format!("ID {:?} ≠ 0x{:X}", d.id, r.can_id));
        }
        if parse_flags(&d.kind) != Some(r.flags) {
            errs.push(format!("type {:?} ≠ flags 0x{:02X}", d.kind, r.flags));
        }
        if d.dlc.trim().parse::<u8>().ok() != Some(r.dlc) {
            errs.push(format!("DLC {:?} ≠ {}", d.dlc, r.dlc));
        }
        let shown = if d.data.trim() == "—" { Some(Vec::new()) } else { hex_bytes(&d.data) };
        let sent = hex_bytes(&r.data);
        if shown != sent || sent.as_ref().map(Vec::len) != can_data_length(r.dlc, r.flags) {
            errs.push(format!("données {:?} ≠ {}", d.data, r.data));
        }
        if d.result.trim() != result_text(r) {
            errs.push(format!("résultat {:?} ≠ {:?}", d.result, result_text(r)));
        }
        if parse_publications(&d.publications).as_ref() != Some(&r.entries) {
            errs.push(format!("publications {:?} ≠ {:?}", d.publications, r.entries));
        }
        if errs.is_empty() {
            v.matched += 1;
        } else {
            v.mismatches.push(format!("session {session:08X} trame {seq} : {}", errs.join(", ")));
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(data: &str, flags: u8, dlc: u8, entries: Vec<EntryView>) -> FrameRow {
        FrameRow {
            session_id: 0x1234ABCD,
            frame_seq: 7,
            t_us: 0,
            can_id: 0x123,
            flags,
            dlc,
            data: data.into(),
            outcome: "result",
            status: 0,
            result_flags: 0,
            cycles: 50,
            entries,
            nack: None,
            rtt_us: Some(900),
            stim_rev: 0,
        }
    }

    #[test]
    fn recorder_is_bounded_and_checks_every_frame() {
        let good = row("11", 0, 1, vec![
            EntryView { slot: 0, kind: 0, bits: 1f32.to_bits() },
            EntryView { slot: 1, kind: 0, bits: 17f32.to_bits() },
        ]);
        let mut rec = Recorder::default();
        for i in 0..10_000u32 {
            let mut r = good.clone();
            r.frame_seq = i + 1;
            if i == 9_999 {
                r.entries[1].bits = 18f32.to_bits();
            }
            rec.row(&r);
        }
        assert_eq!((rec.total_rows, rec.rows.len(), rec.oracle_checked), (10_000, Recorder::KEEP_ROWS, 10_000));
        assert_eq!(rec.oracle_mismatches.len(), 1);
        assert_eq!(rec.rows.back().unwrap().frame_seq, 10_000);
    }

    #[test]
    fn display_and_oracle() {
        let r = row("1122", 0, 2, vec![
            EntryView { slot: 0, kind: 0, bits: 2f32.to_bits() },
            EntryView { slot: 1, kind: 0, bits: 17f32.to_bits() },
        ]);
        let d = DisplayedRow {
            session: "1234ABCD".into(),
            seq: "7".into(),
            id: "0x123".into(),
            kind: "STD".into(),
            dlc: "2".into(),
            data: "11 22".into(),
            result: "OK".into(),
            publications: "s0=2 · s1=17".into(),
        };
        let mut v = verify_display(&[r.clone()], &[d.clone()]);
        check_oracle(&[r.clone()], &mut v);
        assert!(v.ok(), "{v:?}");
        let wrong = DisplayedRow { publications: "s0=2 · s1=18".into(), ..d };
        assert_eq!(verify_display(&[r], &[wrong]).mismatches.len(), 1);
        let rtr = row("", CAN_RTR, 4, vec![EntryView { slot: 0, kind: 0, bits: 0 }, EntryView { slot: 1, kind: 1, bits: 0 }]);
        let mut v = Verification::default();
        check_oracle(&[rtr], &mut v);
        assert!(v.oracle_mismatches.is_empty());
    }
}
