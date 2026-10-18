//! The V1 library as data (contract docs/physical-lab-v1.md §2 and §2.2). Coordinates in grid units at R0, origin
//! of the component; every pin lies on the grid and is reached by a 1-unit lead from the body. Values in SI base
//! units. Provenance of every model value: contract §2.1 (DATASHEET / N7RT_MODEL classes).

use crate::{Actuator, ActuatorKind, Dir, Kind, KindDef, Lit, ParamDef, PinDef, Pt, Rect, Role, StateDef, ValueType};

const OHM: &str = "\u{3A9}";

const fn pt(x: i32, y: i32) -> Pt {
    Pt { x, y }
}

const fn rect(x0: i32, y0: i32, x1: i32, y1: i32) -> Rect {
    Rect { x0, y0, x1, y1 }
}

const fn pin(id: &'static str, name: &'static str, short: &'static str, pos: Pt, dir: Dir, role: Role, required: bool) -> PinDef {
    PinDef { id, name, short, pos, dir, role, required }
}

const fn num(name: &'static str, label_fr: &'static str, unit: &'static str, min: f64, max: f64, default: f64) -> ParamDef {
    ParamDef { name, label_fr, unit, vtype: ValueType::Number, min, max, default: Lit::Num(default), choices: &[], editable: true, hidden: false }
}

const fn fixed(name: &'static str, label_fr: &'static str, unit: &'static str, value: f64) -> ParamDef {
    ParamDef { name, label_fr, unit, vtype: ValueType::Number, min: value, max: value, default: Lit::Num(value), choices: &[], editable: false, hidden: false }
}

const fn int(name: &'static str, label_fr: &'static str, unit: &'static str, min: i64, max: i64, default: i64) -> ParamDef {
    ParamDef { name, label_fr, unit, vtype: ValueType::Int, min: min as f64, max: max as f64, default: Lit::Int(default), choices: &[], editable: true, hidden: false }
}

const fn boolean(name: &'static str, label_fr: &'static str, default: bool) -> ParamDef {
    ParamDef { name, label_fr, unit: "", vtype: ValueType::Bool, min: 0.0, max: 1.0, default: Lit::Bool(default), choices: &[], editable: true, hidden: false }
}

const fn choice(name: &'static str, label_fr: &'static str, choices: &'static [&'static str], default: &'static str) -> ParamDef {
    ParamDef { name, label_fr, unit: "", vtype: ValueType::Choice, min: 0.0, max: 0.0, default: Lit::Choice(default), choices, editable: true, hidden: false }
}

const fn num_or_inf(name: &'static str, label_fr: &'static str, unit: &'static str, min: f64, max: f64) -> ParamDef {
    ParamDef { name, label_fr, unit, vtype: ValueType::NumberOrInf, min, max, default: Lit::Num(f64::INFINITY), choices: &[], editable: true, hidden: false }
}

/// Hidden model value (never sent to the UI, never edited): the default is a neutral value, the real one comes
/// from a mission bench file. Number, "" for a value in %VDD or %/%.
const fn hidden(name: &'static str, label_fr: &'static str, unit: &'static str, min: f64, max: f64, default: f64) -> ParamDef {
    ParamDef { name, label_fr, unit, vtype: ValueType::Number, min, max, default: Lit::Num(default), choices: &[], editable: false, hidden: true }
}

const fn state_bool(name: &'static str, label_fr: &'static str, default: bool) -> StateDef {
    StateDef { name, label_fr, unit: "", vtype: ValueType::Bool, min: 0.0, max: 1.0, default: Lit::Bool(default), persistent: true }
}

const fn state_transient(name: &'static str, label_fr: &'static str) -> StateDef {
    StateDef { name, label_fr, unit: "", vtype: ValueType::Bool, min: 0.0, max: 1.0, default: Lit::Bool(false), persistent: false }
}

const TWO_TERMINAL: [PinDef; 2] = [
    pin("p1", "1", "1", pt(-3, 0), Dir::L, Role::Passive, false),
    pin("p2", "2", "2", pt(3, 0), Dir::R, Role::Passive, false),
];

