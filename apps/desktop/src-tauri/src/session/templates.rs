//! Project templates: starting a project from a stored one, and storing the
//! open project as one.
//!
//! A **factory template** is built by code from Windfall's own factory
//! sounds each time it is asked for. There is no file to edit, replace or
//! delete, so it is always the same.
//!
//! A **user template** is the open project as a portable archive
//! ([`windfall_archive`]) in the `templates` folder beside the settings
//! file, so it carries its audio and the state of its plugins and does not
//! depend on the project it was made from. Saving one never touches the
//! open project: not its file, its paths, its history or its unsaved mark.
//! A template is never overwritten; to change one, delete it and save it
//! again.
//!
//! A project started from either kind is a new project. It is called
//! "Untitled", has no file, so that saving asks where to, and has an empty
//! undo history that is not marked as changed. Starting one goes through
//! the same guarded replacement as New and Open, so it is refused while
//! recording and when the open project was edited or replaced meanwhile.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use windfall_ipc::TemplateInfo;
use windfall_project::{ChannelId, Command, Document, DocumentSnapshot, Project, SettingsPatch};

use super::Session;
use super::presets::safe_file_stem;
use super::samples::decode_all;
use crate::paths;
use crate::template::{DEFAULT_PROJECT_NAME, default_project};

/// The extension of a user template file.
pub const TEMPLATE_EXTENSION: &str = "wftemplate";
/// The folder of the user's templates, beside the settings file.
const TEMPLATE_FOLDER: &str = "templates";
/// The most user templates that are listed.
const MAX_USER_TEMPLATES: usize = 512;

const FACTORY_PREFIX: &str = "factory:";
const USER_PREFIX: &str = "user:";

/// A template that ships with Windfall.
struct Factory {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    build: fn() -> Project,
}

const FACTORY: [Factory; 4] = [
    Factory {
        id: "empty",
        name: "Empty",
        description: "One empty pattern and the master track. Nothing else.",
        build: empty,
    },
    Factory {
        id: "basic-kit",
        name: "Basic kit",
        description: "Kick, clap, hat and snare, each on its own mixer track. What New project starts with.",
        build: default_project,
    },
    Factory {
        id: "four-on-the-floor",
        name: "Four on the floor",
        description: "The basic kit with a house beat in the first pattern, at 124 bpm.",
        build: four_on_the_floor,
    },
    Factory {
        id: "boom-bap",
        name: "Boom bap",
        description: "The basic kit with a swung hip-hop beat in the first pattern, at 90 bpm.",
        build: boom_bap,
    },
];

fn empty() -> Project {
    Project::new(DEFAULT_PROJECT_NAME)
}

/// The basic kit with steps lit and settings changed, built with the same
/// commands the UI sends and returned without their history. `steps` holds
/// the lit steps of each kit channel, in rack order.
fn beat(tempo_bpm: f64, swing: f32, steps: [&[u32]; 4]) -> Project {
    let mut scratch = Document::new(default_project());
    let pattern = scratch.project().patterns[0].id;
    let channels: Vec<ChannelId> = scratch.project().channels.iter().map(|c| c.id).collect();
    let mut commands = vec![Command::UpdateSettings {
        patch: SettingsPatch {
            tempo_bpm: Some(tempo_bpm),
            swing: Some(swing),
            ..SettingsPatch::default()
        },
    }];
    for (channel, lit) in channels.into_iter().zip(steps) {
        for &step in lit {
            commands.push(Command::ToggleStep {
                pattern,
                channel,
                step,
            });
        }
    }
    for command in commands {
        // None of these can fail on the basic kit. If one ever does, a
        // template with a little less in it is still a template.
        if let Err(error) = scratch.dispatch(command, None) {
            log::error!("a factory template could not be built in full: {error}");
        }
    }
    scratch.project().clone()
}

fn four_on_the_floor() -> Project {
    beat(124.0, 0.0, [&[0, 4, 8, 12], &[4, 12], &[2, 6, 10, 14], &[]])
}

fn boom_bap() -> Project {
    beat(
        90.0,
        0.35,
        [&[0, 7, 10], &[], &[0, 2, 4, 6, 8, 10, 12, 14], &[4, 12]],
    )
}

