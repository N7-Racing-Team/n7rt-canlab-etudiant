//! N7RT CAN Lab core, independent of the UI toolkit.
//!
//! - `discovery`: ST-LINK virtual COM ports by USB VID/PID and probe serial number;
//! - `runtime`: pinned runtime release and board identification (`HELLO_ACK` challenge);
//! - `sim`: Virtual CAN generator (`sim` thread);
//! - `engine`: N7RT-Link PC side (`link` thread) and the `Lab` handle used by the UI;
//! - `model`: what is published to the UI;
//! - `e2e`: automated verification of the display against the board's real answers;
//! - `fake`: in-memory board for tests without hardware;
//! - `phys`: Physical Lab engine (circuit lowering, `phys` thread, probe API), independent of the board session;
//! - `mission`: mission controller (student and instructor modes, group or reference pack, bench, vehicle bus source,
//!   hidden nominal check, public library);
//! - `access`: modes, instructor directory discovery, student state, run files;
//! - `cohort`: instructor tools (group packs of a cohort, judgement of a run file);
//! - `workflow`: student progression engines (hidden test vectors, replay, public comparator).

pub mod access;
pub mod cohort;
pub mod discovery;
pub mod e2e;
pub mod engine;
pub mod fake;
pub mod metrics;
pub mod mission;
pub mod model;
pub mod phys;
pub mod runtime;
pub mod sim;
pub mod workflow;

pub use engine::{Command, Connector, Lab, LabConfig, OpenError, SerialConnector, Transport};
pub use access::{InstructorLocator, Mode};
pub use mission::{AccessView, MissionConfig, MissionEntry, MissionView, Missions};
