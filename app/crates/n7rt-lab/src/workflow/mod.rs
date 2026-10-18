//! Student progression (release 0.3): the generic engines behind the mission steps E0 Essai nominal → E1–E3
//! incidents → E4 qualification finale → E5 clôture (private spec ch. 24, adapted without server).
//!
//! Nothing here knows an answer. The truths of a group (expected fiche values, incident descriptors, hidden test
//! vectors and their expected outputs) are rendered by the instructor edition when it generates the group pack, come
//! sealed in that pack and stay in memory: these modules only compare, replay and check.
//!
//! - `vector`: hidden test vectors (`n7rt-vec`) and the exact-delivery check;
//! - `bench`: a virtual copy of a bench driven with a vehicle (hidden sequences, key-measure computations);
//! - `compare`: the public comparator of the board's ticks with the sealed expected outputs (`n7rt-exp`);
//! - `replay`: the bus source that replays a hidden vector to the board;
//! - `progress`: the steps, their displayed states, the attempts and the rules between them (pure logic);
//! - `store`: atomic persistence with integrity digest of the progression documents;
//! - `export`: the final fiche of a closed mission (canonical JSON, readable page, sealed evidence);
//! - `fiche`: the nominal part of the fiche (public schema), its completeness and its check against the sealed truth;
//! - `incident`: the incidents E1–E3 (sealed content, incident part of the fiche, key-measure rule, conformity);
//! - `measure`: the key measure's expected value and its value without the anomaly (evidence, virtual benches);
//! - `sealed`: the 0.3 group pack content (student pack + workflow section) and its strict reading;
//! - `sequence`: instructor edition, the hidden sequences V-0…V-C of the incidents (draws, anomaly switching);
//! - `qualification`: instructor edition, the hidden final qualification E4 (28 s, CP-1…CP-8 labels);
//! - `compile`: instructor edition, renders the workflow section of a group (truth, hidden vectors, expected outputs);
//! - `pcheck`: physical checks P-1/P-2 of the student's circuit against the reference bench;
//! - `session`: the progression of an open pack (persisted progress and fiche, student view, public messages).

pub mod bench;
pub mod compare;
pub mod compile;
pub mod export;
pub mod fiche;
pub mod incident;
pub mod measure;
pub mod pcheck;
pub mod progress;
pub mod qualification;
pub mod replay;
pub mod sealed;
pub mod sequence;
pub mod session;
pub mod store;
pub mod vector;