impl Session {
    fn template_dir(&self) -> PathBuf {
        self.store()
            .recordings_dir()
            .parent()
            .unwrap_or(Path::new("."))
            .join(TEMPLATE_FOLDER)
    }

    /// Every template: the factory ones first, in their fixed order, then
    /// the user's by name.
    pub fn templates(&self) -> Result<Vec<TemplateInfo>, String> {
        let mut templates: Vec<TemplateInfo> = FACTORY
            .iter()
            .map(|factory| TemplateInfo {
                id: format!("{FACTORY_PREFIX}{}", factory.id),
                name: factory.name.to_owned(),
                description: factory.description.to_owned(),
                factory: true,
            })
            .collect();
        let listing = match fs::read_dir(self.template_dir()) {
            Ok(listing) => listing,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(templates),
            Err(error) => {
                return Err(windfall_ipc::ui_message(
                    "templates.folderUnreadable",
                    serde_json::json!({ "error": error.to_string() }),
                ));
            }
        };
        let mut user = Vec::new();
        for item in listing.flatten() {
            let path = item.path();
            let is_template = path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case(TEMPLATE_EXTENSION));
            if !is_template || !item.file_type().is_ok_and(|kind| kind.is_file()) {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            user.push(TemplateInfo {
                id: format!("{USER_PREFIX}{name}"),
                name: name.to_owned(),
                description: "Your template.".to_owned(),
                factory: false,
            });
            if user.len() >= MAX_USER_TEMPLATES {
                break;
            }
        }
        user.sort_by_key(|template| template.name.to_lowercase());
        templates.extend(user);
        Ok(templates)
    }

    /// Stores the open project as a user template called `name`, with its
    /// audio. The open project is left exactly as it is.
    ///
    /// Refused when the name cannot be a file name, when it is the name of
    /// a factory template, and when a template of that name exists already.
    /// Missing or unreadable audio refuses the save and lists the sources,
    /// as for a portable archive.
    pub fn template_save(&self, name: &str) -> Result<TemplateInfo, String> {
        let stem = safe_file_stem(name)?;
        if FACTORY
            .iter()
            .any(|factory| factory.name.eq_ignore_ascii_case(&stem))
        {
            return Err(windfall_ipc::ui_message(
                "templates.factoryName",
                serde_json::json!({ "name": stem }),
            ));
        }
        let folder = self.template_dir();
        fs::create_dir_all(&folder).map_err(|error| {
            windfall_ipc::ui_message(
                "templates.folderNotCreated",
                serde_json::json!({ "error": error.to_string() }),
            )
        })?;
        let target = folder.join(format!("{stem}.{TEMPLATE_EXTENSION}"));
        // Windows would find "kick" for "Kick"; so does this, everywhere.
        let taken = self.templates()?.into_iter().any(|template| {
            !template.factory && template.name.to_lowercase() == stem.to_lowercase()
        });
        if taken || fs::symlink_metadata(&target).is_ok() {
            return Err(windfall_ipc::ui_message(
                "templates.nameTaken",
                serde_json::json!({ "name": stem }),
            ));
        }
        self.archive_export_to(target, self.archive_job()?)
            .map_err(|error| {
                error.replace("no portable archive was created", "no template was saved")
            })?;
        Ok(TemplateInfo {
            id: format!("{USER_PREFIX}{stem}"),
            name: stem,
            description: "Your template.".to_owned(),
            factory: false,
        })
    }

    /// Replaces the open project with a new one made from the template
    /// `id`. Refused in the cases [`project_open`](Self::project_open) is.
    pub fn template_new(&self, id: &str) -> Result<DocumentSnapshot, String> {
        if let Some(factory) = id.strip_prefix(FACTORY_PREFIX) {
            let factory = FACTORY
                .iter()
                .find(|template| template.id == factory)
                .ok_or_else(|| {
                    windfall_ipc::ui_message(
                        "templates.noFactoryTemplate",
                        serde_json::json!({ "id": factory }),
                    )
                })?;
            drop(self.recording_idle()?);
            let ticket = self.begin_replacement();
            let mut project = (factory.build)();
            fresh(&mut project);
            let decoded = decode_all(&self.inner.cache, &project, None, &self.inner.factory_dir);
            return self
                .install(&ticket, Document::new(project), None, None, None, decoded)
                .map_err(|refusal| refusal.message("The new project was not started"));
        }
        let file = self.user_template(id)?;
        self.archive_open_adjusted(
            &paths::display(&file),
            self.archive_job()?,
            "The template",
            fresh,
        )
    }

