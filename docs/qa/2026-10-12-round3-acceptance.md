# Round 3 row acceptance — 2026-10-12

## Running progress

- Starting checkpoint: `feat/parity-100` at `9ef9599a`; clean checkout, no merge in progress and no active Windfall build/test process. Read the complete integrator brief, roadmap implementation contract and QA completion decisions. No round-three test evidence yet.
- Ordered integration: control sources; MIDI second pass; modular rack; scripting/assistant; plugin hosting; piano second pass; sample instruments; analysis/ML; rack/mixer second pass; files second pass; analyzers second pass; performance/video; remote/cloud/content; shell/release.
- All build/test commands run sequentially: Cargo eight jobs via Git Bash helpers; Vitest four workers. Commit each verified step. No delegation, device/ignored harnesses, network fetches, parity inventory edits, push or PR.
- Resumed from `f75a9801` on `feat/parity-100`, clean main checkout. Read QA completion decisions, roadmap sections 3/9, wave-two report, control-source hand-off and MIDI second-pass hand-off. Branch 1 was previously merged at `e44f8356`; marker cleanup at `f75a9801` had no build evidence.
- Inherited check: `cargo check --workspace --all-targets` initially failed on two stale tuple matches for asset-bearing `ChannelSource::Instrument` (`target/round3-inherited-check.log`). Repair 1 then exposed missing Clone implementations for immutable plan control-source/link descriptions (`target/round3-inherited-check-repair1.log`). Repaired both causes; repair 2 exits 0 with zero errors/warnings (`target/round3-inherited-check-repair2.log`). No tests run and no manifest/lockfile changes accepted at this checkpoint. Next: merge `gpt/p2-midi`.
- Branch 2 MIDI merged at `565d09fc`; workspace check exits 0, zero errors/warnings (`target/round3-branch2-check-repair4.log`). Preserved dynamic internal-source menus plus settings editor entry, legacy sample scalar masks plus MIDI destination checks. Repaired ts-rs optional views for scalar/vector fields, public curve-limit and JSON API paths, unused closure bindings, and automation-touch cleanup before moving a refused take into recovery. The bridge cancellation/deadline test incorrectly expected 2048 zeros for its 128-frame buffer; it now asserts every actual sample is zero without changing the fixture or assertion strength. Failed checks retained as `round3-branch2-check.log` and `round3-branch2-check-repair1..3.log`. No dependency changes. Focused tests and helper rebuild pending.
- Branch 3 modular rack integration underway. Read hand-off and its changed rack/notemap seam contracts; conflicts preserve all modules/tests, append `SetChannelRack` after existing commands, retain both settings tables and bounded refusal counters, and combine rack identity with existing immutable instrument/sample asset identities. No tests yet.

## Verified execution record

Inherited workspace check passes after repairs. Focused tests, regenerated artifacts and all final gates remain pending; no passing tests are credited yet.

## Row acceptance

| Row and required summary | Passing test evidence (see exact-file index) | Still open / hand-off reconciliation | Recommendation |
| --- | --- | --- | --- |

The row inventory is being reconciled from all fourteen hand-offs. No unexecuted test is credited as passing evidence.

## Branch fixes, dependency audit and seams

- Pending all fourteen merges and manifest audits. Source/descriptor prefixes are relative to `9ef9599a`.
