# Windfall 1.0.4

- Fixed VST3 editor compatibility: a plugin that leaves the optional host-frame
  callback unimplemented can now attach and display its native window.
- Plugin wrapper and parameter settings now recover when independent native
  state or parameter readback arrives during audio preparation. Conflicting
  edits and project/source replacements remain guarded.
- Added MCP tools `list_plugins`, `add_plugin`, `plugin_editor`, and
  `get_plugin_diagnostics`. An AI can propose loading a scanned plugin, attempt
  its native editor, and return a shareable JSON error report. Plugin additions
  use Windfall's existing proposal approval/trusted-agent controls.
- Support reports include the failed stage and exact error, plugin identity,
  wrapper configuration, scan status, audio settings, helper health, and native
  editor visibility, dimensions and screen bounds on Windows. Credentials,
  song notes and opaque plugin state are excluded.

For a plugin problem, ask your connected AI: “Find BBC Symphony Orchestra with
`list_plugins`, propose adding it, try its native editor, then give me the
complete `get_plugin_diagnostics` JSON report to send to the developer.” Review
and apply the add proposal in Windfall, unless you have already trusted the AI.
If it is already added, ask the AI to inspect that channel instead.

Focused Windows tests used the repository's native VST3/CLAP fixtures, including
real visible editor windows and separate helper processes. BBC Symphony
Orchestra itself was unavailable on the release machine; its specific behavior
must still be checked on the machine where the issue was observed.

Windows x64 release. Existing users can use Help → Check for updates or install
the new Windows setup executable. Project format remains unchanged.
