//! Per-user sample library. One cancellable filesystem worker, independent of the document,
//! audio callback and device settings. The index is disposable; only metadata is persisted.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use windfall_ipc::{
    BrowserEntry, BrowserRoot, LibraryEntry, LibraryFileToken, LibraryMetadata, LibraryQuery,
    LibraryResults, LibrarySearch, LibraryStatus, MAX_RESULTS, normalize_tags,
};
use windfall_project::file::write_atomic;

use crate::{browser, paths, sync::lock};

pub const METADATA_FILE: &str = "browser-library.json";
pub const MAX_ENTRIES: usize = 50_000;
pub const MAX_EXAMINED: usize = 100_000;
pub const MAX_PATH_BYTES: usize = 4096;
pub const MAX_TOTAL_PATH_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_DEPTH: usize = 64;
pub const MAX_ROOTS: usize = 128;
pub const MAX_ISSUES: usize = 32;
const MAX_METADATA: usize = 10_000;
const MAX_METADATA_BYTES: u64 = 8 * 1024 * 1024;
const BATCH: usize = 128;
const SCAN_TIME: Duration = Duration::from_secs(30);
pub const STALE_FILE: &str =
    "This library result is out of date. Refresh the library and select the file again.";

fn path_key(path: &str) -> String {
    if cfg!(windows) {
        path.to_lowercase()
    } else {
        path.to_owned()
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Metadata {
    version: u32,
    files: BTreeMap<String, LibraryMetadata>,
}

struct MetadataStore {
    file: PathBuf,
    data: Metadata,
    error: Option<String>,
}

impl MetadataStore {
    fn load(file: PathBuf) -> Self {
        let read = || -> Result<Metadata, String> {
            let metadata = match fs::metadata(&file) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(Metadata::default());
                }
                result => result.map_err(|e| e.to_string())?,
            };
            if metadata.len() > MAX_METADATA_BYTES {
                return Err("metadata file exceeds 8 MiB".into());
            }
            let mut bytes = Vec::new();
            fs::File::open(&file)
                .map_err(|e| e.to_string())?
                .take(MAX_METADATA_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() as u64 > MAX_METADATA_BYTES {
                return Err("metadata file exceeds 8 MiB".into());
            }
            let mut data: Metadata = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if data.version > 1 || data.files.len() > MAX_METADATA {
                return Err("unsupported version or too many metadata entries".into());
            }
            let mut clean = BTreeMap::new();
            for (path, mut meta) in std::mem::take(&mut data.files) {
                if path.len() > MAX_PATH_BYTES || !Path::new(&path).is_absolute() {
                    return Err("invalid metadata path".into());
                }
                meta.tags = normalize_tags(&meta.tags)?;
                clean.insert(
                    path_key(&paths::display(&paths::clean(Path::new(&path)))),
                    meta,
                );
            }
            data.files = clean;
            Ok(data)
        };
        match read() {
            Ok(data) => Self {
                file,
                data,
                error: None,
            },
            Err(e) => Self {
                error: Some(windfall_ipc::ui_message(
                    "library.metadataUnreadable",
                    serde_json::json!({"error": e.to_string(), "file": paths::display(&file)}),
                )),
                file,
                data: Metadata::default(),
            },
        }
    }

    fn set(&mut self, path: &str, mut meta: LibraryMetadata) -> Result<LibraryMetadata, String> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        meta.tags = normalize_tags(&meta.tags)?;
        let key = path_key(path);
        let mut next = self.data.files.clone();
        if meta == LibraryMetadata::default() {
            next.remove(&key);
        } else {
            next.insert(key, meta.clone());
        }
        if next.len() > MAX_METADATA {
            return Err("The library has reached its 10,000 annotated-file limit. Remove unused favorites or tags first.".into());
        }
        let json = serde_json::to_vec_pretty(&Metadata {
            version: 1,
            files: next.clone(),
        })
        .map_err(|e| e.to_string())?;
        if json.len() as u64 > MAX_METADATA_BYTES {
            return Err("The library metadata has reached its 8 MiB limit.".into());
        }
        write_atomic(&self.file, &json).map_err(|e| {
            windfall_ipc::ui_message(
                "library.saveFailed",
                serde_json::json!({"file": paths::display(&self.file), "error": e.to_string()}),
            )
        })?;
        self.data.files = next;
        self.data.version = 1;
        Ok(meta)
    }
}

