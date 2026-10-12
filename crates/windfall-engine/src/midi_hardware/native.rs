use super::{Ingress, Output, Ports};
use midir::{Ignore, MidiInput, MidiOutput, MidiOutputConnection};
use windfall_ipc::MidiPort;

pub(super) struct Native;

impl Ports for Native {
    fn enumerate(&mut self) -> Result<(Vec<MidiPort>, Vec<MidiPort>), String> {
        let input = MidiInput::new("Windfall MIDI input").map_err(|e| e.to_string())?;
        let output = MidiOutput::new("Windfall MIDI output").map_err(|e| e.to_string())?;
        let inputs = input
            .ports()
            .iter()
            .map(|port| {
                input
                    .port_name(port)
                    .map(|name| MidiPort {
                        id: port.id(),
                        name,
                    })
                    .map_err(|e| e.to_string())
            })
            .collect::<Result<_, _>>()?;
        let outputs = output
            .ports()
            .iter()
            .map(|port| {
                output
                    .port_name(port)
                    .map(|name| MidiPort {
                        id: port.id(),
                        name,
                    })
                    .map_err(|e| e.to_string())
            })
            .collect::<Result<_, _>>()?;
        #[cfg(target_os = "windows")]
        let outputs = super::general_midi::with_system_alias(outputs);
        #[cfg(all(target_os = "windows", feature = "xinput"))]
        let outputs = {
            let mut outputs = outputs;
            outputs.push(MidiPort {
                id: super::feedback::CONTROLLER_FEEDBACK_OUTPUT.into(),
                name: "Game controller feedback (XInput)".into(),
            });
            outputs.push(MidiPort { id: super::feedback::PROJECT_FEEDBACK_OUTPUT.into(), name: "Project game controller feedback (XInput)".into() });
            outputs
        };
        Ok((inputs, outputs))
    }

    fn input(&mut self, id: &str, ingress: Ingress) -> Result<Box<dyn Send>, String> {
        let mut input = MidiInput::new("Windfall MIDI input").map_err(|e| e.to_string())?;
        // Timing messages are read: they carry a device's clock.
        input.ignore(Ignore::Sysex | Ignore::ActiveSense);
        let port = input
            .find_port_by_id(id)
            .ok_or("MIDI input is unavailable.")?;
        input
            .connect(
                &port,
                "Windfall audition",
                |stamp, bytes, ingress| ingress.receive_at(stamp, bytes),
                ingress,
            )
            .map(|connection| Box::new(connection) as Box<dyn Send>)
            .map_err(|e| e.to_string())
    }

    fn output(&mut self, id: &str) -> Result<Box<dyn Output>, String> {
        #[cfg(all(target_os = "windows", feature = "xinput"))]
        if id == super::feedback::CONTROLLER_FEEDBACK_OUTPUT {
            return Ok(Box::new(super::feedback::FeedbackOutput::new(
                crate::peripherals::windows::XInputAdapter,
            )));
        }
        if id == super::feedback::PROJECT_FEEDBACK_OUTPUT {
            #[cfg(all(target_os = "windows", feature = "xinput"))]
            { return Ok(Box::new(super::feedback::ProjectFeedbackOutput::new(crate::peripherals::windows::XInputAdapter))); }
            #[cfg(not(all(target_os = "windows", feature = "xinput")))]
            { return Err("Project controller feedback requires a Windows build with the xinput feature".into()); }
        }
        let output = MidiOutput::new("Windfall MIDI output").map_err(|e| e.to_string())?;
        let port = if id == super::general_midi::SYSTEM_GM_OUTPUT {
            if !cfg!(target_os = "windows") {
                return Err("System General MIDI output is only available on Windows".into());
            }
            output
                .ports()
                .into_iter()
                .find(|port| {
                    output
                        .port_name(port)
                        .is_ok_and(|name| super::general_midi::is_system_synth_name(&name))
                })
                .ok_or("The operating system General MIDI synthesizer is unavailable.")?
        } else {
            output
                .find_port_by_id(id)
                .ok_or("MIDI output is unavailable.")?
        };
        output
            .connect(&port, "Windfall live notes")
            .map(|connection| Box::new(connection) as Box<dyn Output>)
            .map_err(|e| e.to_string())
    }
}

impl Output for MidiOutputConnection {
    fn send(&mut self, bytes: &[u8]) -> Result<(), String> {
        MidiOutputConnection::send(self, bytes).map_err(|e| e.to_string())
    }
}
