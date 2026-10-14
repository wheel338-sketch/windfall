//! Loading plugin files and creating plugins from them.

use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clack_host::prelude::HostInfo;

use crate::clap::ClapModule;
use crate::descriptor::{PluginDescriptor, PluginFormat, PluginLayout};
use crate::error::PluginError;
use crate::instance::PluginInstance;
use crate::vst3::Vst3Module;

/// How serious a plugin thinks its message is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
}

/// Where plugins' log messages go.
pub(crate) type LogSink = Arc<dyn Fn(LogLevel, &str) + Send + Sync>;

/// The host as plugins see it: a name, a version and somewhere to log to.
///
/// It belongs to one thread, the one plugins are created and edited on.
/// Everything made from it is tied to that thread too, except the
/// [`PluginProcessor`](crate::PluginProcessor) an active plugin hands out.
pub struct PluginHost {
    info: HostInfo,
    log: LogSink,
    not_send: PhantomData<*const ()>,
}

impl PluginHost {
    /// A host that introduces itself to plugins with this name and version.
    pub fn new(name: &str, version: &str) -> Self {
        let clean = |text: &str| text.replace('\0', "");
        let info = HostInfo::new(
            &clean(name),
            "The Windfall contributors",
            "https://github.com/windfall-daw/windfall",
            &clean(version),
        )
        .expect("the texts hold no zero bytes");
        Self {
            info,
            log: crate::clap::silent_log(),
            not_send: PhantomData,
        }
    }

    /// The host Windfall itself is.
    pub fn windfall() -> Self {
        Self::new("Windfall", env!("CARGO_PKG_VERSION"))
    }

    /// AUv2 components are registered by macOS, with stable type/subtype/vendor IDs.
    #[cfg(target_os = "macos")]
    pub fn create_audio_unit(&self, id: &str) -> Result<PluginInstance, PluginError> {
        let (descriptor, backend) = crate::au::create(id)?;
        Ok(PluginInstance::new(descriptor, backend))
    }

    /// Sends what plugins log to `sink`. A plugin may log from any thread
    /// but the audio thread, where its messages are dropped. Without a
    /// sink, all of them are.
    pub fn set_log(&mut self, sink: impl Fn(LogLevel, &str) + Send + Sync + 'static) {
        self.log = Arc::new(sink);
    }

    /// Loads a plugin file into this process and runs its start-up code.
    ///
    /// The app does this only for files the scanner has been through and
    /// did not put on the blocklist. A file that crashes here takes the app
    /// with it.
    pub fn load(&self, path: &Path) -> Result<PluginModule, PluginError> {
        let format = PluginFormat::of(path).ok_or_else(|| {
            PluginError::Load(
                windfall_core::notice!("pluginHost.notAPluginFile", path = path.display())
                    .to_string(),
            )
        })?;
        let inner = match format {
            PluginFormat::Clap => ModuleInner::Clap(ClapModule::load(path)?),
            PluginFormat::Vst3 => ModuleInner::Vst3(Vst3Module::load(path)?),
            PluginFormat::Au => {
                return Err(PluginError::Unsupported(
                    "file-based AU; use installed component identity",
                ));
            }
        };
        Ok(PluginModule {
            path: path.to_path_buf(),
            inner,
            info: self.info.clone(),
            log: Arc::clone(&self.log),
            not_send: PhantomData,
        })
    }
}

enum ModuleInner {
    Clap(ClapModule),
    Vst3(Vst3Module),
}

/// A plugin file that is loaded into this process. It stays loaded for as
/// long as this or any plugin created from it exists.
pub struct PluginModule {
    path: PathBuf,
    inner: ModuleInner,
    info: HostInfo,
    log: LogSink,
    not_send: PhantomData<*const ()>,
}

impl PluginModule {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn format(&self) -> PluginFormat {
        match self.inner {
            ModuleInner::Clap(_) => PluginFormat::Clap,
            ModuleInner::Vst3(_) => PluginFormat::Vst3,
        }
    }

    /// The plugins the file lists, in the file's order.
    pub fn descriptors(&self) -> Vec<PluginDescriptor> {
        match &self.inner {
            ModuleInner::Clap(module) => module.descriptors(),
            ModuleInner::Vst3(module) => module.descriptors(),
        }
    }

    /// Creates the plugin with the given id.
    pub fn create(&self, id: &str) -> Result<PluginInstance, PluginError> {
        let descriptor = self
            .descriptors()
            .into_iter()
            .find(|descriptor| descriptor.id == id)
            .ok_or_else(|| PluginError::NotFound(id.to_owned()))?;
        let backend = match &self.inner {
            ModuleInner::Clap(module) => module.create(id, &self.info, Arc::clone(&self.log)),
            ModuleInner::Vst3(module) => module.create(id),
        }
        .map_err(|error| {
            crate::export_runtime::last_error(&self.path).map_or(error, PluginError::Load)
        })?;
        Ok(PluginInstance::new(descriptor, backend))
    }

    /// Creates the plugin with the given id just long enough to ask what
    /// ports and parameters it has. This is the scanner's question.
    pub fn probe(&self, id: &str) -> Result<PluginLayout, PluginError> {
        match &self.inner {
            ModuleInner::Clap(_) => Ok(self.create(id)?.layout().clone()),
            ModuleInner::Vst3(module) => module.probe(id),
        }
    }
}
