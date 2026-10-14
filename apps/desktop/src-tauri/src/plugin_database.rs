//! The saved plugin database: what scans have found and what the user has
//! said about it, in `plugin-database.json` beside the settings file.
//!
//! The scanner's own catalog (`plugins.json`) is the plugin manager's and
//! says what can load. This file is the user's: favorites, categories, and
//! the memory of plugins that are not there right now, so a plugin on a
//! drive that is unplugged keeps its place. [`windfall_ipc::PluginDatabase`]
//! has the rules; this module only reads and writes the file.
//!
//! A file that cannot be read, because it is damaged or was written by a
//! newer build, is left exactly as it is. The database then works from
//! memory, shows the reason, and refuses to save over the file until it has
//! been repaired or moved away.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use windfall_ipc::{PluginAnnotation, PluginDatabase, PluginDatabaseView, PluginManagerState};
use windfall_project::file::write_atomic;

use crate::sync::lock;

/// The name of the database file.
pub const PLUGIN_DATABASE_FILE: &str = "plugin-database.json";
/// The largest database file that is read.
const MAX_DATABASE_BYTES: u64 = 8 * 1024 * 1024;

struct Loaded {
    database: PluginDatabase,
    /// Why the file on disk must not be written over.
    locked: Option<String>,
}

/// The database and its file. Clones of the `Arc` it lives in share it.
pub struct PluginDatabaseStore {
    file: PathBuf,
    loaded: Mutex<Loaded>,
}

impl PluginDatabaseStore {
    /// Reads the database in `folder`, which need not have one yet.
    pub fn load(folder: &Path) -> Self {
        let file = folder.join(PLUGIN_DATABASE_FILE);
        let loaded = match read(&file) {
            Ok(Some(database)) => Loaded {
                database,
                locked: None,
            },
            Ok(None) => Loaded {
                database: PluginDatabase::default(),
                locked: None,
            },
            Err(error) => {
                log::warn!("plugin database: {error}");
                Loaded {
                    database: PluginDatabase::default(),
                    locked: Some(windfall_ipc::ui_message(
                        "plugins.databaseLocked",
                        serde_json::json!({ "error": error.to_string(), "file": file.display().to_string() }),
                    )),
                }
            }
        };
        Self {
            file,
            loaded: Mutex::new(loaded),
        }
    }

    /// The database as it is shown, for what the plugin manager says the
    /// last scan found. A finished scan is taken into the database, and
    /// saved, on the way.
    pub fn view(&self, scan: &PluginManagerState) -> PluginDatabaseView {
        let mut loaded = lock(&self.loaded);
        let mut error = loaded.locked.clone();
        // A scan in progress lists only part of what it will find.
        if !scan.scanning && loaded.database.absorb(scan) && loaded.locked.is_none() {
            error = self.save(&loaded.database).err();
        }
        loaded.database.view(scan, error)
    }

    /// Sets what the user says about one plugin and saves it. When the
    /// file cannot be written the change is taken back and the error says
    /// why.
    pub fn annotate(
        &self,
        format: &str,
        id: &str,
        annotation: PluginAnnotation,
        scan: &PluginManagerState,
    ) -> Result<PluginDatabaseView, String> {
        let mut loaded = lock(&self.loaded);
        if let Some(reason) = &loaded.locked {
            return Err(reason.clone());
        }
        let before = loaded.database.clone();
        loaded.database.annotate(format, id, annotation)?;
        if let Err(error) = self.save(&loaded.database) {
            loaded.database = before;
            return Err(error);
        }
        Ok(loaded.database.view(scan, None))
    }

    /// Forgets the plugins that the last scan did not find and that have
    /// neither a favorite mark nor a category of the user's.
    pub fn forget_missing(&self, scan: &PluginManagerState) -> Result<PluginDatabaseView, String> {
        if scan.scanning {
            return Err("Wait for the plugin scan to finish first.".into());
        }
        let mut loaded = lock(&self.loaded);
        if let Some(reason) = &loaded.locked {
            return Err(reason.clone());
        }
        let before = loaded.database.clone();
        if loaded.database.forget_missing(scan) > 0
            && let Err(error) = self.save(&loaded.database)
        {
            loaded.database = before;
            return Err(error);
        }
        Ok(loaded.database.view(scan, None))
    }

    fn save(&self, database: &PluginDatabase) -> Result<(), String> {
        let bytes = database.to_json()?;
        write_atomic(&self.file, &bytes).map_err(|error| {
            windfall_ipc::ui_message(
                "plugins.databaseSaveFailed",
                serde_json::json!({ "error": error.to_string() }),
            )
        })
    }
}

