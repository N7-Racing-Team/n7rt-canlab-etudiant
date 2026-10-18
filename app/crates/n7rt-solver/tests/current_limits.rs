use n7rt_solver::{Element, Network, Solver, SupplyMode};

fn close(a: f64, b: f64) { assert!((a-b).abs() < 1e-8, "{a} != {b}"); }

fn driver(load: f64, imposed: Option<f64>) -> Network {
    let mut elements = vec![
        Element::VSource { pos: 1, neg: 0, v: 4.8, r_out: 0.01, gate: None },
        Element::LimitedVcvs { pos: 2, neg: 0, cp: 1, cn: 0, gain: 0.5, r_out: 8.0,
            i_lim: 0.02, key: 1, gate: None },
        Element::Resistor { a: 2, b: 0, r: load, leakage: false, gate: None },
    ];
    if let Some(v) = imposed { elements.push(Element::VSource {pos:2, neg:0, v, r_out:0.01, gate:None}); }
    Network { nodes:3, ground:Some(0), elements, gates:vec![] }
}

#[test]
fn bidirectional_limit_and_recovery_preserve_thevenin_law() {
    let mut s = Solver::new();
    s.load(&driver(1200.0,None)).unwrap(); s.step_frame().unwrap();
    assert_eq!(s.supply_mode(1), Some(SupplyMode::Cv));
    close(s.node(2).volts, 2.4*1200.0/1208.0);
    s.load(&driver(0.1,None)).unwrap(); s.step_frame().unwrap();
    assert_eq!(s.supply_mode(1), Some(SupplyMode::Cc));
    close(s.element_current(1).unwrap(),0.02); close(s.node(2).volts,0.002);
    s.load(&driver(1200.0,Some(5.0))).unwrap(); s.step_frame().unwrap();
    assert_eq!(s.supply_mode(1), Some(SupplyMode::CcNeg));
    close(s.element_current(1).unwrap(),-0.02);
    s.load(&driver(1200.0,None)).unwrap(); s.step_frame().unwrap();
    assert_eq!(s.supply_mode(1), Some(SupplyMode::Cv));
    close(s.node(2).volts,2.4*1200.0/1208.0);
}

#[test]
fn limited_series_rail_draws_the_same_current_from_its_input() {
    let mut s=Solver::new();
    let net=Network { nodes:3,ground:Some(0),gates:vec![],elements:vec![
        Element::VSource {pos:1,neg:0,v:12.0,r_out:0.2,gate:None},
        Element::Supply {pos:2,neg:1,v:0.0,r_out:10.0,i_lim:Some(0.025),key:9,gate:None},
        Element::Resistor {a:2,b:0,r:100.0,leakage:false,gate:None},
    ]};
    s.load(&net).unwrap(); s.step_frame().unwrap();
    assert_eq!(s.supply_mode(1),Some(SupplyMode::Cc));
    close(s.element_current(0).unwrap(),s.element_current(1).unwrap());
    close(s.node(2).volts,2.5); close(s.node(1).volts,11.995);
    let original=s.state_digest(); let time=s.time_ns();
    let mut snapshot=s.clone(); snapshot.preview().unwrap();
    assert_eq!(s.state_digest(),original); assert_eq!(s.time_ns(),time);
}

#[test]
fn precise_preview_recovers_conditioned_norton_currents_without_mutating_transients() {
    let network=|current:f64| Network {nodes:6,ground:Some(0),gates:vec![],elements:vec![
        Element::VSource{pos:1,neg:0,v:9.0,r_out:0.023,gate:None},
        Element::Supply{pos:2,neg:1,v:0.0,r_out:13.0,i_lim:Some(0.031),key:1,gate:None},
        Element::Resistor{a:2,b:3,r:0.027,leakage:false,gate:None},
        Element::Supply{pos:4,neg:3,v:0.0,r_out:3.2/current,i_lim:Some(current),key:2,gate:None},
        Element::Resistor{a:4,b:5,r:0.027,leakage:false,gate:None},
        Element::Resistor{a:5,b:0,r:47.0,leakage:false,gate:None},
    ]};
    for current in [0.0067,0.0137] {
        let mut original=Solver::new();original.load(&network(current)).unwrap();original.step_frame().unwrap();
        let digest=original.state_digest();let time=original.time_ns();
        let mut probe=original.clone();probe.preview_precise().unwrap();
        let expected=current*47.0;
        assert!((probe.node(5).volts-expected).abs()<=8.0*f64::EPSILON*expected);
        assert_eq!(probe.supply_mode(1),Some(SupplyMode::Cv));
        assert_eq!(probe.supply_mode(3),Some(SupplyMode::Cc));
        assert_eq!(probe.element_current(3),Some(current));
        assert_eq!(probe.state_digest(),digest);assert_eq!(original.state_digest(),digest);
        assert_eq!(probe.time_ns(),time);assert_eq!(original.time_ns(),time);
    }
    let mut capacitive=network(0.0091);
    capacitive.elements.push(Element::Capacitor{a:5,b:0,c:2.3e-7,key:3});
    let mut original=Solver::new();original.load(&capacitive).unwrap();original.step_frame().unwrap();
    let digest=original.state_digest();let time=original.time_ns();
    original.preview_precise().unwrap();
    assert_eq!(original.state_digest(),digest,"preview must retain stored capacitor charges and modes");
    assert_eq!(original.time_ns(),time);
}
