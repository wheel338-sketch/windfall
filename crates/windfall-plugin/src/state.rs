use serde::{Deserialize, Serialize};
use windfall_core::AudioBuffer;
use windfall_engine::SamplePool;
use windfall_project::{Project, SampleId};

pub const MAX_STATE_BYTES: usize = 64 * 1024 * 1024;
/// Aggregate original SF2 bytes (including banks inside modular racks). JSON
/// encoding is additionally limited to MAX_STATE_BYTES; bytes are never PCM.
pub const MAX_RAW_BANK_BYTES: usize = 16 * 1024 * 1024;
const MAX_SAMPLES: usize = 4096;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EmbeddedSample {
    pub id: SampleId,
    pub sample_rate: u32,
    pub channels: u16,
    pub data: Vec<f32>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EmbeddedSoundFont {
    pub id: SampleId,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EmbeddedPresetSource {
    pub id: SampleId,
    pub bytes: Vec<u8>,
}

fn preset_source(project: &Project, id: SampleId) -> bool {
    project.is_preset_source(id)
}

#[derive(Clone)]
pub struct Program {
    pub project: Project,
    pub samples: Vec<EmbeddedSample>,
    pub soundfonts: Vec<EmbeddedSoundFont>,
    pub preset_sources: Vec<EmbeddedPresetSource>,
    // Copy curve handles need control-side roots even while the host is inactive.
    _asset_pins: std::sync::Arc<windfall_dsp::asset::AssetPins>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Packet {
    schema: u32,
    // Preserve the file loader's validation and migrations rather than bypassing it.
    project: String,
    samples: Vec<EmbeddedSample>,
    #[serde(default)]
    soundfonts: Vec<EmbeddedSoundFont>,
    #[serde(default)]
    preset_sources: Vec<EmbeddedPresetSource>,
}

struct Bounded(Vec<u8>);
impl std::io::Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_STATE_BYTES.saturating_sub(self.0.len()) {
            return Err(std::io::Error::other("Plugin state exceeds 64 MiB"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Default for Program {
    fn default() -> Self {
        Self::new(Project::new("Windfall"), Vec::new(), Vec::new())
    }
}

impl Program {
    pub fn new(
        project: Project,
        samples: Vec<EmbeddedSample>,
        soundfonts: Vec<EmbeddedSoundFont>,
    ) -> Self {
        let mut pins = windfall_dsp::asset::AssetPins::for_effects(
            project
                .mixer
                .tracks
                .iter()
                .flat_map(|t| t.effects.iter().map(|e| &e.params)),
        );
        for rack in &project.settings.modular_racks {
            pins.extend(rack.graph.effect_asset_pins());
        }
        Self {
            project,
            samples,
            soundfonts,
            preset_sources: Vec::new(),
            _asset_pins: std::sync::Arc::new(pins),
        }
    }
    /// Validate on the main thread before preparing or replacing state.
    pub fn with_preset_sources(mut self, sources: Vec<EmbeddedPresetSource>) -> Self {
        self.preset_sources = sources;
        self
    }

    pub fn validate(&self) -> Result<(), String> {
        self.project.check()?;
        for binding in &self.project.plugins {
            crate::nested::validate(binding)?;
        }
        for track in &self.project.mixer.tracks {
            if track.recording.is_some() {
                return Err(
                    "Hardware recording routes cannot open devices inside the plugin".into(),
                );
            }
            if track.external_output.is_some_and(|route| {
                usize::from(route.left) >= crate::OUTPUT_CHANNELS
                    || route
                        .right
                        .is_some_and(|right| usize::from(right) >= crate::OUTPUT_CHANNELS)
            }) {
                return Err(
                    "External mixer route is outside the plugin's 16 stereo outputs".into(),
                );
            }
        }
        if self.project.samples.len() > MAX_SAMPLES
            || self.samples.len()
                != self
                    .project
                    .samples
                    .iter()
                    .filter(|s| !s.soundfont && !preset_source(&self.project, s.id))
                    .count()
            || self.soundfonts.len() > MAX_SAMPLES
            || self.preset_sources.len() > MAX_SAMPLES
        {
            return Err("Every project PCM sample must have exactly one embedded PCM asset".into());
        }
        let mut bytes = 0usize;
        let mut ids = std::collections::HashSet::new();
        for sample in &self.samples {
            if !ids.insert(sample.id)
                || !self.project.samples.iter().any(|asset| {
                    asset.id == sample.id
                        && !asset.soundfont
                        && !preset_source(&self.project, asset.id)
                })
            {
                return Err("Duplicate or unknown embedded sample identity".into());
            }
            if !(8_000..=384_000).contains(&sample.sample_rate)
                || !(1..=32).contains(&sample.channels)
                || sample.data.is_empty()
                || sample.data.len() % usize::from(sample.channels) != 0
                || sample.data.iter().any(|value| !value.is_finite())
            {
                return Err("Embedded sample format or PCM is invalid".into());
            }
            bytes = bytes
                .checked_add(sample.data.len().saturating_mul(4))
                .ok_or("Embedded assets exceed the state bound")?;
            if bytes > MAX_STATE_BYTES {
                return Err("Embedded assets exceed the state bound".into());
            }
        }
        let mut raw_bytes = 0usize;
        let mut preset_bytes = 0usize;
        for source in &self.preset_sources {
            if !ids.insert(source.id) || !preset_source(&self.project, source.id) {
                return Err("Duplicate or unknown embedded preset source identity".into());
            }
            if source.bytes.len() > windfall_engine::preset_render::MAX_PRESET_BYTES {
                return Err("An embedded preset source exceeds 1 MiB".into());
            }
            preset_bytes = preset_bytes
                .checked_add(source.bytes.len())
                .ok_or("Preset source byte overflow")?;
        }
        if preset_bytes > MAX_RAW_BANK_BYTES {
            return Err("Plugin original preset sources exceed the aggregate 16 MiB limit".into());
        }
        bytes = bytes
            .checked_add(preset_bytes)
            .ok_or("Embedded assets exceed the state bound")?;
        for bank in &self.soundfonts {
            if !ids.insert(bank.id)
                || !self
                    .project
                    .samples
                    .iter()
                    .any(|s| s.id == bank.id && s.soundfont)
            {
                return Err("Duplicate, unknown or non-SF2 embedded bank source identity".into());
            }
            raw_bytes = raw_bytes
                .checked_add(bank.bytes.len())
                .ok_or("Raw SF2 byte overflow")?;
        }
        fn rack_bytes(graph: &windfall_project::RackGraph) -> Result<usize, String> {
            let mut bytes = 0usize;
            for node in &graph.nodes {
                if let Some(windfall_project::RackNodeAsset::SoundFont {
                    bytes: Some(raw), ..
                }) = &node.asset
                {
                    bytes = bytes
                        .checked_add(raw.len())
                        .ok_or("Raw SF2 byte overflow")?;
                }
                if let windfall_project::RackProcessor::Rack { graph } = &node.processor {
                    bytes = bytes
                        .checked_add(rack_bytes(graph)?)
                        .ok_or("Raw SF2 byte overflow")?;
                }
            }
            Ok(bytes)
        }
        for rack in &self.project.settings.modular_racks {
            raw_bytes = raw_bytes
                .checked_add(rack_bytes(&rack.graph)?)
                .ok_or("Raw SF2 byte overflow")?;
        }
        if raw_bytes > MAX_RAW_BANK_BYTES {
            return Err("Plugin raw SF2 sources exceed the aggregate 16 MiB bank limit".into());
        }
        if bytes
            .checked_add(raw_bytes)
            .is_none_or(|sum| sum > MAX_STATE_BYTES)
        {
            return Err("Embedded assets exceed the 64 MiB state bound".into());
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        // Bound both JSON layers before allocating a complete project string.
        let mut project = Bounded(Vec::new());
        serde_json::to_writer_pretty(&mut project, &self.project).map_err(|e| e.to_string())?;
        std::io::Write::write_all(&mut project, b"\n").map_err(|e| e.to_string())?;
        let project = String::from_utf8(project.0).map_err(|e| e.to_string())?;
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct BorrowedPacket<'a> {
            schema: u32,
            project: &'a str,
            samples: &'a [EmbeddedSample],
            soundfonts: &'a [EmbeddedSoundFont],
            preset_sources: &'a [EmbeddedPresetSource],
        }
        let packet = BorrowedPacket {
            schema: 3,
            project: &project,
            samples: &self.samples,
            soundfonts: &self.soundfonts,
            preset_sources: &self.preset_sources,
        };
        let mut output = Bounded(Vec::new());
        serde_json::to_writer(&mut output, &packet).map_err(|e| e.to_string())?;
        Ok(output.0)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_STATE_BYTES {
            return Err("Plugin state exceeds 64 MiB".into());
        }
        let packet: Packet = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        if !matches!(packet.schema, 1..=3)
            || (packet.schema == 1 && !packet.soundfonts.is_empty())
            || (packet.schema < 3 && !packet.preset_sources.is_empty())
        {
            return Err("Unsupported Windfall plugin-state schema".into());
        }
        let program = Self::new(
            windfall_project::file::from_json(&packet.project).map_err(|e| e.to_string())?,
            packet.samples,
            packet.soundfonts,
        )
        .with_preset_sources(packet.preset_sources);
        program.validate()?;
        Ok(program)
    }

    pub(crate) fn sample_pool(&self) -> SamplePool {
        let mut pool = SamplePool::new();
        for sample in &self.samples {
            pool.insert(
                sample.id,
                AudioBuffer::from_interleaved(
                    sample.sample_rate,
                    sample.channels,
                    sample.data.clone(),
                ),
            );
        }
        pool
    }

    pub(crate) fn preset_source_diagnostics(&self) -> Vec<String> {
        self.project
            .samples
            .iter()
            .filter(|source| preset_source(&self.project, source.id))
            .filter_map(
                |source| match self.preset_sources.iter().find(|raw| raw.id == source.id) {
                    None => Some(
                        windfall_core::notice!(
                            "pluginExport.presetSourceMissing",
                            name = source.name
                        )
                        .to_string(),
                    ),
                    Some(raw) => {
                        windfall_project::Preset::from_json(&raw.bytes)
                            .err()
                            .map(|error| {
                                windfall_core::notice!(
                                    "pluginExport.presetSourceInvalid",
                                    name = source.name,
                                    error = error.to_string()
                                )
                                .to_string()
                            })
                    }
                },
            )
            .collect()
    }

    pub fn validate_preset_sources(&self) -> Result<(), String> {
        self.validate()?;
        let errors = self.preset_source_diagnostics();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        }
    }

    /// Control-side only. Missing/corrupt bytes install an unavailable bank,
    /// preserving the real SoundFont instrument with silence and a diagnostic.
    /// Each parsed generation reserves the engine's shared aggregate budget.
    pub(crate) fn prepare_soundfonts(&self, pool: &mut SamplePool) -> Vec<String> {
        let mut errors = Vec::new();
        let mut seen = Vec::new();
        for channel in &self.project.channels {
            let Some(asset @ windfall_project::SampleInstrumentAsset::SoundFont { .. }) =
                &channel.sample_instrument
            else {
                continue;
            };
            let asset = asset.canonical(&self.project);
            if seen.contains(&asset) {
                continue;
            }
            seen.push(asset.clone());
            let source = asset.soundfont_source();
            let result = source
                .and_then(|id| self.soundfonts.iter().find(|b| b.id == id))
                .ok_or_else(|| {
                    windfall_core::notice!(
                        "pluginExport.soundFontSourceMissing",
                        source = format!("{:?}", source)
                    )
                    .to_string()
                })
                .and_then(|bank| pool.insert_soundfont(asset.clone(), &bank.bytes));
            if let Err(reason) = result {
                let message = windfall_core::notice!(
                    "pluginExport.soundFontInstrument",
                    instrument = channel.name,
                    source = format!("{:?}", source),
                    reason = windfall_core::notice::term(&reason)
                )
                .to_string();
                pool.insert_unavailable_soundfont(asset, message.clone());
                errors.push(message);
            }
        }
        errors
    }

    /// The rack stores its bank bytes inline in the ordinary project. Check
    /// those before engine construction too; only the detached playback copy
    /// loses a damaged payload. The saved program keeps the exact original.
    pub(crate) fn prepare_rack_soundfonts(playback: &mut Project) -> Vec<String> {
        fn prepare(graph: &mut windfall_project::RackGraph, errors: &mut Vec<String>) {
            for node in &mut graph.nodes {
                if let Some(windfall_project::RackNodeAsset::SoundFont {
                    bytes,
                    bank,
                    program,
                    ..
                }) = &mut node.asset
                {
                    let result = bytes
                        .as_deref()
                        .ok_or_else(|| "Missing embedded raw rack SF2 bytes".to_owned())
                        .and_then(|raw| {
                            windfall_dsp::sample_bank::soundfont::parse_soundfont_preset_budgeted(
                                raw,
                                *bank,
                                *program,
                                &windfall_dsp::sample_bank::BankBudget::shared(),
                            )
                            .map(|_| ())
                            .map_err(|e| e.to_string())
                        });
                    if let Err(reason) = result {
                        errors.push(
                            windfall_core::notice!(
                                "pluginExport.rackSoundFont",
                                instrument = node.name,
                                reason = windfall_core::notice::term(&reason)
                            )
                            .to_string(),
                        );
                        *bytes = None;
                    }
                }
                if let windfall_project::RackProcessor::Rack { graph } = &mut node.processor {
                    prepare(graph, errors);
                }
            }
        }
        let mut errors = Vec::new();
        for rack in &mut playback.settings.modular_racks {
            prepare(&mut rack.graph, &mut errors);
        }
        errors
    }

    /// Native export admission, including retained but currently unused sources.
    /// Host recall instead uses prepare_soundfonts so damaged instruments are silent.
    pub fn validate_soundfont_sources(&self) -> Result<(), String> {
        self.validate()?;
        for source in self.project.samples.iter().filter(|s| s.soundfont) {
            let bank = self
                .soundfonts
                .iter()
                .find(|b| b.id == source.id)
                .ok_or_else(|| {
                    windfall_core::notice!("pluginExport.soundFontNamedMissing", name = source.name)
                        .to_string()
                })?;
            windfall_dsp::sample_bank::soundfont::presets(&bank.bytes).map_err(|e| {
                windfall_core::notice!(
                    "pluginExport.soundFontUnreadable",
                    name = source.name,
                    error = e.to_string()
                )
                .to_string()
            })?;
        }
        let mut pool = SamplePool::new();
        let mut errors = self.prepare_soundfonts(&mut pool);
        errors.extend(Self::prepare_rack_soundfonts(&mut self.project.clone()));
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