/// The database in `file`, or `None` when there is no file yet.
fn read(file: &Path) -> Result<Option<PluginDatabase>, String> {
    let opened = match std::fs::File::open(file) {
        Ok(opened) => opened,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(windfall_ipc::ui_message(
                "plugins.databaseOpenFailed",
                serde_json::json!({ "error": error.to_string() }),
            ));
        }
    };
    let mut bytes = Vec::new();
    opened
        .take(MAX_DATABASE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            windfall_ipc::ui_message(
                "plugins.databaseReadFailed",
                serde_json::json!({ "error": error.to_string() }),
            )
        })?;
    if bytes.len() as u64 > MAX_DATABASE_BYTES {
        return Err("The plugin database is larger than 8 MiB".to_owned());
    }
    PluginDatabase::from_json(&bytes).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use windfall_ipc::{PluginEntry, PluginStatus};

    fn scan(names: &[&str]) -> PluginManagerState {
        PluginManagerState {
            entries: names
                .iter()
                .map(|name| PluginEntry {
                    path: format!("C:/plugins/{name}.clap"),
                    format: "clap".into(),
                    id: format!("org.example.{name}"),
                    name: (*name).to_owned(),
                    vendor: "Example".into(),
                    instrument: true,
                    usable: true,
                    error: None,
                })
                .collect(),
            ..PluginManagerState::default()
        }
    }

    fn status(view: &PluginDatabaseView, id: &str) -> Option<(PluginStatus, bool, String)> {
        view.records
            .iter()
            .find(|record| record.id == id)
            .map(|record| (record.status, record.favorite, record.category.clone()))
    }

    #[test]
    fn scans_favorites_and_categories_are_saved_and_survive_a_restart() {
        let folder = tempfile::tempdir().unwrap();
        let store = PluginDatabaseStore::load(folder.path());
        let file = folder.path().join(PLUGIN_DATABASE_FILE);
        // Nothing found, nothing said: no file is made.
        let view = store.view(&PluginManagerState::default());
        assert_eq!(view.error, None);
        assert!(
            view.records
                .iter()
                .all(|r| r.status == PluginStatus::Builtin)
        );
        assert!(!file.exists());

        // A scan in progress is shown but not remembered yet.
        let mut running = scan(&["glass"]);
        running.scanning = true;
        assert_eq!(
            status(&store.view(&running), "org.example.glass").map(|s| s.0),
            Some(PluginStatus::Verified)
        );
        assert!(!file.exists());

        let found = scan(&["glass", "brass"]);
        assert_eq!(store.view(&found).error, None);
        assert!(file.is_file());
        let view = store
            .annotate(
                "clap",
                "org.example.glass",
                PluginAnnotation {
                    favorite: true,
                    category: Some("Leads".into()),
                },
                &found,
            )
            .unwrap();
        assert_eq!(
            status(&view, "org.example.glass"),
            Some((PluginStatus::Verified, true, "Leads".to_owned()))
        );

        // The next run of the app, with the drive that held both unplugged.
        let store = PluginDatabaseStore::load(folder.path());
        let empty = PluginManagerState::default();
        let view = store.view(&empty);
        assert_eq!(view.error, None);
        assert_eq!(
            status(&view, "org.example.glass"),
            Some((PluginStatus::Missing, true, "Leads".to_owned()))
        );
        assert_eq!(
            status(&view, "org.example.brass").map(|s| s.0),
            Some(PluginStatus::Missing)
        );

        // Forgetting drops the one nothing was said about, in the file too.
        assert!(store.forget_missing(&running).is_err());
        let view = store.forget_missing(&empty).unwrap();
        assert!(status(&view, "org.example.brass").is_none());
        assert!(status(&view, "org.example.glass").is_some());
        let view = PluginDatabaseStore::load(folder.path()).view(&empty);
        assert!(status(&view, "org.example.brass").is_none());
        assert_eq!(status(&view, "org.example.glass").map(|s| s.1), Some(true));
    }

    #[test]
    fn a_file_that_cannot_be_read_is_left_alone_and_reported() {
        for contents in [
            b"not json at all".to_vec(),
            br#"{"version":99,"known":{}}"#.to_vec(),
        ] {
            let folder = tempfile::tempdir().unwrap();
            let file = folder.path().join(PLUGIN_DATABASE_FILE);
            std::fs::write(&file, &contents).unwrap();
            let store = PluginDatabaseStore::load(folder.path());
            let found = scan(&["glass"]);
            let view = store.view(&found);
            // The scan is still shown, with the reason nothing is saved.
            assert!(
                view.error
                    .as_deref()
                    .is_some_and(|e| e.contains("plugins.databaseLocked"))
            );
            assert_eq!(
                status(&view, "org.example.glass").map(|s| s.0),
                Some(PluginStatus::Verified)
            );
            let refused = store.annotate(
                "clap",
                "org.example.glass",
                PluginAnnotation {
                    favorite: true,
                    category: None,
                },
                &found,
            );
            assert!(refused.is_err());
            assert!(store.forget_missing(&found).is_err());
            assert_eq!(std::fs::read(&file).unwrap(), contents);
        }
    }

    #[test]
    fn a_change_that_cannot_be_saved_is_taken_back() {
        let folder = tempfile::tempdir().unwrap();
        let store = PluginDatabaseStore::load(folder.path());
        let found = scan(&["glass"]);
        store.view(&found);
        // A folder where the file belongs makes every write fail.
        let file = folder.path().join(PLUGIN_DATABASE_FILE);
        std::fs::remove_file(&file).unwrap();
        std::fs::create_dir(&file).unwrap();
        let refused = store.annotate(
            "clap",
            "org.example.glass",
            PluginAnnotation {
                favorite: true,
                category: None,
            },
            &found,
        );
        assert!(refused.unwrap_err().contains("plugins.databaseSaveFailed"));
        std::fs::remove_dir(&file).unwrap();
        // The favorite did not stick in memory either.
        let view = store.view(&found);
        assert_eq!(status(&view, "org.example.glass").map(|s| s.1), Some(false));
    }
}
