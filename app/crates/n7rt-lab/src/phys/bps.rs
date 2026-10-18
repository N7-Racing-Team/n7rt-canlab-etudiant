//! Mechanical pressure and sensor laws. Electrical loading, current limits, ADC and contact
//! resistance belong to the common solver; these helpers never supply an ADC or a bus value.

use n7rt_circuit::{Circuit, CompId, Component, Kind};

use super::lower::{param_bool, param_int, param_num, state_num};

/// Local hydraulic pressure obtained from the mechanical world component referenced by the part.
/// A missing or invalid world is unavailable, rather than an invented zero-pressure measurement.
pub(crate) fn pressure(circuit: &Circuit, part: &Component) -> Option<f64> {
    let id = u32::try_from(param_int(part, "world_comp_id")).ok()?;
    let world = circuit.components().get(&CompId(id))?;
    if world.kind != Kind::BrakeHydraulics { return None; }
    let lo = param_num(world, "min_pressure_bar");
    let hi = param_num(world, "max_pressure_bar");
    let beta = param_num(world, "beta");
    let command = state_num(circuit, world, "pressure_bar");
    if ![lo, hi, beta, command].iter().all(|v| v.is_finite()) || lo > hi || beta < 0.0 { return None; }
    let front = command.clamp(lo, hi);
    let pressure = if param_bool(part, "front") { front } else { beta * front };
    pressure.is_finite().then_some(pressure)
}

/// Ratiometric voltage command relative to the sensor's own ground. The solver applies this
/// gain to V(us)-V(gnd), behind the part's resistance and bidirectional current limit.
pub(crate) fn ratio(circuit: &Circuit, sensor: &Component) -> Option<f64> {
    let pressure = pressure(circuit, sensor)?;
    let offset = param_num(sensor, "ratio_offset");
    let span = param_num(sensor, "ratio_span");
    let full_scale = param_num(sensor, "pressure_full_scale_bar");
    let atmospheric = param_num(sensor, "atmospheric_bar");
    let error = param_num(sensor, "instance_error_ratio");
    if ![offset, span, full_scale, atmospheric, error].iter().all(|v| v.is_finite()) || full_scale <= 0.0 {
        return None;
    }
    let ratio = offset + span * (pressure + atmospheric) / full_scale + error;
    ratio.is_finite().then_some(ratio)
}

/// Hydraulic NO contact with hysteresis. The kernel retains `was_closed` between frames;
/// the result selects a real resistance, not the voltage observed by the ECU input.
pub(crate) fn contact_closed(circuit: &Circuit, switch: &Component, was_closed: bool) -> Option<bool> {
    let pressure = pressure(circuit, switch)?;
    let close = param_num(switch, "close_bar");
    let hysteresis = param_num(switch, "hysteresis_bar");
    if !close.is_finite() || !hysteresis.is_finite() || hysteresis < 0.0 { return None; }
    Some(if pressure >= close { true } else if pressure < close - hysteresis { false } else { was_closed })
}
