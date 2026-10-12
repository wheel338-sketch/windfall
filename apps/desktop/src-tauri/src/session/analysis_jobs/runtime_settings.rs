use super::*;
use std::io::Write;
use windfall_ipc::AnalysisRuntimeSettings;
pub(super) fn load(root: &Path) -> Result<AnalysisRuntimeSettings, String> {
    let path = root.join("runtime.json");
    match std::fs::File::open(&path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(8193).read_to_end(&mut bytes).map_err(|e| error("io", e))?;
            if bytes.len() > 8192 { return Err(error("invalid", "Runtime settings exceed 8 KiB.")); }
            serde_json::from_slice(&bytes).map_err(|e| error("invalid", e))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Default::default()),
        Err(e) => Err(error("io", e)),
    }
}
impl Service {
    fn save_runtime(&self, settings: &AnalysisRuntimeSettings) -> Result<(), String> {
        std::fs::create_dir_all(&self.root).map_err(|e| error("io", e))?;
        let bytes = serde_json::to_vec(settings).map_err(|e| error("invalid", e))?;
        if bytes.len() > 8192 { return Err(error("invalid", "Runtime settings exceed 8 KiB.")); }
        let mut file = tempfile::NamedTempFile::new_in(&self.root).map_err(|e| error("io", e))?;
        file.write_all(&bytes).map_err(|e| error("io", e))?;
        file.as_file().sync_all().map_err(|e| error("io", e))?;
        file.persist(self.root.join("runtime.json")).map_err(|e| error("io", e.error))?;
        Ok(())
    }
    pub(super) fn remember_pack_directory(&self, directory: &Path) -> Result<(), String> {
        let directory = std::fs::canonicalize(directory).map_err(|e| error("io", e))?;
        let mut settings = load(&self.root)?;
        settings.pack_directory = Some(crate::paths::display(&directory));
        self.save_runtime(&settings)?;
        let mut dirs = self.runtime.search_directories.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        dirs.truncate(1);
        dirs.push(directory);
        Ok(())
    }
}
impl Session {
    pub fn analysis_runtime_settings(&self) -> Result<AnalysisRuntimeSettings, String> {
        load(&self.inner.analysis.root)
    }
    pub fn analysis_runtime_configure(&self, mut settings: AnalysisRuntimeSettings) -> Result<AnalysisRuntimeSettings, String> {
        let service = &self.inner.analysis;
        let _gate = lock(&service.gate);
        if lock(&service.owned).closed { return Err(error("shutdown", "Analysis closed.")); }
        // The pack search directory comes only from an actual explicit import.
        settings.pack_directory = load(&service.root)?.pack_directory;
        if let Some(path) = &settings.library_path {
            let candidate = Path::new(path);
            if path.len() > 2048 || !candidate.is_absolute() || !candidate.is_file() {
                return Err(error("unavailable", "Choose an absolute path to an installed ONNX Runtime shared library."));
            }
            settings.library_path = Some(crate::paths::display(std::fs::canonicalize(candidate).map_err(|e| error("io", e))?));
        }
        if settings.model_id.is_some() != settings.model_version.is_some() { return Err(error("invalid", "Choose both model ID and version.")); }
        if let (Some(id), Some(version)) = (&settings.model_id, &settings.model_version) {
            service.restore_packs()?;
            let owned = lock(&service.owned);
            let model = owned.models.get(&(id.clone(), version.clone())).ok_or_else(|| error("modelAbsent", "Import the chosen model pack first."))?;
            native::separation::contract(model).map_err(native_error)?;
            if model.provenance.adapter_id != native::separation::ADAPTER_ID { return Err(error("invalid", "Choose an ONNX separation model.")); }
        }
        service.save_runtime(&settings)?;
        *service.runtime.choice.write().unwrap_or_else(std::sync::PoisonError::into_inner) = settings.library_path.as_ref().map(PathBuf::from);
        Ok(settings)
    }
}
