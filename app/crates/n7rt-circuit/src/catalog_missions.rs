// Generic physical components. Hidden defaults are neutral; operating values come from the loaded bench.
const fn world_state(name: &'static str, label: &'static str, unit: &'static str, min: f64, max: f64) -> StateDef {
    StateDef { name, label_fr: label, unit, vtype: ValueType::Number, min, max, default: Lit::Num(0.0), persistent: true }
}
const fn positive_model(name: &'static str, default: f64) -> ParamDef { hidden(name, "Paramètre du modèle", "", 1e-12, 1e12, default) }
const fn model(name: &'static str) -> ParamDef { hidden(name, "Paramètre du modèle", "", -1e12, 1e12, 0.0) }
const fn hidden_bool(name: &'static str) -> ParamDef {
    ParamDef { name, label_fr: "Paramètre du modèle", unit: "", vtype: ValueType::Bool,
        min: 0.0, max: 1.0, default: Lit::Bool(false), choices: &[], editable: false, hidden: true }
}
const fn wheel_side() -> ParamDef {
    ParamDef { name: "wheel_side", label_fr: "Côté du train", unit: "", vtype: ValueType::Choice,
        min: 0.0, max: 0.0, default: Lit::Choice("left"), choices: &["left", "right"], editable: false, hidden: true }
}
const fn world_ref() -> ParamDef {
    ParamDef { name: "world_comp_id", label_fr: "Commande mécanique", unit: "", vtype: ValueType::Int,
        min: 0.0, max: u32::MAX as f64, default: Lit::Int(0), choices: &[], editable: false, hidden: true }
}
const fn mission_def(kind: Kind, name: &'static str, prefix: &'static str, pins: &'static [PinDef],
    params: &'static [ParamDef], states: &'static [StateDef]) -> KindDef {
    KindDef { kind, name_fr: name, prefix, body: match kind { Kind::EcuWsf => rect(-7,-9,7,9), Kind::Ats682Lsh => rect(-3,-3,3,3), Kind::PressureSwitch => rect(-2,-1,2,1), _ => rect(-4,-5,4,5) }, pins, params, states, actuators: &[],
        placeable: true, sources_fr: "Composant physique générique paramétré par le banc ; paramètres internes cachés en mission." }
}
const MISSION_LIBRARY: [KindDef; 10] = [
        mission_def(Kind::EcuWsf, "ECU vitesse de roues", "U", &[
            pin("lv_in","LV_IN","LV",pt(8,-8),Dir::R,Role::PowerIn,true),
            pin("gnd","GND","GND",pt(8,-6),Dir::R,Role::GroundPin,true),
            pin("ws_l_sup","Alimentation gauche","L+",pt(-8,-5),Dir::L,Role::Source,true),
            pin("ws_l_in","Retour gauche","L−",pt(-8,-3),Dir::L,Role::AnalogIn,true),
            pin("ws_r_sup","Alimentation droite","R+",pt(-8,5),Dir::L,Role::Source,true),
            pin("ws_r_in","Retour droite","R−",pt(-8,7),Dir::L,Role::AnalogIn,true),
            pin("can_h","CAN_H","H",pt(8,3),Dir::R,Role::Canh,true),
            pin("can_l","CAN_L","L",pt(8,7),Dir::R,Role::Canl,true),
        ], &[
            positive_model("r_ecu",1.0),model("rail_v"),positive_model("rail_r_out",1.0),positive_model("rail_i_lim",1e-3),
            model("reset_on"),model("reset_off"),positive_model("r_sense",1.0),positive_model("r_sense_rating",1.0),model("boot_ms"),
        ], &[]),
        mission_def(Kind::EcuSas, "ECU angle de volant", "U", &[
            pin("lv_in","LV_IN","LV",pt(-5,-4),Dir::L,Role::PowerIn,true),
            pin("gnd","GND","GND",pt(-5,4),Dir::L,Role::GroundPin,true),
            pin("s5v","Alimentation capteur","5V",pt(-5,-2),Dir::L,Role::Source,true),
            pin("sgnd","Retour capteur","S−",pt(-5,2),Dir::L,Role::GroundPin,true),
            pin("sent","SENT","SENT",pt(-5,0),Dir::L,Role::LogicIn,true),
            pin("can_h","CAN_H","H",pt(5,3),Dir::R,Role::Canh,true),
            pin("can_l","CAN_L","L",pt(5,4),Dir::R,Role::Canl,true),
        ], &[
            positive_model("r_ecu",1.0),model("rail_v"),positive_model("rail_r_out",1.0),positive_model("rail_i_lim",1e-3),
            model("reset_on"),model("reset_off"),positive_model("sig_r_top",1.0),positive_model("sig_r_bottom",1.0),
            positive_model("sig_c",1e-9),positive_model("c_receiver",1e-9),positive_model("sup_r_top",1.0),positive_model("sup_r_bottom",1.0),positive_model("pullup_r",1.0),model("logic_v"),model("boot_ms"),model("adc_bits"),positive_model("adc_vref",1.0),
        ], &[]),
        mission_def(Kind::EcuBps, "ECU pression de freinage", "U", &[
            pin("lv_in","LV_IN","LV",pt(-5,-4),Dir::L,Role::PowerIn,true),
            pin("gnd","GND","GND",pt(-5,4),Dir::L,Role::GroundPin,true),
            pin("v5_f","Alimentation avant","F+",pt(-5,-2),Dir::L,Role::Source,true),
            pin("sig_f","Signal avant","SIG F",pt(-5,0),Dir::L,Role::AnalogIn,true),
            pin("gnd_f","Retour avant","F−",pt(-5,2),Dir::L,Role::GroundPin,true),
            pin("v5_r","Alimentation arrière","R+",pt(5,-2),Dir::R,Role::Source,true),
            pin("sig_r","Signal arrière","SIG R",pt(5,0),Dir::R,Role::AnalogIn,true),
            pin("gnd_r","Retour arrière","R−",pt(5,2),Dir::R,Role::GroundPin,true),
            pin("sw_in","Contact frein","SW",pt(0,-6),Dir::U,Role::LogicIn,true),
            pin("sw_ret","Retour contact","SW−",pt(0,6),Dir::D,Role::GroundPin,true),
            pin("can_h","CAN_H","H",pt(5,3),Dir::R,Role::Canh,true),
            pin("can_l","CAN_L","L",pt(5,4),Dir::R,Role::Canl,true),
        ], &[
            positive_model("r_ecu",1.0),model("rail_v"),positive_model("rail_r_out",1.0),positive_model("rail_i_lim",1e-3),
            model("reset_on"),model("reset_off"),positive_model("sig_r_high",1.0),positive_model("sig_r_low",1.0),
            positive_model("sig_c",1e-9),positive_model("sup_r_high",1.0),positive_model("sup_r_low",1.0),positive_model("sw_pullup_ohm",1.0),
            model("sw_logic_v"),model("boot_ms"),model("adc_bits"),positive_model("adc_vref",1.0),
        ], &[]),
        mission_def(Kind::Ats682Lsh, "Capteur de vitesse de roue", "U", &[
            pin("vcc","Alimentation","V+",pt(4,-2),Dir::R,Role::PowerIn,true),
            pin("gnd","Retour","V−",pt(4,2),Dir::R,Role::GroundPin,true),
        ], &[
            world_ref(),wheel_side(),model("r_eff_m"),model("wheelbase_m"),
            model("track_front_m"),model("steer_ratio"),model("teeth_count"),
            model("i_low"),model("i_high"),model("duty"),model("power_on_ms"),
            model("vdd_min"),model("vdd_max"),model("reset_below_v"),positive_model("r_reverse",1.0),
            model("gap_nominal_mm"),model("gap_failure_mm"),
            model("i_low_min"),model("i_low_max"),model("i_high_min"),model("i_high_max"),model("duty_min"),model("duty_max"),
            num("air_gap_mm","Entrefer","mm",0.0,20.0,0.0),
            num("tolerance_corner","Tolérance du capteur","",-1.0,1.0,0.0),
        ], &[]),
        mission_def(Kind::Tle5014, "Capteur d'angle SENT", "U", &[
            pin("vdd","Alimentation","V+",pt(-5,-2),Dir::L,Role::PowerIn,true),
            pin("gnd","Retour","GND",pt(-5,2),Dir::L,Role::GroundPin,true),
            pin("out","Sortie SENT","OUT",pt(5,0),Dir::R,Role::LogicOut,true),
        ], &[
            world_ref(),model("consumption_a"),model("reset_on"),model("reset_off"),
            positive_model("r_out",1.0),positive_model("r_leak",1.0),positive_model("pullup_r",1.0),model("ut_us"),model("phase_us"),
            model("boot_ms"),model("data_offset"),model("mounting_direction"),model("error_deg"),
            positive_model("c_vdd",1e-9),positive_model("c_out",1e-9),
            model("overvoltage_v"),model("abs_max_v"),model("out_overvoltage_margin_v"),
            model("fault_angle_deg"),hidden_bool("sensor_error"),
        ], &[]),
        mission_def(Kind::Pss260, "Capteur de pression", "U", &[
            pin("us","Alimentation","V+",pt(-5,-2),Dir::L,Role::PowerIn,true),
            pin("gnd","Retour","GND",pt(-5,2),Dir::L,Role::GroundPin,true),
            pin("sig","Signal","SIG",pt(5,0),Dir::R,Role::Source,true),
        ], &[
            world_ref(),hidden_bool("front"),model("ratio_offset"),model("ratio_span"),
            positive_model("pressure_full_scale_bar",1.0),model("atmospheric_bar"),model("instance_error_ratio"),
            positive_model("r_out",1.0),positive_model("output_i_lim",1e-3),positive_model("r_idd",1.0),model("abs_max_v"),
        ], &[]),
        mission_def(Kind::PressureSwitch, "Contact de pression", "S", &TWO_TERMINAL, &[
            world_ref(),hidden_bool("front"),model("close_bar"),model("hysteresis_bar"),
            positive_model("r_on",1.0),positive_model("r_off",1.0),
        ], &[]),
        mission_def(Kind::FrontAxle,"Train avant","M",&[],&[], &[
            world_state("speed_mps","Vitesse véhicule","m/s",0.0,100.0),
            world_state("steering_deg","Angle de volant","°",-360.0,360.0),
        ]),
        mission_def(Kind::SteeringColumn,"Colonne de direction","M",&[],&[
            num("min_angle_deg","Butée minimale","°",-360.0,360.0,-360.0),
            num("max_angle_deg","Butée maximale","°",-360.0,360.0,360.0),
            num("max_rate_deg_per_s","Vitesse mécanique maximale","°/s",1.0,10000.0,360.0),
        ], &[
            world_state("angle_deg","Angle de volant","°",-360.0,360.0),
        ]),
        mission_def(Kind::BrakeHydraulics,"Commande de freinage","M",&[], &[
            num("beta","Rapport hydraulique arrière/avant","",0.0,2.0,1.0),
            num("min_pressure_bar","Pression minimale","bar",0.0,250.0,0.0),
            num("max_pressure_bar","Pression maximale","bar",0.0,250.0,250.0),
        ], &[
            world_state("pressure_bar","Pression avant commandée","bar",0.0,250.0),
        ]),
    ];
fn mission_library() -> Vec<KindDef> { MISSION_LIBRARY.to_vec() }