    /// Deletes a user template. A factory template is part of Windfall and
    /// cannot be deleted.
    pub fn template_delete(&self, id: &str) -> Result<(), String> {
        if id.starts_with(FACTORY_PREFIX) {
            return Err("Templates that come with Windfall cannot be deleted.".into());
        }
        let file = self.user_template(id)?;
        fs::remove_file(&file).map_err(|error| {
            windfall_ipc::ui_message(
                "templates.deleteFailed",
                serde_json::json!({ "error": error.to_string() }),
            )
        })
    }

    /// The file of the user template `id`, which must exist in the
    /// templates folder itself.
    fn user_template(&self, id: &str) -> Result<PathBuf, String> {
        let name = id.strip_prefix(USER_PREFIX).ok_or_else(|| {
            windfall_ipc::ui_message(
                "templates.notATemplate",
                serde_json::json!({ "id": id.to_string() }),
            )
        })?;
        // The id is a name, never a path: anything that could leave the
        // folder is not the name of a template.
        if safe_file_stem(name).as_deref() != Ok(name) {
            return Err(windfall_ipc::ui_message(
                "templates.notATemplateName",
                serde_json::json!({ "name": name.to_string() }),
            ));
        }
        let file = self
            .template_dir()
            .join(format!("{name}.{TEMPLATE_EXTENSION}"));
        if !file.is_file() {
            return Err(windfall_ipc::ui_message(
                "templates.gone",
                serde_json::json!({ "name": name.to_string() }),
            ));
        }
        Ok(file)
    }
}

/// Makes a template's project a new project: it has no name of its own.
/// Its file and its history are not part of a project and are left behind
/// by whoever installs it.
fn fresh(project: &mut Project) {
    project.settings.name = DEFAULT_PROJECT_NAME.to_owned();
}

#[cfg(test)]
mod tests {
    use super::*;
    use windfall_factory::DEFAULT_KIT;

    #[test]
    fn every_factory_template_is_a_valid_project_with_no_history() {
        let mut ids = std::collections::HashSet::new();
        for factory in &FACTORY {
            assert!(ids.insert(factory.id), "{} is listed twice", factory.id);
            assert_eq!(safe_file_stem(factory.name).as_deref(), Ok(factory.name));
            let project = (factory.build)();
            assert_eq!(project.check(), Ok(()), "{}", factory.id);
            let mut document = Document::new(project.clone());
            assert!(!document.is_dirty());
            assert!(document.history().entries.is_empty());
            assert_eq!(document.undo(), None);
            // Built the same every time: there is nothing to have changed.
            assert_eq!((factory.build)(), project, "{}", factory.id);
        }
    }

    #[test]
    fn the_beats_light_the_steps_they_describe() {
        let lit = |project: &Project, channel: usize| -> Vec<u32> {
            let id = project.channels[channel].id;
            project.patterns[0]
                .lane(id)
                .map(|lane| {
                    lane.notes
                        .iter()
                        .map(|note| note.start / windfall_core::TICKS_PER_STEP)
                        .collect()
                })
                .unwrap_or_default()
        };
        let house = four_on_the_floor();
        assert_eq!(house.channels.len(), DEFAULT_KIT.len());
        assert_eq!(house.settings.tempo_bpm, 124.0);
        assert_eq!(lit(&house, 0), [0, 4, 8, 12]);
        assert_eq!(lit(&house, 1), [4, 12]);
        assert_eq!(lit(&house, 2), [2, 6, 10, 14]);
        assert_eq!(lit(&house, 3), Vec::<u32>::new());

        let swung = boom_bap();
        assert_eq!(swung.settings.tempo_bpm, 90.0);
        assert_eq!(swung.settings.swing, 0.35);
        assert_eq!(lit(&swung, 0), [0, 7, 10]);
        assert_eq!(lit(&swung, 3), [4, 12]);

        assert!(empty().channels.is_empty());
        assert_eq!(empty().patterns.len(), 1);
    }
}
