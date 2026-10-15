Windfall 1.0.3 improves the Windows editing workflow following a hands-on song-making and UI review.

## Editing and navigation

- Notebook edits are added to the project after a short pause and when the panel closes. Its Save button and status stay visible. Save the project to keep the notes on disk.
- The audio editor follows the edited clip, so consecutive edits work without reopening it.
- Piano-roll tools wrap to fit the available width, the initial snap is Step, and resized note edges land on the grid. Patterns extended by editing can shrink back to their previous length.
- Audio clip and channel settings replace repeated preset/Previous/Next/Half/Double rows with compact menus. Channel settings gain section navigation and foldable sections.
- Undo hides an empty clip inspector, and double-clicking an audio clip reopens its settings. View > Playlist also exposes the settings and a saved option to turn off right-click deletion; right-click deletion remains the default.

## Clearer browsing and feedback

- Add instrument and effect menus are grouped by purpose with descriptions. The plugin manager uses one search and clearly labels installed plugin files and the plugin library.
- Mixer destinations consistently show their visible track number and name. Adaptive strips use available width, and folded track settings leave effect controls easier to reach.
- Library selection stays visible with tags folded, Factory browsing hides unsupported metadata files, and factory loops show their source tempo with an Add at song tempo action.
- Browser tabs are named Presets & templates, Auto backups, and Folder backup.
- Project history and analysis errors show readable messages. A project without automatic backups gets an explanatory empty state.
- Settings opens at the top with section navigation. Recording status and Start/Stop controls and the tempo-fit Apply button stay visible while setup scrolls.
- Tempo-fit values are rounded for display. Notebook and recording positions use bar:beat(:tick), and the audio editor adds a time ruler and time equivalents for its frame range.
- Searching for audio editor finds the editor and clip settings. Song-mode status correctly says Song stopped when playback is stopped.
- Dropout diagnostics now show timing, engine load, buffer size, and a reset action. An audio buffer is no longer counted twice for the same late completion.

## Known limitations

- The audio editor can require horizontal scrolling at the default window size. Some narrow mixer labels and the channel section bar still truncate or scroll.
- Dropout counts can still increase during stopped editing; the diagnostics and double-count correction do not resolve every device timing issue.
- Add menus support type-to-jump but have no inline search box. Unsupported files in user-added browser folders remain visible and greyed.
- Model-based analysis requires a separately configured runtime and model. Windfall does not download them automatically.

Windows x64 release. The installer includes a verified updater signature; Windows Authenticode code signing is not provided. Existing 1.0.1 and 1.0.2 installations can check for this update from Help > Check for updates.
