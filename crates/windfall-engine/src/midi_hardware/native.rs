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
        let output = MidiOutput::new("Windfall MIDI output").map_err(|e| e.to_string())?;
        let port = output
            .find_port_by_id(id)
            .ok_or("MIDI output is unavailable.")?;
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
