//! Exporting the playlist tracks apart: each lane of the timeline as a file
//! of its own. The encoders, staging and cancellation are the export's own;
//! this module chooses the lanes and names the files.

use windfall_engine::{PlaylistStem, PlaylistStemError, PlaylistStemOptions, playlist_stems};
use windfall_ipc::{ExportOptions, ExportPlaylistStems, PlayMode};
use windfall_project::Project;

use super::Session;

/// Refuses a request that asks for two kinds of stem at once, or for the
/// lanes of something that has none.
pub(super) fn check(
    options: &ExportOptions,
    playlist: Option<&ExportPlaylistStems>,
) -> Result<(), String> {
    if playlist.is_none() {
        return Ok(());
    }
    if options.stems.is_some() {
        return Err(
            "Export the mixer tracks or the playlist tracks apart, not both at once.".to_owned(),
        );
    }
    if options.mode != PlayMode::Song {
        return Err(PlaylistStemError::PatternMode.to_string());
    }
    Ok(())
}

pub(super) fn stem_options(asked: &ExportPlaylistStems) -> PlaylistStemOptions {
    PlaylistStemOptions {
        tracks: asked.tracks.clone(),
        include_mix: asked.include_mix,
        numbered: asked.numbered,
    }
}

/// The streams the export writes, the mix first, then the lanes in
/// playlist order.
pub(super) fn list(
    project: &Project,
    asked: &ExportPlaylistStems,
) -> Result<Vec<PlaylistStem>, String> {
    playlist_stems(project, &stem_options(asked)).map_err(|error| error.to_string())
}

impl Session {
    /// Starts exporting each playlist track to a file of its own and
    /// returns at once, with the progress, cancellation, staging and file
    /// naming of [`Session::export_audio`]: for the path `Song.wav` the
    /// files are `Song - Mix.wav`, `Song - Drums.wav` and so on, in the
    /// folder `Song` or beside it.
    ///
    /// A file holds the song with only that lane's pattern and audio clips
    /// placed, heard at the output; see [`ExportPlaylistStems`]. The export
    /// works from a copy of the project as it is now, follows the active
    /// arrangement, and needs song mode. `options.stems` must be absent.
    pub fn export_playlist_tracks(
        &self,
        options: ExportOptions,
        stems: ExportPlaylistStems,
    ) -> Result<(), String> {
        self.start_export(options, Some(stems))
    }
}
