//! The ONLY module that calls ort. Integrator must check the rc.10 APIs listed
//! in STEM-SEPARATION.md after downloading source. No fallback inference.
use crate::separation::{PreparedSeparation, RuntimeChoice, SeparationBackend, contract};
use crate::{AnalysisError, ModelManifest, Result, WaveformLayout, Work};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub struct OrtBackend {
    pub choice: RuntimeChoice,
    /// App data/runtime and explicitly imported model-pack locations only.
    pub search_directories: std::sync::Arc<std::sync::RwLock<Vec<PathBuf>>>,
}
impl OrtBackend {
    pub fn runtime_path(&self) -> Result<PathBuf> {
        let chosen = self
            .choice
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .or_else(|| std::env::var_os("WINDFALL_ORT_DYLIB").map(PathBuf::from))
            .or_else(|| std::env::var_os("ORT_DYLIB_PATH").map(PathBuf::from));
        if let Some(path) = chosen {
            return regular_library(&path);
        }
        let name = if cfg!(target_os = "windows") {
            "onnxruntime.dll"
        } else if cfg!(target_os = "macos") {
            "libonnxruntime.dylib"
        } else {
            "libonnxruntime.so"
        };
        for directory in self
            .search_directories
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
        {
            let path = directory.join(name);
            if path.exists() {
                return regular_library(&path);
            }
        }
        Err(AnalysisError::Adapter("ONNX Runtime library is missing. Choose the installed library in Settings → Stem separation, or set ORT_DYLIB_PATH. Nothing is downloaded.".into()))
    }
}
fn regular_library(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().len() > 2048 || !path.is_absolute() || !path.is_file() {
        return Err(AnalysisError::Adapter(
            "ONNX Runtime must be an absolute path to an installed shared library.".into(),
        ));
    }
    Ok(std::fs::canonicalize(path)?)
}
fn failed(error: impl std::fmt::Display) -> AnalysisError {
    AnalysisError::Adapter(
        windfall_core::notice!("analysis.onnxCapability", error = error.to_string()).to_string(),
    )
}
struct OrtPrepared {
    session: ort::session::Session,
    input_name: String,
    output_name: String,
    frames: usize,
    channels: usize,
    layout: WaveformLayout,
}
impl SeparationBackend for OrtBackend {
    fn prepare(
        &self,
        model: &ModelManifest,
        weights: &[u8],
        work: &mut Work,
    ) -> Result<Box<dyn PreparedSeparation>> {
        let spec = contract(model)?;
        work.check()?;
        if weights.is_empty() {
            return Err(AnalysisError::ModelAbsent);
        }
        let path = self.runtime_path()?;
        static LIBRARY: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
        let mut installed = LIBRARY
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if installed.as_ref().is_some_and(|old| *old != path) {
            return Err(failed(
                "A different runtime is already loaded. Restart Windfall to change libraries",
            ));
        }
        // load-dynamic may panic on a loader error in some ort releases. Turn
        // that into an honest job failure; never copy the input as a result.
        let session = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || -> Result<ort::session::Session> {
                if installed.is_none() {
                    ort::init_from(path.to_string_lossy().as_ref())
                        .with_name("Windfall separation")
                        .commit()
                        .map_err(failed)?;
                    *installed = Some(path.clone());
                }
                ort::session::Session::builder()
                    .map_err(failed)?
                    .with_intra_threads(1)
                    .map_err(failed)?
                    .with_memory_pattern(false)
                    .map_err(failed)?
                    .commit_from_memory(weights)
                    .map_err(failed)
            },
        ))
        .map_err(|_| failed("The shared library could not be loaded"))??;
        drop(installed);
        work.check()?;
        // Refuse extra inputs/outputs (no hidden unsupported preprocessing).
        if session.inputs.len() != 1
            || session.inputs[0].name != spec.inputs[0].name
            || session.outputs.len() != 1
            || session.outputs[0].name != spec.outputs[0].name
        {
            return Err(failed(
                "Model tensor names/count differ from the validated manifest",
            ));
        }
        let frames = (spec.segment_frames + spec.context_frames * 2) as usize;
        let channels = model.input_channels as usize;
        let (input_shape, output_shape) = match spec.layout {
            WaveformLayout::ChannelsFirst => (
                vec![1, channels as i64, frames as i64],
                vec![1, 4, channels as i64, frames as i64],
            ),
            WaveformLayout::ChannelsLast => (
                vec![1, frames as i64, channels as i64],
                vec![1, 4, frames as i64, channels as i64],
            ),
        };
        validate_type(&session.inputs[0].input_type, &input_shape, &spec.inputs[0])?;
        validate_type(
            &session.outputs[0].output_type,
            &output_shape,
            &spec.outputs[0],
        )?;
        Ok(Box::new(OrtPrepared {
            session,
            input_name: spec.inputs[0].name.clone(),
            output_name: spec.outputs[0].name.clone(),
            channels: model.input_channels as usize,
            frames,
            layout: spec.layout,
        }))
    }
}
fn validate_type(
    value: &ort::value::ValueType,
    expected: &[i64],
    spec: &crate::TensorSpec,
) -> Result<()> {
    let ort::value::ValueType::Tensor { ty, shape, .. } = value else {
        return Err(failed("Only waveform Float32 tensors are supported"));
    };
    if *ty != ort::tensor::TensorElementType::Float32
        || shape.len() != expected.len()
        || shape
            .iter()
            .zip(expected)
            .zip(&spec.shape)
            .any(|((&actual, &size), dim)| {
                actual != size
                    && !(actual == -1 && matches!(dim, crate::TensorDimension::Symbol(_)))
            })
    {
        return Err(failed(
            "Model's declared tensor dtype/shape differs from manifest",
        ));
    }
    Ok(())
}
impl PreparedSeparation for OrtPrepared {
    fn run_chunk(&mut self, input: &[f32], work: &mut Work) -> Result<Vec<Vec<f32>>> {
        work.check()?;
        if input.len() != self.frames * self.channels {
            return Err(failed("Input chunk shape mismatch"));
        }
        let mut tensor = vec![0.0; input.len()];
        for frame in 0..self.frames {
            for channel in 0..self.channels {
                let index = match self.layout {
                    WaveformLayout::ChannelsFirst => channel * self.frames + frame,
                    WaveformLayout::ChannelsLast => frame * self.channels + channel,
                };
                tensor[index] = input[frame * self.channels + channel];
            }
        }
        let shape = match self.layout {
            WaveformLayout::ChannelsFirst => vec![1, self.channels, self.frames],
            WaveformLayout::ChannelsLast => vec![1, self.frames, self.channels],
        };
        let tensor =
            ort::value::Tensor::from_array((shape, tensor.into_boxed_slice())).map_err(failed)?;
        let inputs = vec![(
            self.input_name.as_str(),
            ort::session::SessionInputValue::from(tensor),
        )];
        let outputs = self.session.run(inputs).map_err(failed)?;
        let value = outputs
            .get(self.output_name.as_str())
            .ok_or_else(|| failed("Missing named output"))?;
        let (shape, values) = value.try_extract_tensor::<f32>().map_err(failed)?;
        let expected = match self.layout {
            WaveformLayout::ChannelsFirst => {
                vec![1i64, 4, self.channels as i64, self.frames as i64]
            }
            WaveformLayout::ChannelsLast => vec![1i64, 4, self.frames as i64, self.channels as i64],
        };
        if shape.iter().copied().collect::<Vec<_>>() != expected || values.len() != input.len() * 4
        {
            return Err(failed(
                "Output must be Float32 [1,4,channels,frames] or [1,4,frames,channels]",
            ));
        }
        let mut stems = vec![vec![0.0; input.len()]; 4];
        for stem in 0..4 {
            for frame in 0..self.frames {
                for channel in 0..self.channels {
                    let index = match self.layout {
                        WaveformLayout::ChannelsFirst => channel * self.frames + frame,
                        WaveformLayout::ChannelsLast => frame * self.channels + channel,
                    };
                    stems[stem][frame * self.channels + channel] =
                        values[stem * input.len() + index];
                }
            }
        }
        work.checkpoint(input.len() as u64 * 4)?;
        Ok(stems)
    }
}
