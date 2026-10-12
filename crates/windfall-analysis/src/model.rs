use crate::{AnalysisError, AudioShape, CHUNK_BYTES, Result, Work, add, lock};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tempfile::NamedTempFile;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelProvenance {
    pub origin: String,
    pub source_revision: String,
    pub author: String,
    pub license_spdx: String,
    pub license_reference: String,
    pub adapter_id: String,
    pub adapter_version: String,
    /// This first contract requires a deterministic CPU adapter.
    pub device: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputRole {
    pub role: String,
    pub channels: u16,
}

/// Declarative ONNX contract, not an executable runtime installation. A symbolic
/// dimension is explicit (e.g. `frames`); unknown/zero dimensions are refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TensorDimension {
    Fixed(u32),
    Symbol(String),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TensorSpec {
    pub name: String,
    pub shape: Vec<TensorDimension>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnnxContract {
    pub opset: u32,
    pub inputs: Vec<TensorSpec>,
    pub outputs: Vec<TensorSpec>,
    pub sample_rate: u32,
    pub segment_frames: u32,
    pub overlap_frames: u32,
    /// Order of the model's stem dimension, independently of tensor names.
    pub stem_roles: Vec<String>,
}
impl OnnxContract {
    pub fn validate(&self, model: &ModelManifest) -> Result<()> {
        if !(1..=64).contains(&self.opset)
            || self.sample_rate != model.sample_rate
            || self.segment_frames == 0
            || self.segment_frames as u64 > model.max_input_frames
            || self.overlap_frames >= self.segment_frames
            || self.stem_roles.len() != model.outputs.len()
            || self.stem_roles.iter().enumerate().any(|(i, r)| {
                !model.outputs.iter().any(|o| &o.role == r) || self.stem_roles[..i].contains(r)
            })
        {
            return Err(AnalysisError::Invalid("ONNX segment/rate/stem contract"));
        }
        for tensors in [&self.inputs, &self.outputs] {
            if tensors.is_empty() || tensors.len() > 16 {
                return Err(AnalysisError::Invalid("ONNX tensor count"));
            }
            for (i, tensor) in tensors.iter().enumerate() {
                if tensor.name.is_empty()
                    || tensor.name.len() > 128
                    || tensor.name.chars().any(char::is_control)
                    || tensors[..i].iter().any(|t| t.name == tensor.name)
                    || tensor.shape.is_empty()
                    || tensor.shape.len() > 8
                    || tensor.shape.iter().any(|d| match d {
                        TensorDimension::Fixed(n) => *n == 0 || *n > 16_777_216,
                        TensorDimension::Symbol(s) => {
                            s.is_empty()
                                || s.len() > 64
                                || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                        }
                    })
                {
                    return Err(AnalysisError::Invalid("ONNX tensor name/shape"));
                }
            }
        }
        Ok(())
    }
    fn storage_bytes(&self, capacity: bool) -> Result<u64> {
        let mut bytes = 0;
        for tensors in [&self.inputs, &self.outputs] {
            bytes = add(
                bytes,
                (if capacity {
                    tensors.capacity()
                } else {
                    tensors.len()
                }) as u64
                    * size_of::<TensorSpec>() as u64,
            )?;
            for t in tensors {
                bytes = add(
                    bytes,
                    (if capacity {
                        t.name.capacity()
                    } else {
                        t.name.len()
                    }) as u64,
                )?;
                bytes = add(
                    bytes,
                    (if capacity {
                        t.shape.capacity()
                    } else {
                        t.shape.len()
                    }) as u64
                        * size_of::<TensorDimension>() as u64,
                )?;
                for d in &t.shape {
                    if let TensorDimension::Symbol(s) = d {
                        bytes = add(
                            bytes,
                            (if capacity { s.capacity() } else { s.len() }) as u64,
                        )?;
                    }
                }
            }
        }
        bytes = add(
            bytes,
            (if capacity {
                self.stem_roles.capacity()
            } else {
                self.stem_roles.len()
            }) as u64
                * size_of::<String>() as u64,
        )?;
        for s in &self.stem_roles {
            bytes = add(
                bytes,
                (if capacity { s.capacity() } else { s.len() }) as u64,
            )?;
        }
        Ok(bytes)
    }
}

/// A fixed, fully specified local model. Shapes are exact; outputs preserve
/// selected frame count, sample rate and original selection frame origin.
/// Weight licensing is recorded independently of an adapter's code license.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelManifest {
    pub id: String,
    pub version: String,
    pub revision: u64,
    pub sha256: [u8; 32],
    pub bytes: u64,
    pub max_bytes: u64,
    pub provenance: ModelProvenance,
    pub sample_rate: u32,
    pub input_channels: u16,
    pub max_input_frames: u64,
    pub outputs: Vec<OutputRole>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub onnx: Option<OnnxContract>,
}
impl ModelManifest {
    /// Explicit code descriptor. Imported manifests may never claim this ID.
    pub fn is_builtin_dereverb(&self) -> bool {
        self.id == crate::BUILTIN_DEREVERB_ID
            && self.version == "1"
            && self.revision == 1
            && self.bytes == 0
            && self.max_bytes == 0
            && self.sha256 == [0; 32]
            && self.onnx.is_none()
            && self.provenance.adapter_id == crate::BUILTIN_DEREVERB_ID
            && self.provenance.adapter_version == "1"
            && self.provenance.origin == "builtin:windfall/classical-dereverb"
            && self.outputs.len() == 1
            && matches!(
                self.outputs[0].role.as_str(),
                "dereverb-general" | "dereverb-vocal"
            )
            && self.outputs[0].channels == self.input_channels
    }
    /// Actual retained heap capacity, including unused String/Vec capacity.
    /// Admission also reserves two compact copies used during worker loading.
    pub fn retained_bytes(&self) -> Result<u64> {
        self.storage_bytes(true)
    }
    pub(crate) fn copy_bytes(&self) -> Result<u64> {
        self.storage_bytes(false)
    }
    fn storage_bytes(&self, capacity: bool) -> Result<u64> {
        let p = &self.provenance;
        let mut bytes = (if capacity {
            self.outputs.capacity()
        } else {
            self.outputs.len()
        })
        .checked_mul(size_of::<OutputRole>())
        .ok_or(AnalysisError::Exhausted)? as u64;
        for field in [
            &self.id,
            &self.version,
            &p.origin,
            &p.source_revision,
            &p.author,
            &p.license_spdx,
            &p.license_reference,
            &p.adapter_id,
            &p.adapter_version,
            &p.device,
        ] {
            bytes = add(
                bytes,
                (if capacity {
                    field.capacity()
                } else {
                    field.len()
                }) as u64,
            )?;
        }
        for output in &self.outputs {
            bytes = add(
                bytes,
                (if capacity {
                    output.role.capacity()
                } else {
                    output.role.len()
                }) as u64,
            )?;
        }
        if let Some(onnx) = &self.onnx {
            bytes = add(bytes, onnx.storage_bytes(capacity)?)?;
        }
        Ok(bytes)
    }
    pub fn validate(&self) -> Result<()> {
        let p = &self.provenance;
        for field in [
            &self.id,
            &self.version,
            &p.origin,
            &p.source_revision,
            &p.author,
            &p.license_spdx,
            &p.license_reference,
            &p.adapter_id,
            &p.adapter_version,
        ] {
            if field.is_empty() || field.len() > 1024 || field.chars().any(char::is_control) {
                return Err(AnalysisError::Invalid("model provenance fields"));
            }
        }
        if (self.bytes == 0 && !self.is_builtin_dereverb())
            || self.bytes > self.max_bytes
            || self.max_bytes > usize::MAX as u64
            || self.sample_rate < 8_000
            || self.sample_rate > 192_000
            || !(1..=2).contains(&self.input_channels)
            || self.max_input_frames == 0
            || self.outputs.is_empty()
            || self.outputs.len() > 16
            || p.device != "cpu"
        {
            return Err(AnalysisError::Invalid("model byte/shape/device bounds"));
        }
        for (i, output) in self.outputs.iter().enumerate() {
            if output.role.is_empty()
                || output.role.len() > 64
                || !output
                    .role
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                || !(1..=2).contains(&output.channels)
                || self.outputs[..i].iter().any(|r| r.role == output.role)
            {
                return Err(AnalysisError::Invalid("model output roles"));
            }
        }
        if let Some(onnx) = &self.onnx {
            onnx.validate(self)?;
        }
        Ok(())
    }
    pub fn validate_input(&self, shape: AudioShape, frames: u64) -> Result<()> {
        self.validate()?;
        if shape.sample_rate != self.sample_rate
            || shape.channels != self.input_channels
            || frames == 0
            || frames > self.max_input_frames
        {
            return Err(AnalysisError::Invalid("model/input shape mismatch"));
        }
        Ok(())
    }
    pub(crate) fn cache_name(&self) -> String {
        let mut name = String::with_capacity(70);
        for byte in self.sha256 {
            use std::fmt::Write;
            let _ = write!(name, "{byte:02x}");
        }
        name.push_str(".model");
        name
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ModelCacheLimits {
    pub max_model_bytes: u64,
    pub max_cache_bytes: u64,
    pub max_entries: usize,
    pub max_imports: usize,
}
impl Default for ModelCacheLimits {
    fn default() -> Self {
        Self {
            max_model_bytes: 256 * 1024 * 1024,
            max_cache_bytes: 1024 * 1024 * 1024,
            max_entries: 32,
            max_imports: 1,
        }
    }
}
struct CacheState {
    importing: usize,
    reserved: u64,
}
struct CacheInner {
    root: PathBuf,
    limits: ModelCacheLimits,
    state: Mutex<CacheState>,
}

/// One control authority per dedicated cache directory; Clone shares its
/// reservations. External competitors are never replaced/deleted. A cache
/// hit is verified from content, not accepted from name/metadata alone.
#[derive(Clone)]
pub struct ModelCache(Arc<CacheInner>);

pub struct VerifiedModel {
    manifest: ModelManifest,
    bytes: Box<[u8]>,
}
impl VerifiedModel {
    pub(crate) fn builtin(manifest: ModelManifest) -> Self {
        Self {
            manifest,
            bytes: Box::new([]),
        }
    }
    pub fn manifest(&self) -> &ModelManifest {
        &self.manifest
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl ModelCache {
    pub fn new(root: impl AsRef<Path>, limits: ModelCacheLimits) -> Result<Self> {
        if limits.max_model_bytes == 0
            || limits.max_model_bytes > limits.max_cache_bytes
            || limits.max_entries == 0
            || limits.max_imports == 0
            || limits.max_model_bytes > usize::MAX as u64
        {
            return Err(AnalysisError::Invalid("model cache bounds"));
        }
        fs::create_dir_all(root.as_ref())?;
        let root = fs::canonicalize(root)?;
        if root.as_os_str().len() > 1024 {
            return Err(AnalysisError::Invalid("cache root path length"));
        }
        inventory(&root, limits)?;
        Ok(Self(Arc::new(CacheInner {
            root,
            limits,
            state: Mutex::new(CacheState {
                importing: 0,
                reserved: 0,
            }),
        })))
    }
    pub fn root(&self) -> &Path {
        &self.0.root
    }
    fn validate(&self, manifest: &ModelManifest) -> Result<()> {
        manifest.validate()?;
        if manifest.max_bytes > self.0.limits.max_model_bytes {
            return Err(AnalysisError::Budget("model bytes"));
        }
        Ok(())
    }
    /// Explicit local import. The supplied reader runs on the calling worker;
    /// streaming reads/writes/hash checks are chunked and cancellable. No URL
    /// fetching, uploads, weights discovery or automatic replacement exists.
    pub fn import_reader(
        &self,
        manifest: &ModelManifest,
        mut reader: impl Read,
        work: &mut Work,
    ) -> Result<PathBuf> {
        if manifest.id == crate::BUILTIN_DEREVERB_ID {
            return Err(AnalysisError::Invalid(
                "built-in algorithm cannot be imported as a weight pack",
            ));
        }
        self.validate(manifest)?;
        work.check()?;
        let target = self.0.root.join(manifest.cache_name());
        if fs::symlink_metadata(&target).is_ok() {
            verify_file(&target, manifest, work, false)?;
            return Ok(target);
        }
        let reservation = {
            let mut state = lock(&self.0.state);
            let (used, entries) = inventory(&self.0.root, self.0.limits)?;
            if state.importing >= self.0.limits.max_imports
                || entries + state.importing >= self.0.limits.max_entries
            {
                return Err(AnalysisError::Budget("model import/entry count"));
            }
            if add(add(used, state.reserved)?, manifest.bytes)? > self.0.limits.max_cache_bytes {
                return Err(AnalysisError::Budget("model cache disk"));
            }
            state.importing += 1;
            state.reserved += manifest.bytes;
            ImportReservation {
                cache: self.clone(),
                bytes: manifest.bytes,
            }
        };
        let mut staged = NamedTempFile::new_in(&self.0.root)?;
        let mut chunk = [0; CHUNK_BYTES];
        let mut bytes = 0;
        let mut sha = Sha256::new();
        loop {
            work.check()?;
            let n = reader.read(&mut chunk)?;
            if n == 0 {
                break;
            }
            bytes = add(bytes, n as u64)?;
            if bytes > manifest.bytes {
                return Err(AnalysisError::ModelSize);
            }
            work.checkpoint(n as u64)?;
            staged.write_all(&chunk[..n])?;
            sha.update(&chunk[..n]);
        }
        if bytes != manifest.bytes {
            return Err(AnalysisError::ModelSize);
        }
        if <[u8; 32]>::from(sha.finalize()) != manifest.sha256 {
            return Err(AnalysisError::Checksum);
        }
        staged.as_file().sync_all()?;
        work.check()?;
        match staged.persist_noclobber(&target) {
            Ok(file) => {
                drop(file);
            }
            Err(error)
                if error.error.kind() == std::io::ErrorKind::AlreadyExists
                    || fs::symlink_metadata(&target).is_ok() =>
            {
                // `error.file` owns only our staging. A competing final file is
                // left byte-for-byte intact, then checked rather than trusted.
                drop(error.file);
                verify_file(&target, manifest, work, false)?;
            }
            Err(error) => return Err(AnalysisError::Io(error.error)),
        }
        drop(reservation);
        Ok(target)
    }
    pub fn import_file(
        &self,
        manifest: &ModelManifest,
        path: impl AsRef<Path>,
        work: &mut Work,
    ) -> Result<PathBuf> {
        self.import_reader(manifest, File::open(path)?, work)
    }
    /// Rehash and copy to bounded immutable memory. A worker never infers from
    /// a mutable pathname or trusts a prior import's checksum check.
    pub fn load(&self, manifest: &ModelManifest, work: &mut Work) -> Result<VerifiedModel> {
        if manifest.id == crate::BUILTIN_DEREVERB_ID {
            return Err(AnalysisError::Invalid(
                "built-in algorithm is not an imported pack",
            ));
        }
        self.validate(manifest)?;
        let bytes = verify_file(
            &self.0.root.join(manifest.cache_name()),
            manifest,
            work,
            true,
        )?;
        Ok(VerifiedModel {
            manifest: manifest.clone(),
            bytes: bytes.into_boxed_slice(),
        })
    }
}

struct ImportReservation {
    cache: ModelCache,
    bytes: u64,
}
impl Drop for ImportReservation {
    fn drop(&mut self) {
        let mut s = lock(&self.cache.0.state);
        s.importing -= 1;
        s.reserved -= self.bytes;
    }
}

fn verify_file(
    path: &Path,
    manifest: &ModelManifest,
    work: &mut Work,
    retain: bool,
) -> Result<Vec<u8>> {
    work.check()?;
    let metadata = fs::symlink_metadata(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AnalysisError::ModelAbsent
        } else {
            e.into()
        }
    })?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(AnalysisError::Invalid(
            "model cache entry is not a regular file",
        ));
    }
    let mut file = File::open(path)?;
    if file.metadata()?.len() != manifest.bytes {
        return Err(AnalysisError::ModelSize);
    }
    let mut data = Vec::new();
    if retain {
        data.try_reserve_exact(manifest.bytes as usize)
            .map_err(|_| AnalysisError::Budget("model allocation"))?;
        if data.capacity() as u64 > manifest.bytes {
            return Err(AnalysisError::Budget("model capacity"));
        }
    }
    let mut chunk = [0; CHUNK_BYTES];
    let mut sha = Sha256::new();
    let mut bytes = 0;
    loop {
        work.check()?;
        let n = file.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        bytes = add(bytes, n as u64)?;
        if bytes > manifest.bytes {
            return Err(AnalysisError::ModelSize);
        }
        work.checkpoint(n as u64)?;
        sha.update(&chunk[..n]);
        if retain {
            data.extend_from_slice(&chunk[..n]);
        }
    }
    work.check()?;
    if bytes != manifest.bytes {
        return Err(AnalysisError::ModelSize);
    }
    if <[u8; 32]>::from(sha.finalize()) != manifest.sha256 {
        return Err(AnalysisError::Checksum);
    }
    Ok(data)
}

pub(crate) fn inventory(root: &Path, limits: ModelCacheLimits) -> Result<(u64, usize)> {
    let mut bytes = 0;
    let mut count = 0;
    for entry in fs::read_dir(root)? {
        count += 1;
        if count > limits.max_entries {
            return Err(AnalysisError::Budget("directory entry count"));
        }
        let metadata = fs::symlink_metadata(entry?.path())?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(AnalysisError::Invalid("cache/output directory entry"));
        }
        bytes = add(bytes, metadata.len())?;
        if bytes > limits.max_cache_bytes {
            return Err(AnalysisError::Budget("directory disk bytes"));
        }
    }
    Ok((bytes, count))
}
