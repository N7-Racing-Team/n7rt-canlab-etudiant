//! Cost of the edit functions on the performance fixture (contract §8.6: a drag sends one preview per frame).
//! Ignored by default: `cargo test -p n7rt-circuit --release --test perf -- --ignored --nocapture`.

use std::time::Instant;

use n7rt_circuit::{demos, persist, Document, Edit, Kind, Pt};

#[test]
#[ignore]
fn preview_cost_on_perf_fixture() {
    let demo = demos().iter().find(|d| d.id == "perf-150-300").expect("perf fixture");
    let circuit = persist::load(demo.json).expect("fixture loads");
    let doc = Document::new(circuit);
    let rev = doc.rev();
    let ecu = doc.circuit().components().values().find(|c| c.kind == Kind::EcuStm32).expect("an ECU").id;
    let wires = doc.circuit().wires().len();

    let t = Instant::now();
    let checks = doc.circuit().check();
    println!("check(): {} violations, {:?} ({} wires)", checks.len(), t.elapsed(), wires);

    for dx in [1, 3, 7] {
        let edit = Edit::Move { components: vec![ecu], wires: vec![], delta: Pt { x: dx, y: 0 } };
        let t = Instant::now();
        let r = doc.preview(rev, &edit);
        println!("preview move ECU +{dx}: {:?} ({})", t.elapsed(), if r.is_ok() { "ok" } else { "refused" });
    }
    let some: Vec<_> = doc.circuit().components().values().filter(|c| c.kind != Kind::Junction).take(20).map(|c| c.id).collect();
    let edit = Edit::Move { components: some, wires: vec![], delta: Pt { x: 2, y: 0 } };
    let t = Instant::now();
    let r = doc.preview(rev, &edit);
    println!("preview move 20 components: {:?} ({})", t.elapsed(), if r.is_ok() { "ok" } else { "refused" });
    let t = Instant::now();
    let _ = doc.circuit().netlist();
    println!("netlist(): {:?}", t.elapsed());
}