/// ADC sampling rates: the divisors of 1000 Hz (contract §2.1, sampled at the end of a 1 ms frame).
pub const SAMPLE_RATES_HZ: [i64; 16] = [1, 2, 4, 5, 8, 10, 20, 25, 40, 50, 100, 125, 200, 250, 500, 1000];

pub static LIBRARY: std::sync::LazyLock<Vec<KindDef>> = std::sync::LazyLock::new(|| {
    BASE_LIBRARY.iter().cloned().chain(mission_library()).collect()
});

include!("catalog_missions.rs");

static BASE_LIBRARY: [KindDef; 15] = [
    KindDef {
        kind: Kind::DcSupply,
        name_fr: "Alimentation DC",
        prefix: "V",
        body: rect(-2, -3, 2, 3),
        pins: &[
            pin("pos", "+", "+", pt(0, -4), Dir::U, Role::Source, false),
            pin("neg", "\u{2212}", "\u{2212}", pt(0, 4), Dir::D, Role::Source, false),
        ],
        params: &[
            num("voltage", "Tension de consigne", "V", 0.0, 30.0, 5.0),
            num("current_limit", "Limite de courant", "A", 0.001, 5.0, 0.1),
            num("r_out", "R\u{e9}sistance de sortie (mod\u{e8}le)", OHM, 0.001, 10.0, 0.01),
        ],
        states: &[state_bool("on", "Sortie active", true)],
        actuators: &[],
        placeable: true,
        sources_fr: "Alimentation de laboratoire un quadrant (CV/CC) : consigne et limite r\u{e9}glables ; r_out est un param\u{e8}tre du mod\u{e8}le (N7RT_MODEL).",
    },
    KindDef {
        kind: Kind::Ground,
        name_fr: "Masse",
        prefix: "",
        body: rect(-2, 1, 2, 2),
        pins: &[pin("gnd", "GND", "GND", pt(0, 0), Dir::U, Role::Ground, false)],
        params: &[],
        states: &[],
        actuators: &[],
        placeable: true,
        sources_fr: "R\u{e9}f\u{e9}rence 0 V : tous les symboles de masse forment le net GND.",
    },
    KindDef {
        kind: Kind::Resistor,
        name_fr: "R\u{e9}sistance",
        prefix: "R",
        body: rect(-2, -1, 2, 1),
        pins: &TWO_TERMINAL,
        params: &[
            num("resistance", "R\u{e9}sistance", OHM, 0.01, 1e8, 1e4),
            num("power_rating", "Puissance nominale", "W", 0.0625, 10.0, 0.25),
        ],
        states: &[],
        actuators: &[],
        placeable: true,
        sources_fr: "R\u{e9}sistance lin\u{e9}aire id\u{e9}ale ; surcharge si U\u{b2}/R d\u{e9}passe la puissance nominale.",
    },
    KindDef {
        kind: Kind::Capacitor,
        name_fr: "Condensateur",
        prefix: "C",
        body: rect(-2, -2, 2, 2),
        pins: &TWO_TERMINAL,
        params: &[
            num("capacitance", "Capacit\u{e9}", "F", 1e-12, 1.0, 1e-5),
            num("voltage_rating", "Tension nominale", "V", 1.0, 1000.0, 25.0),
            boolean("polarized", "Polaris\u{e9} (broche 1 = +)", false),
        ],
        states: &[],
        actuators: &[],
        placeable: true,
        sources_fr: "Condensateur id\u{e9}al d\u{e9}charg\u{e9} au RESET. Polaris\u{e9} : polarit\u{e9} inverse signal\u{e9}e sous \u{2212}0,5 V, r\u{e8}gle p\u{e9}dagogique prudente sans source constructeur (N7RT_MODEL).",
    },
    KindDef {
        kind: Kind::Potentiometer,
        name_fr: "Potentiom\u{e8}tre",
        prefix: "RV",
        body: rect(-2, -2, 2, 1),
        pins: &[
            pin("a", "A", "A", pt(-3, 0), Dir::L, Role::Passive, false),
            pin("b", "B", "B", pt(3, 0), Dir::R, Role::Passive, false),
            pin("w", "W (curseur)", "W", pt(0, -3), Dir::U, Role::Passive, false),
        ],
        params: &[
            num("resistance", "R\u{e9}sistance totale", OHM, 10.0, 1e7, 1e4),
            num("power_rating", "Puissance nominale", "W", 0.05, 5.0, 0.25),
        ],
        states: &[StateDef {
            name: "position",
            label_fr: "Position du curseur (0 = A)",
            unit: "",
            vtype: ValueType::Number,
            min: 0.0,
            max: 1.0,
            default: Lit::Num(0.5),
            persistent: true,
        }],
        actuators: &[Actuator { kind: ActuatorKind::DragPot, zone: rect(-2, -2, 2, -1) }],
        placeable: true,
        sources_fr: "Potentiom\u{e8}tre lin\u{e9}aire 3 bornes : R_AW = \u{3b1}\u{b7}R, R_WB = (1\u{2212}\u{3b1})\u{b7}R, position k/1000, plancher num\u{e9}rique de 1 m\u{3a9} par segment (N7RT_MODEL). Surcharge si le courant d'un segment d\u{e9}passe \u{221a}(P/R) : r\u{e8}gle de conception N7RT_MODEL, non v\u{e9}rifi\u{e9}e sur une fiche.",
    },
    KindDef {
        kind: Kind::PushButton,
        name_fr: "Bouton poussoir",
        prefix: "BP",
        body: rect(-2, -3, 2, 1),
        pins: &TWO_TERMINAL,
        params: &[
            choice("contact", "Contact", &["no", "nc"], "no"),
            choice("mode", "Fonctionnement", &["momentary", "latching"], "momentary"),
            num("r_on", "R\u{e9}sistance de contact", OHM, 0.001, 10.0, 0.1),
            num("r_off", "R\u{e9}sistance d'isolement", OHM, 1e6, 1e12, 1e8),
        ],
        states: &[state_transient("pressed", "Appuy\u{e9}"), state_bool("latched", "Verrouill\u{e9}", false)],
        actuators: &[Actuator { kind: ActuatorKind::PressButton, zone: rect(-1, -3, 1, -2) }],
        placeable: true,
        sources_fr: "Omron B3F (A070-E1-08, p. 3) : r\u{e9}sistance de contact 100 m\u{3a9} max, isolement 100 M\u{3a9} min (DATASHEET).",
    },
    KindDef {
        kind: Kind::Switch,
        name_fr: "Interrupteur",
        prefix: "S",
        body: rect(-2, -2, 2, 1),
        pins: &TWO_TERMINAL,
        params: &[
            num("r_on", "R\u{e9}sistance de contact", OHM, 0.001, 10.0, 0.01),
            num("r_off", "R\u{e9}sistance d'isolement", OHM, 1e6, 1e12, 1e9),
        ],
        states: &[state_bool("closed", "Ferm\u{e9}", false)],
        actuators: &[Actuator { kind: ActuatorKind::ToggleSwitch, zone: rect(-1, -2, 1, 0) }],
        placeable: true,
        sources_fr: "NKK s\u{e9}rie M, M2011 (catalogue p. A52) : contact 10 m\u{3a9} max (argent), isolement 1 000 M\u{3a9} min (DATASHEET).",
    },
    KindDef {
        kind: Kind::AdcInput,
        name_fr: "Entr\u{e9}e ADC",
        prefix: "ADC",
        body: rect(-2, -2, 2, 2),
        pins: &[
            pin("in", "IN", "IN", pt(-3, 0), Dir::L, Role::AnalogIn, true),
            pin("ref", "REF (masse locale)", "REF", pt(0, 3), Dir::D, Role::GroundPin, true),
        ],
        params: &[
            num("vref", "Tension de r\u{e9}f\u{e9}rence", "V", 1.0, 5.0, 3.3),
            int("bits", "R\u{e9}solution", "bit", 8, 16, 12),
            num_or_inf("r_in", "R\u{e9}sistance d'entr\u{e9}e (\u{221e} = id\u{e9}ale)", OHM, 1e3, 1e12),
            int("sample_rate", "Fr\u{e9}quence d'\u{e9}chantillonnage", "Hz", 1, 1000, 1000),
        ],
        states: &[],
        actuators: &[],
        placeable: true,
        sources_fr: "Convertisseur g\u{e9}n\u{e9}rique : code = floor(V\u{b7}2^bits/Vref) (architecture \u{a7}10.5) ; \u{e9}chantillonneur-bloqueur en fin de pas de 1 ms. Entr\u{e9}e id\u{e9}ale (\u{221e}) par d\u{e9}faut : non reli\u{e9}e, elle est flottante. Une r_in finie est une vraie r\u{e9}sistance entre IN et REF.",
    },
    KindDef {
        kind: Kind::EcuStm32,
        name_fr: "ECU STM32 (NUCLEO-H753ZI)",
        prefix: "U",
        body: rect(-7, -6, 7, 6),
        pins: &[
            pin("v5", "5V (CN8-9)", "5V", pt(-8, -4), Dir::L, Role::Source, false),
            pin("v3v3", "3V3 (CN8-7)", "3V3", pt(-8, -2), Dir::L, Role::Source, false),
            pin("gnd", "GND (CN9-23)", "GND", pt(-8, 4), Dir::L, Role::GroundPin, true),
            pin("a0", "A0 PA3 (CN9-1)", "A0", pt(8, -4), Dir::R, Role::AnalogIn, false),
            pin("a1", "A1 PC0 (CN9-3)", "A1", pt(8, -2), Dir::R, Role::AnalogIn, false),
            pin("can_tx", "PD1 CAN_TX (CN9-27)", "PD1", pt(8, 2), Dir::R, Role::LogicOut, false),
            pin("can_rx", "PD0 CAN_RX (CN9-25)", "PD0", pt(8, 4), Dir::R, Role::LogicIn, false),
        ],
        params: &[
            choice("adc_bits", "R\u{e9}solution ADC", &["8", "10", "12", "14", "16"], "12"),
            int("adc_sample_rate", "Fr\u{e9}quence d'\u{e9}chantillonnage ADC", "Hz", 1, 1000, 1000),
            choice("can_mode", "Broches FDCAN", &["can", "hi_z"], "can"),
            num("r_out_5v", "R\u{e9}sistance de sortie 5V (mod\u{e8}le)", OHM, 0.001, 10.0, 0.1),
            num("r_out_3v3", "R\u{e9}sistance de sortie 3V3 (mod\u{e8}le)", OHM, 0.001, 10.0, 0.05),
        ],
        states: &[state_bool("usb", "Carte aliment\u{e9}e (USB)", true)],
        actuators: &[],
        placeable: true,
        sources_fr: "UM2407 Rev 4 (tables 19-20 : broches ; p. 20 : 5V limit\u{e9} \u{e0} 500 mA, 300 mA pour les shields) ; ST1L05 (3V3, limite \u{2265} 1,3 A) ; DS12117 Rev 9 (seuils 0,3/0,7\u{b7}VDD, sortie \u{2265} VDD \u{2212} 0,4 V \u{e0} 8 mA, broches FT \u{2264} min(VDD, VDDA, VDD33USB, VBAT) + 4,0 V, ADC 8 \u{e0} 16 bits). R\u{e9}sistances de sortie des rails : param\u{e8}tres du mod\u{e8}le (N7RT_MODEL).",
    },
    KindDef {
        kind: Kind::CanTransceiver,
        name_fr: "Transceiver CAN (MCP2551)",
        prefix: "U",
        body: rect(-6, -7, 6, 7),
        pins: &[
            pin("txd", "1 TXD", "TXD", pt(-7, -4), Dir::L, Role::LogicIn, false),
            pin("vss", "2 VSS", "VSS", pt(0, 8), Dir::D, Role::GroundPin, true),
            pin("vdd", "3 VDD", "VDD", pt(0, -8), Dir::U, Role::PowerIn, true),
            pin("rxd", "4 RXD", "RXD", pt(-7, -2), Dir::L, Role::LogicOut, false),
            pin("vref", "5 VREF", "VREF", pt(7, 4), Dir::R, Role::Source, false),
            pin("canl", "6 CANL", "CANL", pt(7, 2), Dir::R, Role::Canl, false),
            pin("canh", "7 CANH", "CANH", pt(7, -2), Dir::R, Role::Canh, false),
            pin("rs", "8 Rs", "Rs", pt(-7, 2), Dir::L, Role::AnalogIn, false),
        ],
        params: &[num("r_in", "R_IN (entr\u{e9}e mode commun)", OHM, 1e4, 5e4, 2.5e4)],
        states: &[],
        actuators: &[],
        placeable: true,
        sources_fr: "Microchip DS20001667G Rev G (MCP2551) : VDD 4,5-5,5 V, max absolu 7,0 V ; POR 3,8-4,3 / 3,4-4,0 V (seuils retenus 4,05 / 3,70 V : N7RT_MODEL) ; IDD r\u{e9}cessif \u{2264} 10 mA (R_DD = 500 \u{3a9}, pessimiste) ; R_IN 5-50 k\u{3a9} et R_DIFF 20-100 k\u{3a9} (r\u{e9}seau permanent) ; pull-up TXD 25 k\u{3a9} nominal ; VIL 0,8 V / VIH 2,0 V ; RXD \u{2265} 0,7\u{b7}VDD \u{e0} 8 mA (r_out 187,5 \u{3a9}) ; VREF 0,45-0,55\u{b7}VDD \u{e0} \u{b1}50 \u{b5}A (r_out 5 k\u{3a9}) ; bandes de Rs ; minuterie de dominant permanent 1,25-4 ms. Composant NRND (MCP2561 recommand\u{e9}).",
    },
    KindDef {
        kind: Kind::CanBus,
        name_fr: "Bus CAN (reste du v\u{e9}hicule)",
        prefix: "BUS",
        body: rect(-3, -3, 9, 5),
        pins: &[
            pin("canh", "CANH", "CANH", pt(-4, -2), Dir::L, Role::Canh, false),
            pin("canl", "CANL", "CANL", pt(-4, 2), Dir::L, Role::Canl, false),
            pin("gnd", "GND (masse des n\u{153}uds distants)", "GND", pt(-4, 4), Dir::L, Role::GroundPin, true),
        ],
        params: &[
            int("remote_nodes", "N\u{153}uds distants aliment\u{e9}s", "", 0, 30, 1),
            num("remote_r_in", "R_IN des n\u{153}uds distants", OHM, 1e4, 5e4, 2.5e4),
            boolean("remote_termination", "Terminaison 120 \u{3a9} \u{e0} l'extr\u{e9}mit\u{e9} distante", true),
        ],
        states: &[],
        actuators: &[],
        placeable: true,
        sources_fr: "Mod\u{e8}le du reste du bus : n transceivers aliment\u{e9}s par leur carte \u{e0} 5,0 V, chaque ligne reli\u{e9}e par R_IN/n \u{e0} un n\u{153}ud de polarisation \u{e0} VDD/2 = 2,5 V (DS20001667G), terminaison distante 120 \u{3a9} (ISO 11898-2). Hypoth\u{e8}ses N7RT_MODEL.",
    },
    KindDef {
        kind: Kind::Termination,
        name_fr: "Terminaison 120 \u{3a9}",
        prefix: "RT",
        body: rect(-2, -1, 2, 1),
        pins: &TWO_TERMINAL,
        params: &[fixed("resistance", "R\u{e9}sistance", OHM, 120.0), fixed("power_rating", "Puissance nominale", "W", 0.25)],
        states: &[],
        actuators: &[],
        placeable: true,
        sources_fr: "Terminaison nominale 120 \u{3a9} (ISO 11898-2), 0,25 W.",
    },
    KindDef {
        kind: Kind::EcuApps,
        name_fr: "ECU APPS (bo\u{ee}te noire)",
        prefix: "U",
        body: rect(-7, -10, 7, 10),
        pins: &[
            pin("lv_in", "LV_IN (alimentation LV)", "LV_IN", pt(8, -8), Dir::R, Role::PowerIn, true),
            pin("gnd", "GND (masse)", "GND", pt(8, -6), Dir::R, Role::GroundPin, true),
            pin("v5_1", "5V_1 (alimentation du capteur 1)", "5V_1", pt(-8, -9), Dir::L, Role::Source, false),
            pin("sig_1", "SIG_1 (signal 1)", "SIG_1", pt(-8, -5), Dir::L, Role::AnalogIn, false),
            pin("gnd_s1", "GND_S1 (masse du capteur 1)", "GND_S1", pt(-8, -3), Dir::L, Role::GroundPin, false),
            pin("v5_2", "5V_2 (alimentation du capteur 2)", "5V_2", pt(-8, 2), Dir::L, Role::Source, false),
            pin("sig_2", "SIG_2 (signal 2)", "SIG_2", pt(-8, 6), Dir::L, Role::AnalogIn, false),
            pin("gnd_s2", "GND_S2 (masse du capteur 2)", "GND_S2", pt(-8, 8), Dir::L, Role::GroundPin, false),
            pin("can_h", "CAN_H", "CAN_H", pt(8, 3), Dir::R, Role::Canh, false),
            pin("can_l", "CAN_L", "CAN_L", pt(8, 7), Dir::R, Role::Canl, false),
        ],
        params: &[
            hidden("r_ecu", "Consommation propre (r\u{e9}sistance \u{e9}quivalente)", OHM, 1.0, 1e6, 1e3),
            hidden("reset_on", "Sortie de reset (LV_IN \u{2212} GND)", "V", 0.5, 30.0, 6.5),
            hidden("reset_off", "Entr\u{e9}e en reset (LV_IN \u{2212} GND)", "V", 0.5, 30.0, 6.0),
            hidden("rail_v", "Consigne des rails capteur", "V", 0.0, 30.0, 5.0),
            hidden("rail_r_out", "R\u{e9}sistance de sortie des rails", OHM, 0.001, 100.0, 0.05),
            hidden("rail_i_lim", "Limite de courant des rails", "A", 0.001, 5.0, 0.1),
            hidden("sig_r_high", "Diviseur SIG : r\u{e9}sistance haute", OHM, 1.0, 1e8, 15e3),
            hidden("sig_r_low", "Diviseur SIG : r\u{e9}sistance basse", OHM, 1.0, 1e8, 10e3),
            hidden("sig_c", "Filtre anti-repliement", "F", 1e-12, 1e-3, 2.2e-7),
            hidden("sup_r_high", "Mesure des rails : r\u{e9}sistance haute", OHM, 1.0, 1e8, 20e3),
            hidden("sup_r_low", "Mesure des rails : r\u{e9}sistance basse", OHM, 1.0, 1e8, 10e3),
        ],
        states: &[],
        actuators: &[],
        placeable: true,
        sources_fr: "ECU synth\u{e9}tique N7RT : bo\u{ee}te noire, aucun microcontr\u{f4}leur r\u{e9}el n'est mod\u{e9}lis\u{e9} ; seuls ses connecteurs sont visibles (LV_IN, GND, 5V_1, SIG_1, GND_S1, 5V_2, SIG_2, GND_S2, CAN_H, CAN_L ; GND_S1 et GND_S2 reli\u{e9}es \u{e0} GND dans l'ECU). Son conditionnement interne (consommation, reset, rails capteur limit\u{e9}s, diviseurs, filtres, mesures, ADC) n'est pas publi\u{e9}. Interface CAN : c\u{f4}t\u{e9} bus du mod\u{e8}le MCP2551 (DS20001667G), r\u{e9}cessif, aliment\u{e9}e hors reset seulement.",
    },
    KindDef {
        kind: Kind::Mlx90377,
        name_fr: "Capteur de position double puce",
        prefix: "U",
        body: rect(-6, -10, 6, 10),
        pins: &[
            pin("vdd1", "VDD1 (broche 3, puce 1)", "VDD1", pt(7, -9), Dir::R, Role::PowerIn, false),
            pin("vss1", "VSS1 (broche 2, puce 1)", "VSS1", pt(7, -3), Dir::R, Role::GroundPin, false),
            pin("out1", "OUT1 (broche 15, puce 1)", "OUT1", pt(7, -5), Dir::R, Role::Source, false),
            pin("vdd2", "VDD2 (broche 11, puce 2)", "VDD2", pt(7, 2), Dir::R, Role::PowerIn, false),
            pin("vss2", "VSS2 (broche 10, puce 2)", "VSS2", pt(7, 8), Dir::R, Role::GroundPin, false),
            pin("out2", "OUT2 (broche 6, puce 2)", "OUT2", pt(7, 6), Dir::R, Role::Source, false),
        ],
        params: &[
            hidden("r_idd", "Consommation par puce (r\u{e9}sistance \u{e9}quivalente)", OHM, 1.0, 1e6, 500.0),
            hidden("por_on", "Seuil de d\u{e9}marrage (VDD \u{2212} VSS, montant)", "V", 0.5, 30.0, 4.0),
            hidden("por_off", "Seuil d'arr\u{ea}t (VDD \u{2212} VSS, descendant)", "V", 0.5, 30.0, 3.8),
            hidden("r_out", "R\u{e9}sistance de sortie", OHM, 0.001, 1e4, 10.0),
            hidden("r_leak", "Fuite de sortie en haute imp\u{e9}dance", OHM, 1e3, 1e12, 2e6),
            hidden("c_vdd", "D\u{e9}couplage VDD\u{2013}VSS", "F", 1e-12, 1e-3, 1e-7),
            hidden("output_i_lim", "Limite de courant de sortie", "A", 0.0, 1.0, 0.0),
            hidden("c_out", "Condensateur OUT\u{2013}VSS", "F", 1e-12, 1e-3, 4.7e-8),
            hidden("r_iso", "Isolation entre puces (VSS1\u{2013}VSS2)", OHM, 1e3, 1e12, 1e7),
            hidden("tf1_offset", "Puce 1 : sortie au repos (%VDD)", "", -100.0, 200.0, 10.0),
            hidden("tf1_slope", "Puce 1 : pente (%VDD par % de course)", "", -100.0, 100.0, 0.5),
            hidden("tf1_clamp_low", "Puce 1 : \u{e9}cr\u{ea}tage bas (%VDD)", "", 0.0, 100.0, 0.0),
            hidden("tf1_clamp_high", "Puce 1 : \u{e9}cr\u{ea}tage haut (%VDD)", "", 0.0, 100.0, 100.0),
            hidden("tf2_offset", "Puce 2 : sortie au repos (%VDD)", "", -100.0, 200.0, 10.0),
            hidden("tf2_slope", "Puce 2 : pente (%VDD par % de course)", "", -100.0, 100.0, 0.5),
            hidden("tf2_clamp_low", "Puce 2 : \u{e9}cr\u{ea}tage bas (%VDD)", "", 0.0, 100.0, 0.0),
            hidden("tf2_clamp_high", "Puce 2 : \u{e9}cr\u{ea}tage haut (%VDD)", "", 0.0, 100.0, 100.0),
        ],
        states: &[StateDef {
            name: "position",
            label_fr: "Course de la p\u{e9}dale (0 = repos, 1 = pleine course)",
            unit: "",
            vtype: ValueType::Number,
            min: 0.0,
            max: 1.0,
            default: Lit::Num(0.0),
            persistent: true,
        }],
        actuators: &[Actuator { kind: ActuatorKind::DragPot, zone: rect(-2, -1, 2, 1) }],
        placeable: true,
        sources_fr: "Pi\u{e8}ce r\u{e9}elle marqu\u{e9}e 377ADB, deux puces ind\u{e9}pendantes : brochage, isolation entre puces, seuil de d\u{e9}marrage, consommation, charge et fuite de sortie, sortie analogique ratiom\u{e9}trique (%VDD) avec \u{e9}cr\u{ea}tages programmables et circuit recommand\u{e9} tir\u{e9}s de la fiche du fabricant de cette pi\u{e8}ce. Mod\u{e8}le par puce : consommation, porte de d\u{e9}marrage, source ratiom\u{e9}trique \u{e9}cr\u{ea}t\u{e9}e, fuite en haute imp\u{e9}dance, d\u{e9}couplages (N7RT_MODEL) ; limitation du courant de sortie et d\u{e9}lai de d\u{e9}marrage non mod\u{e9}lis\u{e9}s. Curseur : course de la p\u{e9}dale.",
    },
    KindDef {
        kind: Kind::Junction,
        name_fr: "Jonction",
        prefix: "",
        body: rect(0, 0, 0, 0),
        pins: &[pin("j", "J", "J", pt(0, 0), Dir::R, Role::Passive, false)],
        params: &[],
        states: &[],
        actuators: &[],
        placeable: false,
        sources_fr: "Point de net cr\u{e9}\u{e9} en reliant un fil au milieu d'un autre ; supprim\u{e9} par le nettoyage.",
    },
];
