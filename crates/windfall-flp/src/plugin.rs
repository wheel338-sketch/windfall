//! What can be read out of a plugin's saved state.
//!
//! FL Studio stores whatever a plugin hands it (event 213). For plugins it
//! hosts, the state is a wrapper around the plugin's own data, and the
//! wrapper says which plugin it is. For FL Studio's own plugins the state
//! is a row of 32-bit numbers, different for each plugin.
//!
//! Sources: PyFLP `pyflp/plugin.py` (the wrapper's records and their
//! numbers), DawVert `functions_plugin/flp_dec_plugins.py` (the same, the
//! kinds of hosted plugin and the CLAP id) and DawVert
//! `data_main/datadef/fl_studio.ddef` (the order of the numbers of FL
//! Studio's own plugins).

use crate::event::u32_at;
use crate::model::Plugin;

/// The internal name FL Studio gives every plugin it hosts.
pub const WRAPPER_NAME: &str = "Fruity Wrapper";

/// A hosted plugin: one that is not FL Studio's own.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HostedPlugin {
    pub format: PluginFormat,
    /// The plugin's name as it reports it. Record 54.
    pub name: Option<String>,
    /// Record 56.
    pub vendor: Option<String>,
    /// Where the plugin's file was on the computer that saved the project.
    /// Record 55.
    pub path: Option<String>,
    /// The four-character id of a VST 2 plugin, as a number. Record 51.
    pub vst2_id: Option<u32>,
    /// The id of a VST 3 plugin. Record 52.
    pub guid: Option<Vec<u8>>,
    /// The id of a CLAP plugin. Record 58. One source (DawVert).
    pub clap_id: Option<String>,
    /// The plugin's own saved data, with the few bytes FL Studio puts in
    /// front of it. Record 53.
    pub state: Vec<u8>,
    /// PyFLP MIDI subrecord 1: RPN bend range in semitones, bytes 8..12.
    pub pitch_bend_range: Option<u32>,
    /// PyFLP flags subrecord 2, word at offset 9, bit 0.
    pub send_pitch_bend_range: Option<bool>,
}

/// The plugin standard of a hosted plugin.
///
/// One source (DawVert): the first number of record 50 is 0 or 4 for
/// VST 2, 7 or 8 for VST 3 and 11 or 12 for CLAP, the higher of each pair
/// being an instrument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PluginFormat {
    Vst2,
    Vst3,
    Clap,
    /// The wrapper does not say, or says something the sources do not
    /// name.
    #[default]
    Unknown,
}

impl PluginFormat {
    /// The name of the standard, for messages.
    pub fn label(self) -> &'static str {
        match self {
            PluginFormat::Vst2 => "VST 2",
            PluginFormat::Vst3 => "VST 3",
            PluginFormat::Clap => "CLAP",
            PluginFormat::Unknown => "an unknown format",
        }
    }
}

impl Plugin {
    /// True for a plugin FL Studio hosts.
    pub fn is_hosted(&self) -> bool {
        self.internal_name.eq_ignore_ascii_case(WRAPPER_NAME)
    }

    /// Reads the wrapper of a hosted plugin. `None` for FL Studio's own
    /// plugins, and for a wrapper whose records do not add up.
    ///
    /// The state is a 32-bit number, then records: a 32-bit number that
    /// says what the record is, a 64-bit length, and that many bytes.
    pub fn hosted(&self) -> Option<HostedPlugin> {
        if !self.is_hosted() {
            return None;
        }
        let state = &self.state;
        let mut hosted = HostedPlugin::default();
        let mut at = 4;
        let mut records = 0;
        while at < state.len() {
            let header = state.get(at..at + 12)?;
            let id = u32_at(header, 0);
            let length = u64::from(u32_at(header, 4)) | (u64::from(u32_at(header, 8)) << 32);
            let start = at + 12;
            let end = start.checked_add(usize::try_from(length).ok()?)?;
            let data = state.get(start..end)?;
            let text = || Some(crate::text::decode_bytes(data)).filter(|text| !text.is_empty());
            match id {
                1 if data.len() >= 12 => hosted.pitch_bend_range = Some(u32_at(data, 8)),
                2 if data.len() >= 13 => {
                    hosted.send_pitch_bend_range = Some(u32_at(data, 9) & 1 != 0)
                }
                50 if data.len() >= 4 => {
                    hosted.format = match u32_at(data, 0) {
                        0 | 4 => PluginFormat::Vst2,
                        7 | 8 => PluginFormat::Vst3,
                        11 | 12 => PluginFormat::Clap,
                        _ => PluginFormat::Unknown,
                    };
                }
                51 if data.len() >= 4 => hosted.vst2_id = Some(u32_at(data, 0)),
                52 => hosted.guid = Some(data.to_vec()),
                53 => hosted.state = data.to_vec(),
                54 => hosted.name = text(),
                55 => hosted.path = text(),
                56 => hosted.vendor = text(),
                58 => hosted.clap_id = text(),
                _ => {}
            }
            at = end;
            records += 1;
        }
        (records > 0).then_some(hosted)
    }
}

