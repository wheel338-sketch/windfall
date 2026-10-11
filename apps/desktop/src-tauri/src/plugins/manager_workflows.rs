//! Plugin manager workflows on top of the scanner: taking a folder out of
//! the scan paths, retrying everything that is blocked, and verifying every
//! plugin again from nothing.
//!
//! Nothing here loads a plugin or touches a hosted instance. Each workflow
//! changes the folder list or the scanner's catalog and then starts the
//! ordinary isolated scan, which is what decides what may load.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use windfall_plugin_host::{
    PluginFormat, paths,
    scan::PluginCatalog,
};

use super::PluginManager;

/// What a rescan starts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rescan {
    /// Every blocked file is given another try; what was verified stays.
    Blocked,
    /// The catalog and the blocklist are emptied first, so every file in
    /// the scan paths is verified again.
    Everything,
}

impl PluginManager {
    /// The folders every installation scans. They cannot be removed.
    fn standard_folders() -> Vec<PathBuf> {
        let mut folders = paths::standard_folders(PluginFormat::Clap);
        folders.extend(paths::standard_folders(PluginFormat::Vst3));
        folders
    }

    /// Whether `folder` is one the user added, as opposed to a standard
    /// one.
    pub fn is_custom_folder(folder: &str) -> bool {
        !Self::standard_folders()
            .iter()
            .any(|standard| standard == Path::new(folder))
    }

    /// Takes a folder the user added out of the scan paths. Its plugins
    /// leave the list with the next scan, which the caller starts.
    pub fn remove_folder(&self, folder: &str) -> Result<(), String> {
        if !Self::is_custom_folder(folder) {
            return Err("The standard plugin folders are always scanned".into());
        }
        let folders = {
            let mut state = self
                .status
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if state.scanning {
                return Err("A plugin scan is already running".into());
            }
            let before = state.folders.len();
            state.folders.retain(|configured| configured != folder);
            if state.folders.len() == before {
                return Err("That folder is not one of the plugin folders".into());
            }
            state.folders.clone()
        };
        std::fs::write(
            &self.folders_file,
            serde_json::to_vec(&folders).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())
    }

    /// Changes what the scanner remembers, then scans. Refused while a
    /// scan is running, which owns the catalog until it is done.
    pub fn rescan(self: &Arc<Self>, from: Rescan) -> Result<(), String> {
        {
            let scanning = self
                .status
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .scanning;
            // The scan thread holds the catalog for as long as it runs.
            let Ok(mut catalog) = self.catalog.try_lock() else {
                return Err("A plugin scan is already running".into());
            };
            if scanning {
                return Err("A plugin scan is already running".into());
            }
            match from {
                Rescan::Blocked => {
                    catalog.retry_blocked();
                }
                Rescan::Everything => *catalog = PluginCatalog::new(),
            }
        }
        self.scan(None)
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn a_folder_the_user_added_can_be_removed_and_a_standard_one_cannot() {
        let config = tempfile::tempdir().unwrap();
        let extra = tempfile::tempdir().unwrap();
        let folder = extra.path().to_string_lossy().into_owned();
        let manager = PluginManager::new(config.path()).unwrap();
        let standard = manager.state().folders;
        assert!(!standard.contains(&folder));
        assert!(PluginManager::is_custom_folder(&folder));

        manager.add_folder(folder.clone()).unwrap();
        assert!(manager.state().folders.contains(&folder));
        manager.remove_folder(&folder).unwrap();
        assert_eq!(manager.state().folders, standard);
        // The choice survives a restart.
        let again = PluginManager::new(config.path()).unwrap();
        assert_eq!(again.state().folders, standard);

        assert!(manager.remove_folder(&folder).unwrap_err().contains("not one of"));
        for folder in &standard {
            assert!(!PluginManager::is_custom_folder(folder));
            assert!(manager.remove_folder(folder).unwrap_err().contains("standard"));
        }
        assert_eq!(manager.state().folders, standard);
    }
}
