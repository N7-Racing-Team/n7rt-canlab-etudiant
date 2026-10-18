//! Public stand-in for the oracle (docs/distribution.md §1): the reference consumer, the comparison of a nominal check
//! and the judgement of run files are private and compiled in only by the instructor builds (build.rs). The items
//! below keep their signatures so that every caller builds either way; they never give a verdict.

use crate::{e2e, BoardTick, Pack, RunRecord, TickOut, Verdict};

/// Answer of every oracle entry point in a public build.
pub(crate) const ABSENT: &str =
    "oracle absent de cette version : jugement réservé aux outils encadrant (n7rt-tool, édition encadrant)";

/// Supervision state of one monitored message after the last tick (diagnostics and tests; never shown to students).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Supervision {
    /// E2E state machine (P22 messages only).
    pub sm: Option<e2e::SmState>,
    /// Status of the last cyclic evaluation.
    pub status: Option<e2e::CheckStatus>,
    pub stale: bool,
    pub rx_timeout: bool,
}

/// Reference consumer of the mission: private. In a public build it cannot be created.
pub struct RefConsumer {
    _private: (),
}

impl RefConsumer {
    /// # Panics
    /// Always in a public build (use [`RefConsumer::try_new`]).
    pub fn new(pack: &Pack) -> RefConsumer {
        match RefConsumer::try_new(pack) {
            Ok(c) => c,
            Err(e) => panic!("RefConsumer::new : {e}"),
        }
    }

    /// Err in a public build.
    pub fn try_new(_pack: &Pack) -> Result<RefConsumer, String> {
        Err(ABSENT.into())
    }

    pub fn on_frame(&mut self, _t_us: u64, _id: u32, _flags: u8, _data: &[u8]) {}

    pub fn on_tick(&mut self, t_us: u64) -> TickOut {
        TickOut { t_us, slots: Vec::new() }
    }

    pub fn supervision(&self, _id: u32) -> Option<Supervision> {
        None
    }
}

/// Comparison of a nominal check: private. In a public build the verdict is a failure that says so.
pub fn compare(_pack: &Pack, _expected: &[TickOut], _board: &[BoardTick]) -> Verdict {
    Verdict { pass: false, detail: ABSENT.into() }
}

/// Judgement of a run file: private. Err in a public build.
pub fn judge_run(_pack: &Pack, _run: &RunRecord) -> Result<Verdict, String> {
    Err(ABSENT.into())
}