/// The 32-bit numbers a plugin of FL Studio's own saved, by position.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Numbers<'a>(pub(crate) &'a [u8]);

impl Numbers<'_> {
    /// The number at `index`, or `None` past the end of the state. States
    /// grew over the versions, so a short one is not an error.
    pub(crate) fn get(&self, index: usize) -> Option<i32> {
        let at = index.checked_mul(4)?;
        (self.0.len() >= at.checked_add(4)?).then(|| u32_at(self.0, at) as i32)
    }

    /// The single byte at `offset`.
    pub(crate) fn byte(&self, offset: usize) -> Option<u8> {
        self.0.get(offset).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: u32, data: &[u8]) -> Vec<u8> {
        let mut bytes = id.to_le_bytes().to_vec();
        bytes.extend_from_slice(&(data.len() as u64).to_le_bytes());
        bytes.extend_from_slice(data);
        bytes
    }

    fn wrapper(records: &[Vec<u8>]) -> Plugin {
        let mut state = 10_u32.to_le_bytes().to_vec();
        for record in records {
            state.extend_from_slice(record);
        }
        Plugin {
            internal_name: WRAPPER_NAME.to_owned(),
            generator: Some(true),
            state,
        }
    }

    #[test]
    fn a_wrapper_names_the_plugin_it_holds() {
        let plugin = wrapper(&[
            record(1, &[0xFF; 20]),
            record(50, &[4, 0, 0, 0, 9, 9]),
            record(51, b"1Sdn"),
            record(54, b"Bright Keys"),
            record(56, b"Example Audio"),
            record(55, b"C:\\Plugins\\BrightKeys.dll"),
            record(53, &[1, 2, 3, 4, 5]),
        ]);
        let hosted = plugin.hosted().expect("a readable wrapper");
        assert_eq!(hosted.format, PluginFormat::Vst2);
        assert_eq!(hosted.name.as_deref(), Some("Bright Keys"));
        assert_eq!(hosted.vendor.as_deref(), Some("Example Audio"));
        assert_eq!(hosted.path.as_deref(), Some("C:\\Plugins\\BrightKeys.dll"));
        assert_eq!(hosted.vst2_id, Some(u32::from_le_bytes(*b"1Sdn")));
        assert_eq!(hosted.state, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn the_first_number_of_record_50_says_the_format() {
        let format = |kind: u32| {
            wrapper(&[record(50, &kind.to_le_bytes())])
                .hosted()
                .map(|hosted| hosted.format)
        };
        assert_eq!(format(0), Some(PluginFormat::Vst2));
        assert_eq!(format(8), Some(PluginFormat::Vst3));
        assert_eq!(format(11), Some(PluginFormat::Clap));
        assert_eq!(format(99), Some(PluginFormat::Unknown));
    }

    #[test]
    fn a_wrapper_whose_records_do_not_add_up_is_not_read() {
        let mut plugin = wrapper(&[record(54, b"Bright Keys")]);
        plugin.state.truncate(plugin.state.len() - 3);
        assert_eq!(plugin.hosted(), None);

        let mut huge = 10_u32.to_le_bytes().to_vec();
        huge.extend_from_slice(&54_u32.to_le_bytes());
        huge.extend_from_slice(&u64::MAX.to_le_bytes());
        plugin.state = huge;
        assert_eq!(plugin.hosted(), None);

        plugin.state = vec![10, 0, 0, 0];
        assert_eq!(plugin.hosted(), None);
    }

    #[test]
    fn only_wrappers_are_hosted_plugins() {
        let plugin = Plugin {
            internal_name: "Some Native Synth".to_owned(),
            generator: Some(true),
            state: wrapper(&[record(54, b"x")]).state,
        };
        assert!(!plugin.is_hosted());
        assert_eq!(plugin.hosted(), None);
    }

    #[test]
    fn numbers_read_by_position_and_stop_at_the_end() {
        let state = [1, 0, 0, 0, 0xF4, 0xFF, 0xFF, 0xFF, 7, 0];
        let numbers = Numbers(&state);
        assert_eq!(numbers.get(0), Some(1));
        assert_eq!(numbers.get(1), Some(-12));
        assert_eq!(numbers.get(2), None);
        assert_eq!(numbers.byte(8), Some(7));
        assert_eq!(numbers.byte(10), None);
    }
}
