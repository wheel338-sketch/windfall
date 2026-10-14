# QA evidence

The 2026-10-09 pass follows the user's instruction to verify implementation
already present, repair failures, and mark accepted features done. The earlier
implementation-first QA deferral is superseded. All checks run locally;
GitHub CI and Actions are not used.

The final pass accepted 46 new completions. Current accounting is 136 done,
104 in progress, 100 todo and two exclusions. The [completion report](2026-10-09-completion.md)
lists exact accepted IDs, successful final gates and the remaining scope.

`../parity/parity.json` remains the completion source of truth. Its `done`
status means the full row's software behavior has accepted evidence. A passing
test of a subset does not complete an unfinished integration, device workflow,
plugin capability, or platform promise. Reports distinguish runtime tests,
source review, and outstanding manual acceptance.

This pass tests the dirty working tree based on
`be782b3c86058006c96c3374cc86f757b88c91a2`; that commit alone does not contain
the implementation or repairs under test. The execution reports record commands,
results, and generated artifact checks for the combined tree.

- [Completion acceptance and final gates](2026-10-09-completion.md)
- [Implementation inventory](2026-10-09-inventory.md)
- [Whole-feature completion audit](2026-10-09-completion-audit.md)
- [Frontend checks](2026-10-09-frontend.md)
- [Native checks and artifacts](2026-10-09-native.md)
- [Piano gestures](2026-10-09-rack-piano.md)
- [Piano randomizer and timeline contracts](2026-10-09-piano-contracts.md)
- [Recording and mixer](2026-10-09-recording-mixer.md)
- [Playlist and bounce](2026-10-09-playlist-workflow.md)
- [Arrangement transactions](2026-10-09-arrangement-repair.md)
- [DSP, plugins, and analyzers](2026-10-09-dsp-plugins.md)
- [Delay-bank integration and remaining gates](2026-10-09-delay-bank.md)
- [Plain notebook acceptance](2026-10-09-notebook.md)
- [Analysis UI and IPC](2026-10-09-analysis-repair.md)
- [Workflow and channel groups](2026-10-09-workflow.md)
- [Channel timing acceptance](2026-10-09-channel-timing.md)
- [Mixer UI and scale acceptance](2026-10-09-mixer-ui.md)
- [Engine warning cleanup](2026-10-09-engine-cleanup.md)
- [Project fixture reconciliation](2026-10-09-project-fixture-reconciliation.md)
- [DSP runtime harness reconciliation](2026-10-09-dsp-runtime-harness.md)
- [Live browser smoke check](2026-10-09-browser.md)

Logs retain initial failed attempts as diagnostic evidence. Use each report's
final accepted command results, rather than treating every historical log as
the final outcome.

The tracker reconciliation moved 85 ledger-mapped rows from `todo` to
`in-progress`: their documented implementation is a subset awaiting full-row
acceptance. This is not a QA pass or a completion promotion. Together with the
eight earlier scope corrections, it removes stale accounting while retaining
the actual remaining gates. Final accepted promotions are recorded separately.

## Completion rule and decisions — 2026-10-11

The user delegated the open product, licensing and tracker-policy decisions to
the overseer on 2026-10-11. This section records them; it supersedes the
stricter rule above where the two differ.

**When a row is `done`.** A row is done when everything that can be verified
on the development machine is built and passes in the full suites, and the only
checks left need something the machine does not have: a physical audio
interface or MIDI controller, another operating system, a person listening, or
the operator's credentials. Those rows carry a note beginning "Software
complete" that lists the external checks still outstanding. A row stays
`in-progress` when any software item is missing, when part of its code has
never been compiled or run here (the macOS-only Audio Unit host, for example),
or when it needs an asset the project does not have (a trained model).

**Decisions.**

- `fmt-import-rex`, `fmt-host-vst2`, `fmt-import-zgr`: won't do; the reasons
  are in the tracker.
- `fmt-export-m4a`: encode with the operating system's own encoder (Media
  Foundation on Windows); no encoder is bundled.
- `fmt-wavpack`: an in-repository decoder and encoder written from the public
  specification; no new dependency. Checking against files from the reference
  tool is an external check.
