//! Nets (contract §1.5, §3.1): union-find over the wires, plus every pin of role `ground` in one net.

use std::collections::BTreeMap;

use crate::model::{pin_def, Comps, Wires};
use crate::{kind_def, Kind, Net, Netlist, PinRef, Role};

fn find(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    if ra != rb {
        // Smaller index as root: deterministic, independent of the wire order.
        let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
        parent[hi] = lo;
    }
}

pub(crate) fn build(comps: &Comps, wires: &Wires) -> Netlist {
    // Every pin of every component, ascending (comp, pin id).
    let mut pins: Vec<PinRef> = comps
        .values()
        .flat_map(|c| kind_def(c.kind).pins.iter().map(move |p| PinRef { comp: c.id, pin: p.id.to_string() }))
        .collect();
    pins.sort();
    let index = |p: &PinRef| pins.binary_search(p).ok();
    let mut parent: Vec<usize> = (0..pins.len()).collect();

    for w in wires.values() {
        if let (Some(a), Some(b)) = (index(&w.a), index(&w.b)) {
            union(&mut parent, a, b);
        }
    }
    let is_ground = |p: &PinRef| {
        comps.get(&p.comp).and_then(|c| pin_def(c.kind, &p.pin)).is_some_and(|d| d.role == Role::Ground)
    };
    let mut first_ground: Option<usize> = None;
    for (i, p) in pins.iter().enumerate() {
        if is_ground(p) {
            match first_ground {
                None => first_ground = Some(i),
                Some(g) => union(&mut parent, g, i),
            }
        }
    }

    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..pins.len() {
        let r = find(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    let mut wires_of: BTreeMap<usize, Vec<crate::WireId>> = BTreeMap::new();
    for w in wires.values() {
        if let Some(a) = index(&w.a) {
            let r = find(&mut parent, a);
            wires_of.entry(r).or_default().push(w.id);
        }
    }

    let mut nets: Vec<Net> = groups
        .into_iter()
        .map(|(root, members)| {
            let net_pins: Vec<PinRef> = members.iter().map(|&i| pins[i].clone()).collect();
            let ground = net_pins.iter().any(is_ground);
            let name = if ground {
                "GND".to_string()
            } else if let Some(p) = net_pins.iter().find(|p| comps.get(&p.comp).is_some_and(|c| c.kind != Kind::Junction)) {
                let c = &comps[&p.comp];
                let short = pin_def(c.kind, &p.pin).map_or(p.pin.as_str(), |d| d.short);
                format!("{}.{}", c.label, short)
            } else {
                format!("J{}", net_pins[0].comp.0)
            };
            let mut net_wires = wires_of.remove(&root).unwrap_or_default();
            net_wires.sort();
            Net { key: net_pins[0].clone(), name, ground, pins: net_pins, wires: net_wires }
        })
        .collect();
    nets.sort_by(|a, b| a.key.cmp(&b.key));
    Netlist { nets }
}