#[derive(Clone)]
struct Indexed {
    entry: BrowserEntry,
    relative: String,
    root: String,
    fingerprint: String,
}

struct Index {
    roots: Vec<BrowserRoot>,
    generation: u32,
    status: LibraryStatus,
    entries: Vec<Arc<Indexed>>,
    examined: usize,
    truncated: bool,
    root_limit: bool,
    issues: Vec<String>,
}

struct Work {
    pending: Option<(u32, Vec<BrowserRoot>)>,
    shutdown: bool,
}

struct Control {
    work: Mutex<Work>,
    wake: Condvar,
    current: AtomicU32,
}

pub struct Library {
    index: Arc<Mutex<Index>>,
    control: Arc<Control>,
    metadata: Mutex<MetadataStore>,
    worker_error: Option<String>,
}

impl Drop for Library {
    fn drop(&mut self) {
        self.control.current.fetch_add(1, Ordering::AcqRel);
        lock(&self.control.work).shutdown = true;
        self.control.wake.notify_one();
    }
}

impl Library {
    pub fn new(metadata_file: PathBuf, roots: Vec<BrowserRoot>) -> Self {
        let index = Arc::new(Mutex::new(Index {
            roots: Vec::new(),
            generation: 0,
            status: LibraryStatus::Ready,
            entries: Vec::new(),
            examined: 0,
            truncated: false,
            root_limit: false,
            issues: Vec::new(),
        }));
        let control = Arc::new(Control {
            work: Mutex::new(Work {
                pending: None,
                shutdown: false,
            }),
            wake: Condvar::new(),
            current: AtomicU32::new(0),
        });
        let worker_index = index.clone();
        let worker_control = control.clone();
        let worker = std::thread::Builder::new()
            .name("windfall-library".into())
            .spawn(move || worker(worker_index, worker_control));
        let worker_error = worker.err().map(|e| {
            windfall_ipc::ui_message(
                "library.workerFailed",
                serde_json::json!({"error": e.to_string()}),
            )
        });
        let library = Self {
            index,
            control,
            metadata: Mutex::new(MetadataStore::load(metadata_file)),
            worker_error,
        };
        library.set_roots(roots);
        library
    }

    /// Immediately invalidates old results, then queues the newest roots for the single worker.
    pub fn set_roots(&self, mut roots: Vec<BrowserRoot>) {
        let truncated =
            roots.len() > MAX_ROOTS || roots.iter().any(|r| r.path.len() > MAX_PATH_BYTES);
        roots.truncate(MAX_ROOTS);
        roots.retain(|r| r.path.len() <= MAX_PATH_BYTES);
        let mut index = lock(&self.index);
        self.restart(&mut index, roots, truncated);
    }

