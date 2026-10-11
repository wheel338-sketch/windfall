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