- `fmt-cloud-backup`: the provider-neutral folder backup is the product; no
  cloud SDK or account.
- `core-gopher`: an offline rule-based provider plus a bring-your-own-key
  provider that is off until the user enters a key; no provider SDK.
- `win-playlist-deverb`: classical two-mode dereverberation, judged on measured
  tail reduction and dry-signal damage, not on using a neural network.
- `core-denoising`, `editor-newtone`, `editor-newtime`, `wf-audio-to-midi`:
  the classical implementations are the product.
- `core-stem-separation`, `wf-remix-a-song`: stay open until a trained
  separation model is run end to end. Direction: ONNX Runtime with model packs
  the user imports; no weights in the repository.
- Sample pre-processing normalizes to −1 dBFS, as the audio editor does. An
  instrument track's pattern clips play only the linked channel, in playback
  and in MIDI export.

## Decisions — 2026-10-13

- **Deadline budgets** mean a share of one performance core. On a hybrid CPU
  the release deadline harness restricts its own measuring thread to the
  highest efficiency class, judges the minimum of three identical trials and
  still prints the median, the worst and one unrestricted trial. Limits,
  workloads and declared maxima are unchanged.
- **Code that exists only for macOS or Linux** counts as verified on this
  machine when it type-checks in the recorded cross-target check and its
  platform-independent logic passes its tests on Windows. Running it on those
  systems is an external check.
- **Recording rows** were accepted on the real-device harness run against a
  real WASAPI driver (a digital output with its loopback as input; no audible
  device and no microphone). An analogue round trip through a physical
  interface, ASIO hardware and unplugging are external checks.
- `core-stem-separation`, `wf-remix-a-song`: accepted after the real ONNX
  Runtime harness ran a four-stem model end to end (Open-Unmix UMX-HQ, MIT
  weights, exported locally; neither the runtime library nor the weights are
  in the repository). Real-music listening is an external check.
- `fmt-import-legacy-presets`: the public drum-synth text preset format is
  implemented in full. The reference product's small-synth (`.syn`) and speech
  preset files have no public byte-level description (search recorded in
  `2026-10-13-r6-formats.md`) and the plan rules out reverse engineering, so
  those files stay unsupported with an explicit message. The capability is
  completed with Windfall's own synth and speech presets rendering into a
  sampler channel as samples.
- `fmt-flp-import` (revised after the round-eight audit): the row is what the
  plan promises: notes, clips, automation, routing, controllers and sample
  references are read out of the project. Device and channel state is carried
  onto Windfall's stand-in wherever the public open-source readers describe
  the field and the stand-in has a matching control, judged against the
  stand-in's actual parameter list, not the importer's own classification. Every
  described field that still has no matching control (native device fields,
  channel envelope/LFO fields and synth switches alike) is counted and named as
  omitted in the import review, per device, with the opaque state kept, and one
  test pins that complete list and checks each entry against the stand-in's
  parameters. Adding new controls to the stand-ins is not part of the import
  row: the plan (`WINDFALL_PLAN.md`, "Identical sound from any FL project
  file") says a project only sounds the same where a Windfall equivalent
  exists, and the roadmap (F1) says to extend conversion only for supported
  targets and keep exact, approximate and omitted counts honest. Device state
  with no public layout stays opaque with a per-device warning.
- `wf-project-settings` (revised after the round-eight audit): a hosted plugin
  delivers one summed output, so the host cannot process one note inside it.
  Pan and volume each take exactly one route: where the plugin offers a native
  per-note route the plugin applies that property per note; otherwise the host
  applies it once at the channel level after the plugin. A note can therefore
  have its pan applied in the plugin and its volume on the channel (or the
  reverse). A property applied on the channel follows the most recent sounding
  note, so overlapping notes share it; per-note host processing of individual
  notes inside the summed output is not attempted. The project pan law is
  applied once in every arrangement, assuming the plugin applies its own
  per-note pan linearly as declared; how third-party plugins behave is an
  external check. A route change caused by a plugin gaining or losing a
  capability while notes sound must continue each running ramp from its
  current value without a click, on the path the app actually takes (in
  process, in a replaced helper process, and in the same unit).