    fn restart(&self, index: &mut Index, roots: Vec<BrowserRoot>, truncated: bool) {
        let generation = self
            .control
            .current
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1);
        *index = Index {
            roots: roots.clone(),
            generation,
            status: LibraryStatus::Indexing,
            entries: Vec::new(),
            examined: 0,
            truncated,
            root_limit: truncated,
            issues: Vec::new(),
        };
        if let Some(error) = &self.worker_error {
            index.status = LibraryStatus::Ready;
            index.issues.push(error.clone());
            return;
        }
        lock(&self.control.work).pending = Some((generation, roots));
        self.control.wake.notify_one();
    }

    pub fn refresh(&self) {
        // Keep root selection and invalidation in one critical section: a concurrent
        // removal must never be followed by refresh reintroducing its old snapshot.
        let mut index = lock(&self.index);
        let roots = index.roots.clone();
        let root_limit = index.root_limit;
        self.restart(&mut index, roots, root_limit);
    }

    pub fn cancel(&self, generation: u32) {
        let mut index = lock(&self.index);
        if index.generation != generation || index.status != LibraryStatus::Indexing {
            return;
        }
        self.control.current.fetch_add(1, Ordering::AcqRel);
        lock(&self.control.work).pending = None;
        index.status = LibraryStatus::Cancelled;
    }

    pub fn search(&self, search: &LibrarySearch) -> Result<LibraryResults, String> {
        let query = LibraryQuery::parse(&search.query)?;
        let tags = normalize_tags(&search.tags)?;
        let (mut result, entries) = {
            let index = lock(&self.index);
            (
                LibraryResults {
                    generation: index.generation,
                    status: index.status,
                    indexed: index.entries.len() as u32,
                    examined: index.examined as u32,
                    entries: Vec::new(),
                    truncated: index.truncated,
                    results_truncated: false,
                    issues: index.issues.clone(),
                    available_tags: Vec::new(),
                    limitation: None,
                },
                index.entries.clone(),
            )
        };
        let (metadata, error) = {
            let store = lock(&self.metadata);
            (store.data.files.clone(), store.error.clone())
        };
        if let Some(error) = error {
            result.issues.push(error);
        }
        let mut available = BTreeSet::new();
        // Search CPU and sorting happen outside every session/index lock.
        let mut matches = Vec::new();
        let started = Instant::now();
        for (position, held) in entries.into_iter().enumerate() {
            if position % BATCH == 0 {
                if lock(&self.index).generation != result.generation {
                    return Err(STALE_FILE.into());
                }
                if started.elapsed() > Duration::from_secs(2) {
                    return Err("This search is too expensive. Use fewer wildcard terms or smaller library folders.".into());
                }
            }
            let meta = metadata
                .get(&path_key(&held.entry.path))
                .cloned()
                .unwrap_or_default();
            available.extend(meta.tags.iter().cloned());
            if search.favorites_only && !meta.favorite
                || !tags.iter().all(|t| meta.tags.contains(t))
                || !query.matches(&held.entry.path)
            {
                continue;
            }
            matches.push((held, meta));
        }
        matches.sort_by(|(a, _), (b, _)| {
            browser::natural_cmp(&a.relative, &b.relative)
                .then_with(|| a.entry.path.cmp(&b.entry.path))
        });
        result.results_truncated = matches.len() > MAX_RESULTS;
        for (held, metadata) in matches.into_iter().take(MAX_RESULTS) {
            result.entries.push(LibraryEntry {
                entry: held.entry.clone(),
                relative_path: held.relative.clone(),
                token: LibraryFileToken {
                    path: held.entry.path.clone(),
                    root_path: held.root.clone(),
                    generation: result.generation,
                    fingerprint: held.fingerprint.clone(),
                },
                metadata,
            });
        }
        result.available_tags = available.into_iter().take(256).collect();
        // A root removed while search was calculating cannot leak its old results.
        if lock(&self.index).generation != result.generation {
            return Err(STALE_FILE.into());
        }
        Ok(result)
    }

    /// Makes a token for a manually browsed file too, without requiring the index to be finished.
    pub fn file_token(&self, path: &str) -> Result<LibraryFileToken, String> {
        let file = paths::absolute(path)?;
        let (root, generation) = {
            let index = lock(&self.index);
            let root = index
                .roots
                .iter()
                .find(|r| under(&file, Path::new(&r.path)))
                .ok_or(STALE_FILE)?;
            (root.path.clone(), index.generation)
        };
        let fingerprint = file_fingerprint(&file, Path::new(&root))?;
        let token = LibraryFileToken {
            path: paths::display(&file),
            root_path: root,
            generation,
            fingerprint,
        };
        drop(self.guard(&token)?);
        Ok(token)
    }

    /// All filesystem checks happen before taking the commit guard or document lock.
    pub fn check_file(&self, token: &LibraryFileToken, path: &str) -> Result<(), String> {
        if !paths::same(&paths::absolute(path)?, &paths::absolute(&token.path)?)
            || token.fingerprint.len() > 512
        {
            return Err(STALE_FILE.into());
        }
        drop(self.guard(token)?);
        let fingerprint = file_fingerprint(Path::new(&token.path), Path::new(&token.root_path))?;
        // Manual tree drags carry root/generation before asynchronous file facts arrive.
        // Indexed results always carry the observed fingerprint; both paths use the loader.
        if !token.fingerprint.is_empty() && fingerprint != token.fingerprint {
            return Err(STALE_FILE.into());
        }
        drop(self.guard(token)?);
        Ok(())
    }

    /// Pins a manually browsed file version at operation start, retaining the drag's root epoch.
    pub fn pin_file(
        &self,
        token: &LibraryFileToken,
        path: &str,
    ) -> Result<LibraryFileToken, String> {
        self.check_file(token, path)?;
        let mut pinned = token.clone();
        if pinned.fingerprint.is_empty() {
            pinned.fingerprint =
                file_fingerprint(Path::new(&pinned.path), Path::new(&pinned.root_path))?;
        }
        self.check_file(&pinned, path)?;
        Ok(pinned)
    }

    /// Held only for an in-memory commit, after recording exclusion and before State.
    /// Root removal/refresh cannot cross a checked import's final document mutation.
    pub fn guard(&self, token: &LibraryFileToken) -> Result<LibraryGuard<'_>, String> {
        let index = lock(&self.index);
        if token.generation != index.generation
            || !index
                .roots
                .iter()
                .any(|r| paths::same(Path::new(&r.path), Path::new(&token.root_path)))
            || !under(Path::new(&token.path), Path::new(&token.root_path))
        {
            return Err(STALE_FILE.into());
        }
        Ok(LibraryGuard { _index: index })
    }

    pub fn metadata(&self, path: &str) -> Result<LibraryMetadata, String> {
        let token = self.file_token(path)?;
        let store = lock(&self.metadata);
        if let Some(e) = &store.error {
            return Err(e.clone());
        }
        Ok(store
            .data
            .files
            .get(&path_key(&token.path))
            .cloned()
            .unwrap_or_default())
    }

    pub fn set_metadata(
        &self,
        path: &str,
        metadata: LibraryMetadata,
    ) -> Result<LibraryMetadata, String> {
        let token = self.file_token(path)?;
        lock(&self.metadata).set(&token.path, metadata)
    }
}

