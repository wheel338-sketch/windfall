# Round-six languages hand-off — 2026-10-13

Worktree: `.claude/worktrees/r6-languages`; branch: `gpt/r6-languages`;
base: `c0144d315689c058a29653f785b1a91ae2bafc99`.
Row: `wf-languages`.

**Partial source implementation. This does not finish the requested whole-app
translation and must not be accepted as software complete.** The whole-source
and descriptor admission tests deliberately retain zero-omission assertions.
Existing omissions are expected to make those tests fail; no passing result is
claimed. They were not narrowed to the files edited in this round.

Implementation commits:

- `1f7b1dde`: coded MIDI reviews, reports and diagnostics; retained diagnostic
  toast contents; catalogs and equality admission.
- `2754738b`: reactive parameter controls, menu metadata, retained settings/
  update failures, numeric input/readouts and detached window titles.
- `f575912d`: whole-source and exhaustive descriptor admission guards.

Nothing was built or run as verification. No Cargo, pnpm/npm, Vitest, tsc,
formatter, application, native probe, generator, installation or download ran.
Only source reads/edits, source-reading inventory scripts and Git ran. Git
whitespace checks passed; commit hooks were disabled to honor the edit-only
contract. No subagents, push, PR or tag. The only read outside this worktree was
the contract at the parent path explicitly provided in the task. All source,
inventory, Git and editing operations used this worktree.

No dependencies, generated bindings, simulator artifacts, parity trackers,
musical registries, descriptor order or project persistence fields changed.
The MIDI wire fields retain their existing string types. The integrator must
rebuild the simulator to exercise the new IPC implementation in real-WASM tests.

## Measured size and limits of measurement

The initial read-only regex inventory found **3,402 candidate literal
occurrences / 2,752 distinct texts in 940 TS/TSX files**. It covered raw JSX
captions, direct display properties/data and common hint/error/toast setters,
excluding `.test`/`.spec` files, generated bindings and the entire i18n directory.
This is a candidate inventory, **not the AST guard's measured result**: it can
include technical data and miss indirect/template expressions. The AST guard
was not executed, so its before/after violation counts and actual allowlisted
occurrence counts remain unmeasured. Do not substitute the regex count for a
passing or measured AST gate.

Source inspection of the spread catalogs gives **953 unique keys in English
and 953 in Spanish**, up from 877. The additions are 59 native catalog entries
(57 wire codes and two decoder diagnostics) and 17 shared-control/title keys.
The native wire registry has **57 append-only codes**. New Spanish entries are
actual translations, not English padding.

The exception data is solely `src/lib/i18n/invariants.ts`: **32 general literal
entries, eight keycap-only keyboard entries, six anchored notation/unit/codec
patterns and 14 justified same-as-English catalog entries**. General entries
cover the product name, protocol/format acronyms, units and axis symbols.
Patterns cover numbers/punctuation, international note names, extensions/codec
identifiers, measured unit readouts and oversampling notation. Keyboard exceptions
apply only to keycaps: the audio parameter “Space” is not exempt. Equality entries
cover established shared musical/Spanish words and parameter/punctuation-only
templates. Each entry has a reason. There is no prose baseline/feature allowlist.

Generated descriptor inspection found **68 effect kinds / 1,095 parameters and
50 instrument kinds / 1,577 parameters**, totaling **2,672 parameter records**.
Parameter/choice captions collapse to 722 distinct texts when numeric indices
are normalized. This demonstrates why translating only inspector entry points
could not close the row.

## What changed

- `src/lib/i18n/source-guard.ts` and `whole-app-coverage.test.ts` scan all app
  TS/TSX sources, excluding tests/test harness, generated bindings and the five
  catalog modules. A virtual TypeScript program reads supplied strings only;
  it never emits or loads dependencies. The guard follows local/imported
  constants and helper returns, recognizes actual translation imports/aliases
  and translator factories, distinguishes shadowed functions and verifies keys
  in every catalog. It checks JSX, display props/data/defaults, hints/errors/
  prompts/toasts (including promise states), menu/action titles, imperative
  text/attributes and canvas captions. It rejects semantic literal parameters
  and concatenated translated fragments. Trusted parameter-control render
  boundaries are identified by their defining source module, not tag spelling.
- `descriptor-coverage.test.ts` additionally enumerates every generated kind,
  parameter, choice, generated group/short label and track-processing name.
  Runtime descriptor text cannot disappear behind exclusion of generated files.
