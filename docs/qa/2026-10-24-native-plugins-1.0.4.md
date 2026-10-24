# Windows 1.0.4 native plugin fix and MCP support reports

Published [Windfall 1.0.4](https://github.com/wheel338-sketch/windfall/releases/tag/v1.0.4)
as the latest stable Windows x64 release at `2026-10-24T06:45:22Z`.
The annotated tag resolves to `6b9b72171fcf9972e54ec81eb47ad6f711d36b6c`.

## Scope and reproduction

The user reported that BBC Symphony Orchestra was detected and exposed its
parameters, but its native editor did not become visible. Changing plugin
settings could also fail with “The project changed while sampler audio was
being prepared. Try again.” BBC was installed only on a friend's machine and
was unavailable locally. Its exact failure cause remains unconfirmed.

Two local reproductions failed before the fixes:

- A real, fixed-size VST3 fixture exposed its parameters and editor, but the
  host rejected `IPlugView::setFrame` returning `kNotImplemented` before it
  attempted attachment. The host now follows the SDK editor host's attachment
  behavior. Native view failures also identify the failing method, return code
  or invalid bounds in support errors.
- A native parameter update arriving while wrapper settings were prepared
  produced the exact reported sampler error. Independent native values now
  permit a bounded rebuild of plugin settings against the current document.
  Newer edits of the same control, unrelated document edits, replacements,
  sources, providers and preparation environments retain their guards.

MCP now exposes `list_plugins`, `add_plugin`, `plugin_editor` and
`get_plugin_diagnostics`. Plugin additions retain the existing proposal and
trusted-agent approval path. Reports inspect the selected playback owner,
including separate-process editor visibility, size and Windows screen bounds.
UI editor/settings errors and MCP load errors retain their failed stage and
error. Reports exclude credentials, song notes and opaque plugin state.

## Focused validation

The user requested section testing rather than an application-wide retest.

| Check | Result |
|---|---|
| Native editor tests | 7 passed, including real visible windows, optional frame support, close/reopen and precise native error results |
| VST3 host and scanner tests | 17 passed |
| Separate-helper editor and diagnostics test | 1 passed |
| Desktop project preparation tests | 14 passed |
| Desktop sampler processing tests | 22 passed |
| Desktop MCP session tests | 13 passed, including actual VST3 addition, visible editor and support reports |
| MCP crate tests | 66 passed |
| Update manifest/signature tests | 5 passed |
| Clippy on affected library crates with warnings denied | passed; plugin host repeated after error detail changes |
| Workspace formatting and diff whitespace checks | passed |
| Production frontend, native runtime/helper, desktop and NSIS builds | passed |
| Release executable plugin/MCP smoke | 11 checks passed |

The release smoke used `WINDFALL_SOFTWARE_AUDIO=1`, a private profile under
`target/r104/plugin-smoke/`, and the existing repository WebView2 DevTools
workflow, limited to plugins and MCP. It scanned the native fixture, proposed
and approved its addition over MCP, verified that the production separate
helper owned a visible 320-pixel editor, applied a wrapper setting, closed the
editor and checked detailed failure reporting and credential/state exclusion.
The initial smoke client omitted the modern MCP method/name headers; correcting
the client resolved that protocol rejection. The passing report is retained
at `target/r104/plugin-smoke/report.json`.

No full application suite, installed previous-version update flow, physical
audio/MIDI, macOS/Linux runtime or BBC Symphony Orchestra test is claimed.
Build output and focused logs are retained under `target/r104/`.

## Publication evidence

The installer was signed with the existing updater key. Its signature, signed
version/file identity, update manifest and hashes were verified locally, after
downloading the draft assets, and after anonymous public downloads. The public
latest-release API and `releases/latest/download/latest.json` both returned
`v1.0.4` / `1.0.4`. Project format remains 1.

| Asset | Bytes | SHA-256 |
|---|---:|---|
| `Windfall_1.0.4_x64-setup.exe` | 34,025,817 | `1dbc09ebddc7c1fcfe3e14d57a5023e349766decaf70ff20b7bf050d40486511` |
| `Windfall_1.0.4_x64-setup.exe.sig` | 436 | `cffa46df31ec7a65025c8f88858703b58d7861f407c7c5b06022c0ee08227458` |
| `latest.json` | 2,600 | `d2ca1ea6b3b7b10f165c9f2409cf2bd60b9c069078d777f9da98da5213883827` |
| `SHA256SUMS.txt` | 272 | `8c45f02474c8699c701fba1da7ad7fdb225c0fe33dad6aadac28a4a06a248073` |

The tested release executable reports product/file version `1.0.4`; its SHA-256
is `e423d203eec018a21d2b28164c510167b6615a9b3904b2cb2daa942700ef01d5`.
The bundled runtime distribution declares `1.0.4`, ABI 1 and
`x86_64-pc-windows-msvc`.