/// The file an agent named, if it is a regular file inside one of `roots`
/// (the browser's folders and the factory content). A path with `.` or
/// `..` in it is refused outright, and so is a symbolic link or junction
/// anywhere between the folder and the file; the file's canonical path
/// must also lie inside the folder's, so nothing reaches past a root.
/// Returns the file's cleaned absolute path.
pub(crate) fn confined_file(text: &str, roots: &[PathBuf]) -> Result<PathBuf, String> {
    let outside = || {
        format!(
            "{text} is not inside a folder in Windfall's browser or its factory content; search_library lists the files an agent may use"
        )
    };
    let given = Path::new(text);
    if text.trim().is_empty() || !given.is_absolute() {
        return Err(format!(
            "{text} is not a full path; give the absolute path search_library gives"
        ));
    }
    if text.len() > MAX_PATH_BYTES {
        return Err(outside());
    }
    if given
        .components()
        .any(|part| matches!(part, std::path::Component::ParentDir | std::path::Component::CurDir))
        || text.split(['/', '\\']).any(|part| part == ".." || part == ".")
    {
        return Err(format!("{text} has . or .. in it; paths that climb out of a folder are refused"));
    }
    let file = paths::clean(given);
    let root = roots
        .iter()
        .map(|root| paths::clean(root))
        .find(|root| under(&file, root))
        .ok_or_else(outside)?;
    let mut depth = 0;
    for part in file.ancestors() {
        if paths::same(part, &root) {
            break;
        }
        depth += 1;
        if depth > MAX_DEPTH + 2 {
            return Err(outside());
        }
        let metadata = fs::symlink_metadata(part)
            .map_err(|error| format!("{}: {error}", paths::display(part)))?;
        if reparse(&metadata) {
            return Err(format!(
                "{text} goes through a symbolic link or junction ({}); agents may only use the original files",
                paths::display(part)
            ));
        }
    }
    if !fs::symlink_metadata(&file).is_ok_and(|metadata| metadata.is_file()) {
        return Err(format!("{text} is not a file"));
    }
    let real_root = dunce::canonicalize(&root).map_err(|error| error.to_string())?;
    let real_file = dunce::canonicalize(&file).map_err(|error| error.to_string())?;
    if !real_file.starts_with(&real_root) {
        return Err(outside());
    }
    Ok(file)
}

pub struct LibraryGuard<'a> {
    _index: MutexGuard<'a, Index>,
}

fn under(file: &Path, root: &Path) -> bool {
    file.ancestors().skip(1).any(|p| paths::same(p, root))
}

fn reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn hidden(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x6 != 0
    }
    #[cfg(not(windows))]
    {
        let _ = metadata;
        false
    }
}

/// The OS file identity, without retaining thousands of open handles in the index/cache.
pub(crate) fn file_identity(path: &Path) -> std::io::Result<u64> {
    let handle = same_file::Handle::from_path(path)?;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    handle.hash(&mut hash);
    Ok(hash.finish())
}