- `crates/windfall-ipc/src/ui_messages.rs`, registered in `lib.rs`, defines the
  `windfall-message:` JSON string envelope, code registry and MIDI adapters.
  Every MIDI adjustment, export report notice, export-limit error and current
  parser diagnostic reason has a code plus parameters. Session preview expiry/
  replacement/exhaustion and export-extension errors use it. Core MIDI Display
  remains available for non-UI callers. OS I/O details remain opaque parameters.
- `src/features/midi/dialogs.tsx` retains the encoded/original failure and review
  strings, translating while rendering. `lib/i18n/index.ts` validates envelopes,
  required placeholders and finite primitive parameters, then formats using
  current-language catalogs/Intl. Unknown or malformed codes are visible.
- `lib/errors.ts` and `lib/i18n/diagnostic.tsx` put retained coded messages and
  dynamic title metadata into reactive toast contents. `lib/actions/index.ts`
  and `components/context-actions.tsx` preserve registered/inline title metadata
  when reporting an action failure.
- Shared `ParamControl`, `ParamGroup`, `ParamChoice`, `GenericParamEditor` and
  effect `ParamFader` subscribe to language changes. Captions already present in
  catalogs translate at these boundaries. Parameter hints use complete templates.
  Numeric readouts use Intl; localized input and canonical technical input are
  accepted without changing the canonical `formatParam` API. Faders publish unit
  identity. Bare-number clipboard state retains canonical numeric input for
  by-text pastes across locale changes.
- Context menus use `actionTitle` for registry entries and retain optional
  `titleMessage`, `labelMessage` and `submenuMessage` metadata for inline entries.
  Unmarked authored labels stay raw, including names equal to a catalog key.
  Value-menu commands carry metadata and refusal reasons translate. Dropdown
  action refusal reasons translate too.
- MIDI hardware settings, native note-capability hooks and Updates retain raw
  failures; Updates also retains status keys/parameters. They translate during
  rendering instead of storing the current language's prose.
- `features/layout/use-app-boot.ts` and `app-shell.tsx` update main and detached
  native window titles on locale changes. Authored project names remain unchanged.

## Exact tests for the integrator

All tests below are written, non-ignored and **unexecuted**.

`crates/windfall-ipc/src/ui_messages.rs` (registered through `lib.rs`):

- `ui_message_codes_are_unique_and_round_trip_without_preformatted_prose`
- `every_midi_adjustment_has_a_stable_code_and_numeric_or_authored_parameters`
- `midi_export_report_codes_preserve_counts_and_have_no_zero_count_notices`
- `parser_diagnostic_reasons_have_stable_codes_instead_of_english_parameters`
- `every_parser_source_reason_is_registered_before_it_can_reach_the_ui`

Frontend:

- `lib/i18n/whole-app-coverage.test.ts`: `walks all application TypeScript and TSX rather than a scoped feature list`; `follows imported captions and helper returns and respects local translation shadowing`; `detects captions, props, data registries, nested helpers, retained errors and phrases`; `recognizes translated control imports by their defining module, including aliases`; `accepts translated messages and explicitly justified invariant notation only`; `rejects semantic string parameters and concatenations of translated fragments`.
- `lib/i18n/descriptor-coverage.test.ts`: `requires translated processor names, parameter names, choices and generated group labels`.
- `lib/i18n/catalogs.test.tsx`: new `rejects untranslated English entries unless their equality has an explicit reason`; retain existing complete-key/plural/placeholder tests.
- `lib/i18n/native-messages.test.ts`: `has a catalog entry in every language for every Rust wire code`; `retranslates retained codes, plurals and numeric parameters without changing authored names`; `rejects malformed envelopes visibly and leaves ordinary user text outside the decoder`.
- `lib/i18n/diagnostic.test.tsx`: `keeps the code and retranslates its mounted title and diagnostic`.
- `lib/errors-locale.test.tsx`: `keeps code and title parameters in the emitted toast so a locale change updates both`.
- `lib/i18n/control-locale.test.tsx`: `updates group headings, accessible names, numeric readouts and choices without editing values`.
- `features/params/format-locale.test.ts`: `uses locale decimals and reads them back while accepting canonical technical input`; `preserves the canonical parsed value at every descriptor endpoint and default in both locales`.
- `components/context-actions-locale.test.tsx`: `retranslates open static and dynamic entries and their refusal reason`.
- `components/value-context-menu.test.tsx`: new `pastes a retained localized bare-number readout using its canonical numeric input`; retain all prior unit/refusal/gesture tests.
- `features/layout/project-name.test.tsx`: new `retranslates a detached native title immediately and preserves authored project names`.
- `features/updates/section.test.tsx`: new `retranslates retained availability and native errors without checking again`.
- `features/midi/dialogs.test.tsx`: new `retranslates a retained native failure while the failed review stays open`; existing mounted preview and unsupported-content assertions now measure coded plural notices.
- Existing `session/tests/midi.rs::reviewed_import_still_reports_unsupported_key_signature_without_false_marker_loss` retains its omission/history/marker assertions using stable message identifiers.

