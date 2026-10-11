//! Provider-neutral immutable archive history. All IO is control-side.
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use windfall_ipc::BackupVersion;

pub const MAX_ARCHIVE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_VERSIONS: usize = 4096;

/// Adapters must provide an atomic compare-and-publish transaction, immutable
/// objects, verified downloads and cancellation before each publication boundary.
/// `expected` is the last head actually reviewed by the caller (None = empty).
pub trait BackupProvider: Send + Sync {
    fn versions(&self) -> Result<Vec<BackupVersion>, String>;
    fn publish(
        &self,
        archive: &Path,
        expected: Option<&str>,
        keep: usize,
        cancel: &AtomicBool,
    ) -> Result<BackupVersion, String>;
    fn restore(&self, id: &str, target: &Path, cancel: &AtomicBool) -> Result<(), String>;
}

pub struct FolderProvider {
    root: PathBuf,
}
struct FolderLock {
    path: PathBuf,
    file: Option<File>,
}
impl Drop for FolderLock {
    fn drop(&mut self) {
        self.file.take();
        let _ = fs::remove_file(&self.path);
    }
}
impl FolderProvider {
    /// The caller chooses a folder dedicated to one project history.
    pub fn new(root: &Path) -> Result<Self, String> {
        if !root.is_absolute() || !root.is_dir() {
            return Err("Choose an existing absolute backup folder.".into());
        }
        Ok(Self {
            root: fs::canonicalize(root).map_err(err)?,
        })
    }
    fn lock(&self) -> Result<FolderLock, String> {
        // Keep the open file too on Windows; lock ownership ends on close/remove.
        let path = self.root.join(".windfall-backup-lock");
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| {
                "Backup folder is busy. A stale lock after a crash requires review.".to_string()
            })?;
        Ok(FolderLock {
            path,
            file: Some(file),
        })
    }
    fn object(&self, id: &str) -> Result<PathBuf, String> {
        if id.len() != 64
            || !id
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err("Invalid backup version.".into());
        }
        Ok(self.root.join(format!("{id}.zip")))
    }
    fn history(&self) -> Result<Vec<BackupVersion>, String> {
        let path = self.root.join("windfall-history.json");
        let file = match File::open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(err(e)),
        };
        if file.metadata().map_err(err)?.len() > 2 * 1024 * 1024 {
            return Err("Backup history exceeds its limit.".into());
        }
        let rows: Vec<BackupVersion> =
            serde_json::from_reader(file.take(2 * 1024 * 1024 + 1)).map_err(err)?;
        if rows.len() > MAX_VERSIONS {
            return Err("Too many backup versions.".into());
        }
        let mut seen = std::collections::HashSet::new();
        for (index, row) in rows.iter().enumerate() {
            self.object(&row.id)?;
            self.object(&row.checksum)?;
            if let Some(parent) = &row.parent {
                self.object(parent)?;
            }
            if index > 0 && row.parent.as_ref() != Some(&rows[index - 1].id) {
                return Err("Backup history has a broken version chain.".into());
            }
            if row.bytes > MAX_ARCHIVE_BYTES || row.checksum.len() != 64 || !seen.insert(&row.id) {
                return Err("Invalid backup history.".into());
            }
        }
        Ok(rows)
    }
}
impl BackupProvider for FolderProvider {
    fn versions(&self) -> Result<Vec<BackupVersion>, String> {
        self.history()
    }
    fn publish(
        &self,
        archive: &Path,
        expected: Option<&str>,
        keep: usize,
        cancel: &AtomicBool,
    ) -> Result<BackupVersion, String> {
        if !(1..=1000).contains(&keep) {
            return Err("Retention must be 1 to 1000 versions.".into());
        }
        let _lock = self.lock()?;
        check(cancel)?;
        let mut rows = self.history()?;
        if rows.last().map(|r| r.id.as_str()) != expected {
            return Err(
                "Backup conflict: folder history changed. Refresh and review before retrying."
                    .into(),
            );
        }
        let mut staged = tempfile::NamedTempFile::new_in(&self.root).map_err(err)?;
        let (checksum, bytes) = copy_hash(archive, staged.as_file_mut(), cancel)?;
        staged.as_file().sync_all().map_err(err)?;
        let created_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(err)?
            .as_millis() as u64;
        let mut identity = Sha256::new();
        identity.update(checksum.as_bytes());
        identity.update(created_ms.to_le_bytes());
        identity.update(expected.unwrap_or("").as_bytes());
        let mut nonce = [0u8; 32];
        getrandom::fill(&mut nonce).map_err(err)?;
        identity.update(nonce);
        let version = BackupVersion {
            id: format!("{:x}", identity.finalize()),
            parent: expected.map(str::to_owned),
            checksum,
            bytes,
            created_ms,
        };
        let object = self.object(&version.id)?;
        check(cancel)?;
        staged.persist_noclobber(&object).map_err(err)?;
        rows.push(version.clone());
        let remove = rows.len().saturating_sub(keep);
        let expired: Vec<_> = rows.drain(..remove).collect();
        // Cancellation prior to head commit removes ONLY the new unreferenced object.
        let commit = (|| {
            check(cancel)?;
            let json = serde_json::to_string(&rows).map_err(err)?;
            windfall_project::file::write_atomic(
                &self.root.join("windfall-history.json"),
                json.as_bytes(),
            )
            .map_err(err)
        })();
        if let Err(e) = commit {
            let _ = fs::remove_file(object);
            return Err(e);
        }
        // Once the head commits, cancellation cannot roll back a published backup.
        // Pruning after commit can leave harmless orphan objects on IO failure.
        for old in expired {
            if let Ok(path) = self.object(&old.id)
                && let Err(e) = fs::remove_file(path)
            {
                log::warn!("Backup retention left an orphan: {e}");
            }
        }
        Ok(version)
    }
    fn restore(&self, id: &str, target: &Path, cancel: &AtomicBool) -> Result<(), String> {
        let _lock = self.lock()?;
        let row = self
            .history()?
            .into_iter()
            .find(|r| r.id == id)
            .ok_or("Backup version is no longer in history.")?;
        let parent = target
            .parent()
            .ok_or("Restore requires a destination folder.")?;
        let mut staged = tempfile::NamedTempFile::new_in(parent).map_err(err)?;
        let (checksum, bytes) = copy_hash(&self.object(id)?, staged.as_file_mut(), cancel)?;
        if checksum != row.checksum || bytes != row.bytes {
            return Err("Backup checksum mismatch; restore refused.".into());
        }
        staged.as_file().sync_all().map_err(err)?;
        check(cancel)?;
        staged.persist_noclobber(target).map_err(err)?;
        Ok(())
    }
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn check(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        Err("Backup cancelled.".into())
    } else {
        Ok(())
    }
}
fn copy_hash(
    source: &Path,
    out: &mut impl Write,
    cancel: &AtomicBool,
) -> Result<(String, u64), String> {
    let mut input = File::open(source).map_err(err)?;
    if input.metadata().map_err(err)?.len() > MAX_ARCHIVE_BYTES {
        return Err("Backup archive exceeds 2 GiB.".into());
    }
    let mut hash = Sha256::new();
    let mut total = 0u64;
    let mut chunk = [0u8; 64 * 1024];
    loop {
        check(cancel)?;
        let n = input.read(&mut chunk).map_err(err)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > MAX_ARCHIVE_BYTES {
            return Err("Backup archive grew beyond its limit.".into());
        }
        hash.update(&chunk[..n]);
        out.write_all(&chunk[..n]).map_err(err)?;
    }
    Ok((format!("{:x}", hash.finalize()), total))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_sha256_reference_and_mid_chunk_cancel() {
        let folder = tempfile::tempdir().unwrap();
        let source = folder.path().join("source.zip");
        fs::write(&source, b"abc").unwrap();
        let cancel = AtomicBool::new(false);
        let mut output = Vec::new();
        let (checksum, bytes) = copy_hash(&source, &mut output, &cancel).unwrap();
        assert_eq!(
            checksum,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(bytes, 3);
        assert_eq!(output, b"abc");
        fs::write(&source, vec![0; 128 * 1024]).unwrap();
        struct CancelWriter<'a>(&'a AtomicBool, usize);
        impl Write for CancelWriter<'_> {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.1 += bytes.len();
                self.0.store(true, Ordering::Release);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut writer = CancelWriter(&cancel, 0);
        assert!(
            copy_hash(&source, &mut writer, &cancel)
                .unwrap_err()
                .contains("cancelled")
        );
        assert_eq!(writer.1, 64 * 1024);
    }
    #[test]
    fn checksum_conflict_cancel_retention_and_restore_round_trip() {
        let folder = tempfile::tempdir().unwrap();
        let provider = FolderProvider::new(folder.path()).unwrap();
        let source = folder.path().join("source.zip");
        fs::write(&source, b"portable archive fixture").unwrap();
        let cancel = AtomicBool::new(false);
        let first = provider.publish(&source, None, 2, &cancel).unwrap();
        assert_eq!(first.checksum.len(), 64);
        assert!(
            provider
                .publish(&source, None, 2, &cancel)
                .unwrap_err()
                .contains("conflict")
        );
        let second = provider
            .publish(&source, Some(&first.id), 1, &cancel)
            .unwrap();
        assert!(!provider.object(&first.id).unwrap().exists());
        assert_eq!(provider.versions().unwrap().len(), 1);
        let restored = folder.path().join("restore.zip");
        provider.restore(&second.id, &restored, &cancel).unwrap();
        assert_eq!(fs::read(&restored).unwrap(), fs::read(&source).unwrap());
        assert!(provider.restore(&second.id, &restored, &cancel).is_err());
        cancel.store(true, Ordering::Release);
        assert!(
            provider
                .publish(&source, Some(&second.id), 1, &cancel)
                .is_err()
        );
        assert_eq!(provider.versions().unwrap().last().unwrap().id, second.id);
        cancel.store(false, Ordering::Release);
        fs::write(provider.object(&second.id).unwrap(), b"corrupted").unwrap();
        let bad = folder.path().join("bad.zip");
        assert!(
            provider
                .restore(&second.id, &bad, &cancel)
                .unwrap_err()
                .contains("checksum")
        );
        assert!(!bad.exists());
    }
}
