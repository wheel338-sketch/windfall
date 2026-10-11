# Interface scaling

2026-10-11 follow-up (`gpt/w3-shell-release`, edit-only): application scale now
has a custom IPC path to the plugin owner thread, remembered for reopen and
applied to supported embedded Windows CLAP/VST3 content. Refusals are explicit.
This source has not been compiled/tested. Release bridge editor hosting, actual
monitor-transition notifications, macOS/Linux hosting, and physical/performance
gates below remain open. See `docs/qa/2026-10-11-shell-release.md` for details.
The phase-1 evidence below predates this follow-up and is not evidence for it.

This implements the shared application portion of `wf-ui-scaling` (phase 1,
roadmap R1). The full row also requires scaling native third-party plugin
window content. **That part is pending with the plugin-runtime owner; this
change does not complete the parity row.** The current editor API only opens
or closes an editor (`pluginEditor(target, open)`); it has no scale argument.
No plugin-runtime, Rust, IPC, project-format, generated binding or WASM changes
are part of this implementation.

## Preference and scope

Settings → Appearance → Interface scale offers 75%, 100%, 125%, 150%, 175% and
200%, with Restore 100%. The default is 100%. The preference lives in the
existing version-1 `windfall.ui` local-storage record, outside the musical
document. Missing, nonnumeric, non-enumerated, out-of-range and malformed JSON
preferences recover to 100% on startup. Old version-1 preferences remain
compatible. Same-origin storage events rehydrate preferences in already-open
panel views without echoing a storage write.

The main entry point applies CSS `zoom` to the document root. Layout reflows in
a smaller or larger logical viewport, including panel sizes, text, SVG icons,
audio controls, canvas headers, grid, menus, tooltips, dialogs, command palette,
built-in instrument/effect editors and generic hosted-plugin parameter forms.
The same entry point covers detached panel views and the control showcases.
This is application scaling in both browser and native webview rendering; it
does not call a native webview zoom API. A webview without CSS zoom support
uses 100%, disables the scale selector and explains that limitation.

Changing this preference invokes no project command, engine configuration or
backend operation. It creates no dirty state, document revision or undo entry.
Ordinary parameter/note edits continue to use their existing document commands.

## Coordinate policy

