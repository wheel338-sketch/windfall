//! Session-independent, bounded metadata next to (never inside) the byte cache.
//! Imported metadata is data only; reopening never installs executable code.
use super::*;
use std::io::Write;
const LIMIT: u64 = 128 * 1024;
#[derive(serde::Serialize, serde::Deserialize)]
struct Registry {
    version: u32,
    models: Vec<AnalysisModel>,
}
impl Service {
    pub(super) fn restore_packs(&self) -> Result<(), String> {
        if lock(&self.owned).registry_loaded {
            return Ok(());
        }
        let path = self.root.join("packs.json");
        let file = match std::fs::File::open(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                lock(&self.owned).registry_loaded = true;
                return Ok(());
            }
            Err(e) => return Err(error("io", e)),
        };
        if std::fs::symlink_metadata(&path)
            .map_err(|e| error("io", e))?
            .file_type()
            .is_symlink()
        {
            return Err(error("invalid", "Pack registry cannot be a symlink."));
        }
        let mut bytes = Vec::new();
        file.take(LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| error("io", e))?;
        if bytes.len() as u64 > LIMIT {
            return Err(error("budget", "Pack registry exceeds 128 KiB."));
        }
        let registry: Registry = serde_json::from_slice(&bytes).map_err(|e| error("invalid", e))?;
        if registry.version != 1 || registry.models.len() > 8 {
            return Err(error(
                "invalid",
                "Unsupported pack registry version or count.",
            ));
        }
        let cache = self.cache()?;
        let mut models = BTreeMap::new();
        for spec in registry.models {
            let model = manifest(spec)?;
            // Hash content again on every app restart. No network fallback.
            cache
                .load(&model, &mut work(native::CancelToken::default())?)
                .map_err(native_error)?;
            let key = (model.id.clone(), model.version.clone());
            if models.insert(key, model).is_some() {
                return Err(error("invalid", "Duplicate registered pack."));
            }
        }
        let mut owned = lock(&self.owned);
        owned.models = models;
        owned.registry_loaded = true;
        Ok(())
    }
    pub(super) fn persist_packs(
        &self,
        models: &BTreeMap<(String, String), native::ModelManifest>,
    ) -> Result<(), String> {
        let bytes = serde_json::to_vec(&Registry {
            version: 1,
            models: models.values().map(model_wire).collect(),
        })
        .map_err(|e| error("invalid", e))?;
        if bytes.len() as u64 > LIMIT {
            return Err(error("budget", "Pack registry exceeds 128 KiB."));
        }
        std::fs::create_dir_all(&self.root).map_err(|e| error("io", e))?;
        let path = self.root.join("packs.json");
        if let Ok(meta) = std::fs::symlink_metadata(&path)
            && (!meta.is_file() || meta.file_type().is_symlink())
        {
            return Err(error(
                "invalid",
                "Pack registry target is not a regular file.",
            ));
        }
        let mut staged = tempfile::NamedTempFile::new_in(&self.root).map_err(|e| error("io", e))?;
        staged.write_all(&bytes).map_err(|e| error("io", e))?;
        staged.as_file().sync_all().map_err(|e| error("io", e))?;
        staged.persist(&path).map_err(|e| error("io", e.error))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pack_registry_restart_keeps_onnx_contract_and_rechecks_checksum() {
        let root = tempfile::tempdir().unwrap();
        let service = Service::new(root.path().join("Analysis"));
        let weights = root.path().join("authored.onnx");
        // Authored bytes exercise registration only, never ONNX inference.
        std::fs::write(&weights, b"authored").unwrap();
        let fingerprint = native::ContentFingerprint::reader(
            std::fs::File::open(&weights).unwrap(),
            32,
            &mut work(native::CancelToken::default()).unwrap(),
        )
        .unwrap();
        let spec = native::ModelManifest {
            id: "authored-pack".into(),
            version: "1".into(),
            revision: 1,
            sha256: fingerprint.sha256,
            bytes: 8,
            max_bytes: 32,
            provenance: native::ModelProvenance {
                origin: "authored".into(),
                source_revision: "1".into(),
                author: "Windfall".into(),
                license_spdx: "CC0-1.0".into(),
                license_reference: "authored fixture".into(),
                adapter_id: "onnx-future".into(),
                adapter_version: "1".into(),
                device: "cpu".into(),
            },
            sample_rate: 48_000,
            input_channels: 2,
            max_input_frames: 48_000,
            outputs: vec![native::OutputRole {
                role: "vocals".into(),
                channels: 2,
            }],
            onnx: Some(native::OnnxContract {
                layout: Default::default(),
                context_frames: 0,
                opset: 17,
                inputs: vec![native::TensorSpec {
                    name: "audio".into(),
                    shape: vec![
                        native::TensorDimension::Fixed(1),
                        native::TensorDimension::Fixed(2),
                        native::TensorDimension::Symbol("frames".into()),
                    ],
                }],
                outputs: vec![native::TensorSpec {
                    name: "stems".into(),
                    shape: vec![
                        native::TensorDimension::Fixed(1),
                        native::TensorDimension::Fixed(2),
                        native::TensorDimension::Symbol("frames".into()),
                    ],
                }],
                sample_rate: 48_000,
                segment_frames: 24_000,
                overlap_frames: 6_000,
                stem_roles: vec!["vocals".into()],
            }),
        };
        service
            .cache()
            .unwrap()
            .import_file(
                &spec,
                &weights,
                &mut work(native::CancelToken::default()).unwrap(),
            )
            .unwrap();
        let models = BTreeMap::from([((spec.id.clone(), spec.version.clone()), spec.clone())]);
        service.persist_packs(&models).unwrap();
        drop(service);
        let reopened = Service::new(root.path().join("Analysis"));
        reopened.restore_packs().unwrap();
        assert_eq!(lock(&reopened.owned).models, models);
        let cache_file = std::fs::read_dir(root.path().join("Analysis/Models"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        std::fs::write(cache_file, b"tampered").unwrap();
        let corrupted = Service::new(root.path().join("Analysis"));
        assert!(corrupted.restore_packs().unwrap_err().contains("checksum"));
        assert!(lock(&corrupted.owned).models.is_empty());
    }
}
