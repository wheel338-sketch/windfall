//! Stable opt-in route to Windows' existing OS General MIDI output. Uses the
//! same native MIDI worker, message queue and program/bank controls as MidiOut.
use windfall_ipc::MidiPort;

pub const SYSTEM_GM_OUTPUT: &str = "windfall:system-general-midi";
pub fn is_system_synth_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("Microsoft GS Wavetable Synth")
        || name.eq_ignore_ascii_case("Microsoft GS Wavetable SW Synth")
}
pub fn with_system_alias(mut outputs: Vec<MidiPort>) -> Vec<MidiPort> {
    if outputs.iter().any(|port| is_system_synth_name(&port.name)) {
        outputs.push(MidiPort {
            id: SYSTEM_GM_OUTPUT.into(),
            name: "System General MIDI synthesizer".into(),
        });
    }
    outputs
}
pub fn resolve_system_id(outputs: &[MidiPort]) -> Result<&str, &'static str> {
    outputs.iter().find(|port|is_system_synth_name(&port.name)).map(|port|port.id.as_str())
        .ok_or("The operating system General MIDI synthesizer is unavailable; no hardware fallback was selected.")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn system_route_resolves_fresh_ids_and_never_selects_external_hardware() {
        let hardware = MidiPort {
            id: "usb:1".into(),
            name: "USB keyboard".into(),
        };
        assert!(resolve_system_id(std::slice::from_ref(&hardware)).is_err());
        assert_eq!(with_system_alias(vec![hardware.clone()]).len(), 1);
        let synth = MidiPort {
            id: "os:3".into(),
            name: "Microsoft GS Wavetable Synth".into(),
        };
        let outputs = with_system_alias(vec![hardware, synth]);
        assert_eq!(resolve_system_id(&outputs), Ok("os:3"));
        assert_eq!(outputs.last().unwrap().id, SYSTEM_GM_OUTPUT);
        assert!(!is_system_synth_name("A device called Wavetable"));
    }
}