`src/lib/ui-scale.ts` is the shared contract. CSS zoom's layout/coordinate
behavior is specified in the [CSS Viewport specification](https://drafts.csswg.org/css-viewport/#zoom-property).

| Space | Meaning and conversion |
| --- | --- |
| Logical CSS pixels | Layout sizes, `clientWidth`/`clientHeight`, canvas viewport sizes, pixels per tick/row, scroll positions and control drag travel before application zoom. |
| Visual client pixels | Pointer `clientX`/`clientY`, `getBoundingClientRect()` positions/sizes and pixel-mode wheel distances. Divide relative positions and distances by `uiScaleFactor()` exactly once. |
| Backing/device pixels | Logical size multiplied by effective density: browser/native `devicePixelRatio` × application scale, subject to allocation bounds. Browser zoom/monitor DPI are already reflected in DPR and client pixels. |
| Ratio inputs | A pointer offset divided by a visual rectangle dimension already cancels application scale. Keyboard steps and line/page wheel units stay logical. |

Use `localPoint(element, event)` for border-aware local input,
`logicalDelta(distance)` for raw client deltas, and `logicalWheel(event)` for
wheel navigation. `TimeGridView.localPoint()` now returns logical pixels.
Callers keep their established line/page step sizes (playlist 16/400;
piano roll and mixer 1/1), preserving the 100% input behavior.
`createPointerFrame()` freezes both the origin and application scale at press,
preserving the established gesture contract when layout moves under a held
pointer. The musical tick/key/track/hit-test/snap algorithms remain intact.
Normalize strip/ruler/value-lane positions and scrollbar positions/lengths at
their DOM boundary as well; mixing an unscaled thumb with a scaled pointer
would change scroll extents.

`canvasResolution()` is shared by time grids, their header/value layers and
audio display canvases. It sanitizes logical dimensions, caps each edge at
8192 device pixels and each backing store at 16,777,216 pixels, including
rounding and exact observer boxes. Extremely large canvases lower density
instead of allocating an unbounded surface. All layers use the same policy
and pass their actual density to their painters/renderers.
Each canvas's cap is independent: a short ruler can retain a higher density
than a tall grid. Ruler/value painters derive `deviceTransform` from the shared
logical viewport with their own `LayerSize.dpr`, never the grid's device
transform. Logical alignment is retained within device-pixel snapping tolerance.
The layer tracks logical size and density as well as backing dimensions, so
an observer resize that leaves capped pixel dimensions unchanged still paints.
ResizeObserver
device boxes are accepted only when consistent with the effective density;
otherwise logical size × density wins, using the existing fractional-DPR
tolerance. Scale events, window resize and DPR media-query changes update
stores even without an observer delivery. Canvas2D, WebGL2 and optional
WebGPU still receive the existing device-transform contract and retain their
existing selection, restoration and fallback policy.

Floating UI/Base UI positioners report visual coordinates. Their portal gets
a fixed viewport frame with inverse root zoom; only popup content is zoomed
back to application scale. Anchor widths and available bounds are converted
to logical content units, retaining caller-specified popup widths. This avoids
double-scaling placement and keeps submenus/tooltips/selects on screen. Fixed
dialogs use logical viewport limits and scroll; the command list has its own
bounded scrolling area. Dense app headers/toolbars can scroll horizontally.
At 100%, the portal wrapper is `display: contents` and these non-default
layout overrides do not apply.

For future native editor integration, consume the application factor and
scale-change notification separately from the native window's monitor DPI.
Do not multiply both into an API that already handles monitor DPI. That owner
must implement supported-plugin scale negotiation, content resizing, failure
reporting, reopen/persistence and real native editor verification.

## Checks and evidence

Checks ran in `gpt/t3-ui-scaling`, based on
`42522322e96c04cffc831370cef762d2bf1a41d0`, after fast-forwarding the parent
artifact/document commit `186652c6f91463a629feadd9bf2641f54f958956`.
No Cargo build or local artifact regeneration was needed.

New focused tests:

- `ui-scale.test.tsx`: actual Settings selector/Restore control, persistence
  rehydration, invalid/malformed recovery, unsupported fallback, storage-event
  application, unchanged document identity and zero dispatch/configure calls.
  Actual knob/fader DOM drags reach both endpoints at 75/125/150/200%, with
  keyboard Home/End. Generic `PluginControls` number-field events edit a retained
  fixture binding through the real project command path at those four scales.
- `canvas/scaling.test.ts`: real common canvas/view/input across 75/125/150/200%
  × DPR 1/2/1.25, note hits, a frozen pointer frame, ResizeObserver logical
  resize and inconsistent device boxes, scale change without observer delivery,
  bounded square/nonsquare allocations and the audio-canvas density contract.
- `canvas/scaling-input.test.ts`: real piano grid and playlist pointer handlers,
  WASM document commands and sessions at the four scales. Note draw/hit/move,
  box/lasso selection and wheel navigation; playlist clip placement/movement;
  automation point insertion/hit/movement. Drawing contexts are test doubles;
  editor/input handlers and document commands are real.

The first scoped regression command passed **16 files / 224 tests**, covering
the three scaling files, time-grid view, pointer frame, renderer parity,
WebGL2 restoration/fallback, renderer contract, drag-value, fader, envelope,
waveform, Settings, stamp lifecycle, playlist pointer and automation UI tests.
A follow-up passed **5 files / 62 tests** for preference/density updates,
menu close behavior, hosted plugin controls and descriptor parameter controls.
The final common scaling run passed **3 files / 44 tests**, including the four
additional generic hosted editor scale cases and preservation of piano-roll
line/page wheel steps. All runs use `--maxWorkers=2`; no full suite was run.
Typecheck, ESLint, Vite production build, changed-source Prettier checks and
`git diff --check` passed. Vite reports its existing large-chunk advisory.

Commands from `apps/desktop` (the whitespace check is from the worktree root):

```powershell
pnpm test src/lib/ui-scale.test.tsx src/lib/canvas/scaling.test.ts src/lib/canvas/scaling-input.test.ts src/lib/canvas/time-grid-view.test.ts src/lib/canvas/pointer-frame.test.ts src/lib/canvas/renderer-parity.test.ts src/lib/canvas/renderer-webgl2.test.ts src/lib/canvas/renderer.test.ts src/components/audio/use-drag-value.test.tsx src/components/audio/fader.test.tsx src/components/audio/envelope-editor.test.tsx src/components/audio/waveform.test.tsx src/features/settings/settings.test.tsx src/features/piano-roll/stamp-lifecycle.test.tsx src/features/playlist/pointer.test.ts src/features/playlist/automation/automation-ui.test.tsx --maxWorkers=2
pnpm test src/lib/ui-scale.test.tsx src/lib/canvas/scaling.test.ts src/features/layout/menus-close.test.tsx src/features/plugins/plugins.test.tsx src/features/params/param-control.test.tsx --maxWorkers=2
pnpm test src/lib/ui-scale.test.tsx src/lib/canvas/scaling.test.ts src/lib/canvas/scaling-input.test.ts --maxWorkers=2
pnpm typecheck
pnpm lint
pnpm build
git diff --check
```

T3 preview used an isolated tab at `http://127.0.0.1:5211`, with status checked
before opening. Browser interactions used actual clicks, keys and scrolling;
store/session reads supplied verification only. No React state injection was
counted as user behavior. Runs occurred on 2026-10-08 UTC in
HeadlessChrome 154.0.8037.92, reporting Windows NT 10.0/Win64 and DPR 2.
This is a server preview, not a physical native-window test:

| Scenario | Observed result |
| --- | --- |
| Settings at 1280×800 | Clicked 75/125/150/200% options successfully; selected preference persisted. Reload retained 200%. |
| Settings at 800×600, 200% | Stable dialog bounds `(32,32,736,536)` visual pixels; scroll height 1192 versus logical client height 268. Scrolled to Appearance/Keyboard, clicked Restore 100%, selected 200% again, and reached Close by ordinary focus/scroll behavior. |
| Command palette | At 200%, 1920×1080 bounds `(448,112,1024,856)`; at 200%, 800×600 bounds `(32,36,736,528)` with a scrolling list. At 75%, 800×600 bounds `(208,139.5,384,321)`. All stable bounds fit the viewport; Escape closes. |
| Menus | Channel selector options selected through their real popup; Options menu at 75% fits `(273.5,25,168,96.75)`. Note context menu at 200% fits the desktop viewport and preserves the note actions. |
| Piano roll at 200% | Snare note placed at tick 2160/key 60, then selected and moved by ArrowRight/ArrowUp to tick 2400/key 61. History contains Add note and the two Move note commands. Grid backing 2656×335 for about 1328×167 visual pixels at DPR 2. |
| Piano roll at 75% | Kick note placed at tick 2400/key 60, then selected and moved to tick 2640/key 61. The initial probe landed on an existing note's end, correctly identifying the resize hit; the placement check used empty space. |
| Audio/built-in controls | Master volume Home/End reaches 0/2 at 200%. Sampler inspector scrolls to all sections; Tune Home/End reaches −48/+48, with a 72×72 visual knob at 200%. At 75%, the descriptor showcase's Balance Gain reaches 0/4 through Home/End and records the normal parameter gestures. |

Early popup checks exposed double-scaled coordinates and a portal stacking
problem; both were corrected and the actual Settings option clicks were
repeated successfully. Incorrect preview locators and before-unload prompts
from isolated test edits were corrected/resolved before continuing. Browser
drag trajectories are covered by the DOM input tests; the preview tool's
locator-only drag API was not used to claim physical pointer-drag evidence.

## R1 boundary repairs

The independent review of `f418da57be12c4f9dd1f9f6971e431c2624c6b14`
found two P2 boundaries. Both are repaired in this follow-up; the full parity
row remains pending for native editor scaling and external verification.

The browser owner acknowledged the `browser/index.tsx` reservation before
edits. Only that feature file and a new feature layout test change. The virtual
tree gets a nonshrinking 128-logical-pixel minimum window. Direct content
sections get a 216-logical-pixel minimum width, and the outer panel scrolls
both axes. Large panels keep the tree's existing flexible fill behavior;
small panels can scroll to search, footer, metadata and preview controls.
The tree's virtualization/scroller, library control limits, import/store/cache
logic, search and preview implementations are untouched.

Ordinary own-tab Settings clicks reproduced a **zero-height tree** before the
repair at both 1920×1080 and 800×600, 200%, DPR 2. After repair, the final
Settings-driven matrix measured the following `browser-scroll.clientHeight`
values in logical pixels with Kick 02 selected and metadata visible:

| Viewport | 75% | 100% | 125% | 200% |
| --- | ---: | ---: | ---: | ---: |
| 1920×1080 | 845 | 484 | 218 | 128 |
| 800×600 | 139 | 128 | 128 | 128 |
| 640×480 | 128 | 128 | 128 | 128 |

Both overflow axes report `auto`. Search retains at least 100 logical pixels
of input width in this matrix. Narrow panels display part of the wider content
at a time; horizontal scrolling exposes the remainder. At 640×480/200%, actual
click/type/save interactions reached Tags and saved the fixture tag `scaled-r1`.
Keyboard Tab from the tree reached the Add folder footer. Scrolling both axes
reached the preview's Add to playlist button, whose visible intersection passed
the browser's `elementFromPoint` pointer test. At 800×600/200%, ordinary clicks
opened Drums/Kicks, selected Kick 02, reached Save tags and played its preview;
facts showed 0.55 s, 44.1 kHz and Mono. These checks did not invoke musical
import actions. Restore 100% and selecting 200% again worked; navigation reload
retained 200% and a 128-pixel tree. Project revision stayed 0, dirty stayed false,
and history retained zero entries/cursor throughout the preference checks.

`layer-scaling.test.tsx` mounts the actual Ruler and ValueLane on a real
TimeGridView/session with WASM document commands. Recorded painter calls
reproduced the erroneous marker at logical X **217.285** for the 4000×2000
grid and **153.809** for the 5000×5000 grid, versus tick 3840's expected X
**307.2**. Before repair, an observer-only width change 5000→5001 also failed
to repaint a capped ruler whose backing dimensions remained equal.

The eight layer tests now pass: square/nonsquare caps at 200% and DPR
2/1.25/1.5, 125%/DPR 2, and uncapped 100%/DPR 1; actual rendered grid-note,
ruler-end and value-cap positions agree with logical hits. Presses on the
rendered value bar reach velocity 1 and 0 through the real update command.
Dragging the rendered ruler marker changes pattern length from 16 to 32 steps
and redraws it at logical X 614.4. Tests cover a bordered grid container,
observer-only equal-backing resize, resize back to an uncapped view, application
scale/DPR changes and fractional scrolling. They retain the 8192-edge/16M-pixel
bounds. Layout and 2D rasterization are test doubles here: recorded real painter
calls establish the source rendering/input contract, not hardware raster pixels.

`layout-scaling.test.tsx` mounts the actual browser at 75/100/125/200%, checks
the minimum-window/overflow DOM contract, then searches/selects a sample and
saves metadata through real controls, with unchanged project/history identity.
It does not claim jsdom has measured layout. Actual pixel-height and overflow
evidence comes from the T3 preview above, in the same isolated port-5211 tab,
HeadlessChrome 154.0.8037.92 / Windows NT 10.0 / DPR 2 on 2026-10-08 UTC.

The focused regression run passed **11 files / 131 tests**. After the narrow
horizontal-overflow improvement, the browser layout/tree/preview rerun passed
**3 files / 50 tests**. Both runs used two workers maximum. Typecheck, ESLint,
production build, changed-source formatting and `git diff --check` passed;
the existing Vite large-chunk advisory remains. No full suite, Cargo build,
native build or artifact regeneration was run.

```powershell
pnpm test src/features/browser/layout-scaling.test.tsx src/features/browser/browser-tree.test.tsx src/features/browser/browser-preview.test.tsx src/features/browser/library.test.tsx src/features/piano-roll/layer-scaling.test.tsx src/lib/ui-scale.test.tsx src/lib/canvas/scaling.test.ts src/lib/canvas/scaling-input.test.ts src/lib/canvas/time-grid-view.test.ts src/lib/canvas/renderer-parity.test.ts src/lib/canvas/renderer-webgl2.test.ts --maxWorkers=2
pnpm test src/features/browser/layout-scaling.test.tsx src/features/browser/browser-tree.test.tsx src/features/browser/browser-preview.test.tsx --maxWorkers=2
pnpm typecheck
pnpm lint
pnpm build
git diff --check
```

The protected stamp editor/grid-input files are unchanged in this follow-up.
The independent stamp-cancellation findings remain separately owned. Native
third-party window content scaling still requires the runtime owner's API work.
The physical/native-window, detached-window, WebGPU and scale-specific
performance limits below remain. This round did not add ordinary preview
playlist/automation editing or freehand-lasso evidence; their existing focused
DOM/WASM checks must not be described as physical interaction proof.

## Integration boundaries and pending verification

The consuming changes are deliberately narrow. Protected
`piano-roll/grid-input.ts` only imports the common conversions and normalizes
wheel/pan distances; its stamp/editor gesture behavior is not rewritten.
`editor.ts`, stamp logic and note-tool/dialog/menu/action implementations are
untouched. The other piano strips, ruler, value lane, scrollbar and canvas
layer only adopt the logical coordinate/density boundary. Playlist rulers,
scrollbars/track-header positions, EQ display, tempo drag, mixer wheel and
toast offset make the corresponding DOM-boundary conversion.

`channel-rack/rack-grid.tsx` has one overlap with the rack worker: reorder-gap
Y is normalized before division by the existing row height. Preserve that
conversion when integrating channel-operation work. Parent owns roadmap,
parity, root integration and generated artifacts; none are updated here.

Pending external checks include native Windows/WebView2 monitor transitions,
physical pointer/touch/trackpad behavior, macOS WKWebView, Linux WebKitGTK,
multiple physical detached windows and native third-party editor content
scaling. Unsupported CSS zoom is exercised as a test fallback, not on an old
physical webview. Real WebGPU hardware/device loss and a new 10k/50k performance
benchmark at each scale were not run. Existing renderer policy and mock parity
checks passing do not establish those external results. The entire
`wf-ui-scaling` parity row must remain pending until native editor integration
and its required external evidence are accepted.