Suggested integrator commands, not run here:

```text
cargo test -p windfall-ipc --lib ui_messages
cargo test -p windfall-desktop --lib session::tests::midi
pnpm --dir apps/desktop test src/lib/i18n src/lib/errors-locale.test.tsx src/features/params src/components/value-context-menu.test.tsx src/components/context-actions-locale.test.tsx src/features/midi/dialogs.test.tsx src/features/layout/project-name.test.tsx src/features/updates/section.test.tsx
```

Run normal compilation/format/lint and existing MIDI/settings/plugin/layout/
palette regressions after simulator rebuild. Expect the whole-app and descriptor
admission gates to be red until the real omissions below are implemented.

## Remaining software

SOFTWARE: Whole-app translation is not implemented. Custom instrument/effect,
sample/rack editors, auxiliary/helper/tool menus, dialogs/toasts/empty states,
remaining help and error boundaries still contain English. The broad guard has
not been made to pass. An AST before/after/allowlisted-occurrence measurement is
still required; only the regex inventory and catalog/descriptor counts above
were measured in this edit-only worker.

SOFTWARE: The kind/parameter-id descriptor translation registry is not implemented.
The shared renderers still use the transitional English-text lookup and fallback.
Many full/short/group/choice names have no key. Automation target descriptions,
controller-link menus and other descriptor consumers remain unmigrated. Complete
their keyed/provenance-aware labels, preserving authored/vendor names and every
persisted index; the new descriptor gate exposes this rather than accepting it.

SOFTWARE: Native code conversion covers the declared MIDI paths only. MIDI hardware,
shared project/recording/path validation, plugin/helper/capability errors and other
import/status/refusal paths still return prose. `localizeBackendText` retains its
legacy prefix/opaque-English diagnostic fallback; this is not complete translation.
The MIDI I/O `detail` parameter is an opaque OS diagnostic. Extend the append-only
registry and exhaustive UI/Rust tests to the remaining app-owned messages.

SOFTWARE: Retained messages outside the edited consumers, ordinary legacy toasts,
dynamic semantic option parameters and general counts/number/date/time consumers
still need structured references and locale formatting. Value-menu captured
names/readout strings can retain the old locale even though the command caption
retranslates. Inline semantic menu callers must supply message metadata; blindly
translating authored labels would corrupt their display. Runtime native caption/
menu completeness is not established by the detached-title hook: create-time Rust
captions and other native owners still require an audit and complete wiring.

EXTERNAL-ONLY: After these software omissions are closed, independent Spanish copy/
accessibility review, text expansion and native-window/dialog behavior on other
operating systems. These do not excuse the software work listed above.

## Least certain spots

- The virtual TypeScript compiler host, alias/scope tracing and trusted component
  recognition have not been compiled or executed against TS 6.0.2. Inspect the
  fixture tests first. Conservative property/return tracing can report technical
  styling or canonical formatting as display data; repair context recognition
  without hiding prose in an allowlist or narrowing the scanned files.
- Source admission is not a proof of every runtime dataflow. The exhaustive
  descriptor/backend tests and mounted lifetime tests remain necessary. Their
  unresolved omissions must not be relabeled external checks.
- React-element toast payloads for coded/dynamic messages need actual Sonner and
  existing action-error tests. Retained English semantic parameter strings still
  need proper nested/keyed metadata; these helpers cannot infer their meaning.
- Locale numeric parsing, precision, unit-aware copying and live fader/knob input
  need the exhaustive round trips and existing gesture/copy tests. Canonical
  formatter behavior was retained, and no numeric tolerance was relaxed.
- Native JSON envelope serialization, parser source-reason enumeration and the
  Rust-source raw import used by the UI registry test require compilation/Vitest
  resolution. Rust formatting is pending. Cross-platform window-title ownership
  requires integration/runtime evidence.

All implementation edits were committed before this record was written. This is
a partial hand-off for integration and continued implementation, not row closure.

HAND-OFF COMPLETE