fn stamp(path: &Path, metadata: &fs::Metadata) -> std::io::Result<String> {
    Ok(format!(
        "{}:{}:{:?}:{:?}",
        file_identity(path)?,
        metadata.len(),
        metadata.modified().ok(),
        metadata.created().ok()
    ))
}

fn file_fingerprint(file: &Path, root: &Path) -> Result<String, String> {
    if paths::display(file).len() > MAX_PATH_BYTES || !under(file, root) {
        return Err(STALE_FILE.into());
    }
    // Recheck each ancestor too: a folder can have become a junction since indexing.
    let mut depth = 0;
    for part in file.ancestors() {
        depth += 1;
        if depth > MAX_DEPTH + 2 {
            return Err(STALE_FILE.into());
        }
        let metadata = fs::symlink_metadata(part).map_err(|e| {
            windfall_ipc::ui_message(
                "library.pathUnreadable",
                serde_json::json!({"path": paths::display(part), "error": e.to_string()}),
            )
        })?;
        if reparse(&metadata) {
            return Err("Library search skips symbolic links and junctions. Add the original folder instead.".into());
        }
        if paths::same(part, root) {
            break;
        }
    }
    let metadata = fs::symlink_metadata(file).map_err(|e| e.to_string())?;
    if !metadata.is_file() {
        return Err(STALE_FILE.into());
    }
    stamp(file, &metadata).map_err(|e| {
        windfall_ipc::ui_message(
            "library.fileUnreadable",
            serde_json::json!({"path": paths::display(file), "error": e.to_string()}),
        )
    })
}

