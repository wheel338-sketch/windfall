use serde::{Deserialize, Serialize};
use windfall_core::AudioBuffer;
use windfall_engine::SamplePool;
use windfall_project::{Project, SampleId};

pub const MAX_STATE_BYTES: usize = 64 * 1024 * 1024;
const MAX_SAMPLES: usize = 4096;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EmbeddedSample {
    pub id: SampleId,
    pub sample_rate: u32,
    pub channels: u16,
    pub data: Vec<f32>,
}

#[derive(Clone)]
pub struct Program {
    pub project: Project,
    pub samples: Vec<EmbeddedSample>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Packet {
    schema: u32,
    // Preserve the file loader's validation and migrations rather than bypassing it.
    project: String,
    samples: Vec<EmbeddedSample>,
}

impl Default for Program {
    fn default() -> Self {
        Self {
            project: Project::new("Windfall"),
            samples: Vec::new(),
        }
    }
}

impl Program {
    /// Validate on the main thread before preparing or replacing state.
    pub fn validate(&self) -> Result<(), String> {
        self.project.check()?;
        if !self.project.plugins.is_empty() {
            return Err(
                "Nested native plugins need an export-owned provider; not supported yet".into(),
            );
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
        if self.samples.len() > MAX_SAMPLES || self.samples.len() != self.project.samples.len() {
            return Err("Every project sample must have exactly one embedded asset".into());
        }
        let mut bytes = 0usize;
        let mut ids = std::collections::HashSet::new();
        for sample in &self.samples {
            if !ids.insert(sample.id)
                || !self
                    .project
                    .samples
                    .iter()
                    .any(|asset| asset.id == sample.id)
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
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        let packet = Packet {
            schema: 1,
            project: windfall_project::file::to_json(&self.project).map_err(|e| e.to_string())?,
            samples: self.samples.clone(),
        };
        let bytes = serde_json::to_vec(&packet).map_err(|e| e.to_string())?;
        if bytes.len() > MAX_STATE_BYTES {
            return Err("Plugin state exceeds 64 MiB".into());
        }
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_STATE_BYTES {
            return Err("Plugin state exceeds 64 MiB".into());
        }
        let packet: Packet = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        if packet.schema != 1 {
            return Err("Unsupported Windfall plugin-state schema".into());
        }
        let program = Self {
            project: windfall_project::file::from_json(&packet.project)
                .map_err(|e| e.to_string())?,
            samples: packet.samples,
        };
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
}