fn worker(index: Arc<Mutex<Index>>, control: Arc<Control>) {
    loop {
        let request = {
            let mut work = lock(&control.work);
            while work.pending.is_none() && !work.shutdown {
                work = control
                    .wake
                    .wait(work)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
            if work.shutdown {
                return;
            }
            work.pending.take().unwrap()
        };
        scan(
            &index,
            &control,
            request.0,
            &request.1,
            MAX_ENTRIES,
            MAX_EXAMINED,
        );
    }
}

fn scan(
    index: &Mutex<Index>,
    control: &Control,
    generation: u32,
    roots: &[BrowserRoot],
    max_entries: usize,
    max_examined: usize,
) {
    let cancelled = || control.current.load(Ordering::Acquire) != generation;
    let started = Instant::now();
    let mut batch = Vec::new();
    let (mut examined, mut count, mut bytes) = (0, 0, 0);
    let mut seen = HashSet::new();
    let mut issues = Vec::new();
    let mut truncated = roots.len() > MAX_ROOTS;
    let mut issue = |path: &Path, error: &dyn std::fmt::Display| {
        if issues.len() < MAX_ISSUES {
            issues.push(windfall_ipc::ui_message(
                "library.indexFailed",
                serde_json::json!({"path": paths::display(path), "error": error.to_string()}),
            ));
        }
    };
    'roots: for root in roots.iter().take(MAX_ROOTS) {
        let root_path = Path::new(&root.path);
        if !root_path.is_absolute() {
            issue(
                root_path,
                &"the path is not absolute; remove this folder and add it again",
            );
            continue;
        }
        let mut stack = vec![(root_path.to_path_buf(), 0)];
        while let Some((folder, depth)) = stack.pop() {
            if cancelled() {
                return;
            }
            if started.elapsed() > SCAN_TIME {
                truncated = true;
                break 'roots;
            }
            let metadata = match fs::symlink_metadata(&folder) {
                Ok(m) => m,
                Err(e) => {
                    issue(&folder, &e);
                    continue;
                }
            };
            if reparse(&metadata) {
                issue(
                    &folder,
                    &"symbolic links and junctions are skipped; add the original folder",
                );
                continue;
            }
            if !metadata.is_dir() {
                issue(&folder, &"the path is not a folder");
                continue;
            }
            let entries = match fs::read_dir(&folder) {
                Ok(entries) => entries,
                Err(e) => {
                    issue(&folder, &e);
                    continue;
                }
            };
            for entry in entries {
                if cancelled() {
                    return;
                }
                if examined >= max_examined || count >= max_entries || started.elapsed() > SCAN_TIME
                {
                    truncated = true;
                    break 'roots;
                }
                examined += 1;
                // Publish on examined work, including hidden/broken/link entries.
                // A skipped entry at a batch boundary must not delay all results.
                if examined.is_multiple_of(BATCH) {
                    let mut index = lock(index);
                    if cancelled() || index.generation != generation {
                        return;
                    }
                    index.entries.append(&mut batch);
                    index.examined = examined;
                }
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(e) => {
                        issue(&folder, &e);
                        continue;
                    }
                };
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                let path = entry.path();
                if path.to_str().is_none() {
                    truncated = true;
                    issue(
                        &folder,
                        &"a filename is not valid Unicode; rename it to index it",
                    );
                    continue;
                }
                let shown = paths::display(&path);
                if shown.len() > MAX_PATH_BYTES || bytes + shown.len() > MAX_TOTAL_PATH_BYTES {
                    truncated = true;
                    continue;
                }
                let metadata = match fs::symlink_metadata(&path) {
                    Ok(m) => m,
                    Err(e) => {
                        issue(&path, &e);
                        continue;
                    }
                };
                if reparse(&metadata) || hidden(&metadata) {
                    continue;
                }
                if !seen.insert(path_key(&shown)) {
                    continue;
                }
                bytes += shown.len();
                if metadata.is_dir() {
                    if depth < MAX_DEPTH {
                        stack.push((path, depth + 1));
                    } else {
                        truncated = true;
                    }
                } else if metadata.is_file() {
                    let fingerprint = match stamp(&path, &metadata) {
                        Ok(stamp) => stamp,
                        Err(e) => {
                            issue(&path, &e);
                            continue;
                        }
                    };
                    count += 1;
                    batch.push(Arc::new(Indexed {
                        entry: BrowserEntry {
                            name,
                            path: shown,
                            kind: browser::file_kind(&path),
                        },
                        relative: paths::display(path.strip_prefix(root_path).unwrap_or(&path))
                            .replace('\\', "/"),
                        root: root.path.clone(),
                        fingerprint,
                    }));
                }
            }
        }
    }
    let mut index = lock(index);
    if cancelled() || index.generation != generation {
        return;
    }
    index.entries.append(&mut batch);
    index.examined = examined;
    index.truncated |= truncated;
    index.issues = issues;
    index.status = LibraryStatus::Ready;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait(library: &Library) -> LibraryResults {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let result = library.search(&LibrarySearch::default()).unwrap();
            if result.status != LibraryStatus::Indexing {
                return result;
            }
            assert!(Instant::now() < deadline, "index worker did not finish");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn recursive_search_and_metadata_survive_restart_without_touching_settings() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("Samples");
        fs::create_dir_all(root.join("Drums/Kicks")).unwrap();
        fs::create_dir_all(root.join("Vocals")).unwrap();
        let kick = root.join("Drums/Kicks/KICK Punch.WAV");
        fs::write(&kick, b"not decoded by index").unwrap();
        fs::write(root.join("Vocals/Vocal Chop.flac"), b"").unwrap();
        fs::write(root.join(".hidden.wav"), b"").unwrap();
        let settings = temp.path().join("settings.json");
        let original = br#"{"audio":{"bufferFrames":256},"midi":{"selected":"retain"}}"#;
        fs::write(&settings, original).unwrap();
        let file = temp.path().join(METADATA_FILE);
        let roots = vec![browser::user_root(&root)];
        let library = Library::new(file.clone(), roots.clone());
        assert_eq!(wait(&library).indexed, 2);
        let search = LibrarySearch {
            query: "(drums\\k?cks AND *.wav) OR \"vocal chop\"".into(),
            ..Default::default()
        };
        assert_eq!(library.search(&search).unwrap().entries.len(), 2);
        let shown = paths::display(&paths::clean(&kick));
        let meta = library
            .set_metadata(
                &shown,
                LibraryMetadata {
                    favorite: true,
                    tags: vec![" WARM ".into(), "drum".into(), "Warm".into()],
                },
            )
            .unwrap();
        assert_eq!(meta.tags, ["drum", "warm"]);
        let only = LibrarySearch {
            favorites_only: true,
            tags: vec!["WARM".into()],
            ..Default::default()
        };
        assert_eq!(library.search(&only).unwrap().entries[0].entry.path, shown);
        assert_eq!(fs::read(&settings).unwrap(), original);
        drop(library);
        let reopened = Library::new(file, roots);
        wait(&reopened);
        assert_eq!(reopened.metadata(&shown).unwrap(), meta);
        assert_eq!(reopened.search(&only).unwrap().entries.len(), 1);
    }

    #[test]
    fn removed_roots_and_changed_deleted_or_replaced_files_refuse_old_tokens() {
        let temp = tempfile::tempdir().unwrap();
        let samples = temp.path().join("Samples");
        fs::create_dir(&samples).unwrap();
        let file = samples.join("kick.wav");
        fs::write(&file, b"first").unwrap();
        let roots = vec![browser::user_root(&samples)];
        let library = Library::new(temp.path().join(METADATA_FILE), roots.clone());
        let result = wait(&library);
        let token = result.entries[0].token.clone();
        library.check_file(&token, &token.path).unwrap();
        fs::write(&file, b"changed and longer").unwrap();
        assert!(library.check_file(&token, &token.path).is_err());
        fs::remove_file(&file).unwrap();
        assert!(library.check_file(&token, &token.path).is_err());
        fs::create_dir(&file).unwrap();
        assert!(library.check_file(&token, &token.path).is_err());
        library.set_roots(Vec::new());
        assert!(
            library
                .search(&LibrarySearch::default())
                .unwrap()
                .entries
                .is_empty()
        );
        assert!(library.guard(&token).is_err());
        library.refresh();
        assert!(wait(&library).entries.is_empty());
        library.set_roots(roots);
        assert!(library.guard(&token).is_err());
    }

    #[test]
    fn corrupt_or_unwritable_metadata_is_preserved_and_reports_the_repair() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join(METADATA_FILE);
        fs::write(&file, b"broken").unwrap();
        let mut store = MetadataStore::load(file.clone());
        let error = store
            .set(
                "/samples/kick.wav",
                LibraryMetadata {
                    favorite: true,
                    tags: vec![],
                },
            )
            .unwrap_err();
        assert!(windfall_core::english::english(&error).contains("preserved"));
        assert_eq!(fs::read(&file).unwrap(), b"broken");
        let blocked = temp.path().join("blocked");
        fs::write(&blocked, b"file").unwrap();
        let mut store = MetadataStore {
            file: blocked.join(METADATA_FILE),
            data: Metadata::default(),
            error: None,
        };
        assert!(
            store
                .set(
                    "/samples/kick.wav",
                    LibraryMetadata {
                        favorite: true,
                        tags: vec![]
                    }
                )
                .is_err()
        );
        assert!(store.data.files.is_empty());
    }

    #[test]
    fn scan_limits_and_supersession_bound_work_and_do_not_publish_stale_batches() {
        let temp = tempfile::tempdir().unwrap();
        for i in 0..12 {
            fs::write(temp.path().join(format!("{i}.wav")), b"").unwrap();
        }
        let roots = vec![browser::user_root(temp.path())];
        let index = Mutex::new(Index {
            roots: roots.clone(),
            generation: 7,
            status: LibraryStatus::Indexing,
            entries: vec![],
            examined: 0,
            truncated: false,
            root_limit: false,
            issues: vec![],
        });
        let control = Control {
            work: Mutex::new(Work {
                pending: None,
                shutdown: false,
            }),
            wake: Condvar::new(),
            current: AtomicU32::new(7),
        };
        scan(&index, &control, 7, &roots, 3, 12);
        assert_eq!(lock(&index).entries.len(), 3);
        assert!(lock(&index).truncated);
        lock(&index).entries.clear();
        lock(&index).status = LibraryStatus::Indexing;
        control.current.store(8, Ordering::Release);
        scan(&index, &control, 7, &roots, 12, 12);
        assert!(lock(&index).entries.is_empty());
        control.current.store(7, Ordering::Release);
        scan(&index, &control, 7, &roots, 12, 2);
        assert_eq!(lock(&index).examined, 2);
        assert!(lock(&index).truncated);
    }

    #[test]
    fn cancellation_is_generation_scoped_and_refresh_resumes() {
        let temp = tempfile::tempdir().unwrap();
        let library = Library::new(temp.path().join(METADATA_FILE), vec![]);
        wait(&library);
        // A deterministic pending generation; the actual worker remains idle.
        let generation = {
            let mut i = lock(&library.index);
            i.generation += 1;
            i.status = LibraryStatus::Indexing;
            i.generation
        };
        library.cancel(generation - 1);
        assert_eq!(lock(&library.index).status, LibraryStatus::Indexing);
        library.cancel(generation);
        assert_eq!(
            library.search(&LibrarySearch::default()).unwrap().status,
            LibraryStatus::Cancelled
        );
        library.refresh();
        assert_eq!(wait(&library).status, LibraryStatus::Ready);
    }

    #[test]
    fn inaccessible_roots_and_bounded_results_are_visible() {
        let temp = tempfile::tempdir().unwrap();
        let samples = temp.path().join("Samples");
        fs::create_dir(&samples).unwrap();
        for i in 0..501 {
            fs::write(samples.join(format!("Kick {i}.wav")), b"").unwrap();
        }
        let missing = temp.path().join("Disconnected");
        let file = temp.path().join("Not a folder");
        fs::write(&file, b"").unwrap();
        let library = Library::new(
            temp.path().join(METADATA_FILE),
            vec![
                browser::user_root(&missing),
                browser::user_root(&file),
                browser::user_root(&samples),
            ],
        );
        let result = wait(&library);
        assert_eq!(result.indexed, 501);
        assert_eq!(result.entries.len(), MAX_RESULTS);
        assert!(result.results_truncated);
        assert_eq!(result.issues.len(), 2);
        assert!(windfall_core::english::english(&result.issues[0]).contains("Check folder access"));
    }

    fn link_folder(link: &Path, target: &Path) {
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).unwrap();
        #[cfg(windows)]
        assert!(
            std::process::Command::new("cmd")
                .args(["/D", "/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()
                .unwrap()
                .status
                .success()
        );
    }

    #[test]
    fn agents_may_name_only_ordinary_files_inside_the_roots() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("Sounds");
        let secret = temp.path().join("Secret");
        fs::create_dir_all(root.join("Banks")).unwrap();
        fs::create_dir(&secret).unwrap();
        let bank = root.join("Banks").join("GM.sf2");
        fs::write(&bank, b"RIFF").unwrap();
        fs::write(secret.join("private.sf2"), b"RIFF").unwrap();
        let roots = [root.clone()];
        let named = |path: &Path| confined_file(&paths::display(path), &roots);
        assert_eq!(named(&bank).unwrap(), paths::clean(&bank));
        // Outside every root.
        assert!(named(&secret.join("private.sf2")).unwrap_err().contains("not inside"));
        // Climbing out with .. is refused before anything is looked at, even
        // when it lands back inside.
        for climb in [
            root.join("..").join("Secret").join("private.sf2"),
            root.join("Banks").join("..").join("Banks").join("GM.sf2"),
            root.join(".").join("Banks").join("GM.sf2"),
        ] {
            assert!(named(&climb).unwrap_err().contains(".."), "{climb:?}");
        }
        // A link or junction inside the root that leads out of it.
        let escape = root.join("Escape");
        link_folder(&escape, &secret);
        assert!(
            named(&escape.join("private.sf2"))
                .unwrap_err()
                .contains("symbolic link or junction")
        );
        // A folder is not a file, and a relative path is not a full one.
        assert!(named(&root.join("Banks")).unwrap_err().contains("not a file"));
        assert!(confined_file("Banks/GM.sf2", &roots).unwrap_err().contains("full path"));
        assert!(named(&root.join("Banks").join("Missing.sf2")).is_err());
        // A root that is itself reached through a link still confines: the
        // file must be inside the folder the link leads to.
        let linked_root = temp.path().join("Linked");
        link_folder(&linked_root, &root);
        let through = [linked_root.clone()];
        assert!(confined_file(&paths::display(&linked_root.join("Banks").join("GM.sf2")), &through).is_ok());
        assert!(confined_file(&paths::display(&secret.join("private.sf2")), &through).is_err());
    }

    #[test]
    fn symlink_or_junction_cycles_are_skipped_and_cannot_be_used_after_indexing() {
        let temp = tempfile::tempdir().unwrap();
        let samples = temp.path().join("Samples");
        fs::create_dir(&samples).unwrap();
        fs::write(samples.join("kick.wav"), b"original").unwrap();
        let cycle = samples.join("Cycle");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&samples, &cycle).unwrap();
        #[cfg(windows)]
        assert!(
            std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&cycle)
                .arg(&samples)
                .output()
                .unwrap()
                .status
                .success()
        );
        let library = Library::new(
            temp.path().join(METADATA_FILE),
            vec![browser::user_root(&samples)],
        );
        let result = wait(&library);
        assert_eq!(result.indexed, 1);
        assert!(
            library
                .file_token(&paths::display(&cycle.join("kick.wav")))
                .is_err()
        );
    }
}
