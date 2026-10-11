# Per-plugin wrapper settings

The native plugin inspector stores wrapper settings with each `PluginBinding`.
`SetPluginWrapper` uses ordinary document history, duplication and save/reopen.
Default settings are omitted so older projects keep their existing behavior.
These settings change preparation/lifecycle and are not audio automation targets.
The implementation and tests are **source candidates, written but not executed**;
see the [hosting hand-off](../qa/2026-10-11-plugin-hosting.md).

- **Smart disable** stops native processing after a finite tail and a second of
  observed quiet input/output. Input, notes or controls wake it. Endless tails
  are never disabled. This preserves tail audio rather than skipping a silent
  input while a plugin is still sounding.
- **Native buffer** collects exactly B native samples with sample-offset events
  and sidechain input. It adds B samples of reported latency in both direct and
  helper hosting; bridge collection/scheduling latency remains additional.
  Variable uses incoming native blocks. Preparation reserves all buffers and
  event storage off audio. Pending parameter edits return to the inactive owner.
- **Processing** selects Automatic (the platform default), In process or Bridged.
  Windows Automatic uses the existing helper supervisor. In process shares
  desktop crash/hang exposure. Bridged uses actual isolated native execution,
  timeout failure, aligned dry fallback for effects and silence for instruments.
  Non-Windows bridging is refused. Threaded has no provider and is visibly
  unavailable; a saved threaded policy produces an explicit runtime error and
  can be changed to a supported mode. It is not an inert enabled switch.
- **Native editor scale** calls CLAP/VST3 native scaling interfaces. Custom
  unsupported/refused scales fail visibly. Existing system DPI applies too.
- **Detached editor** removes the native parent. Turning it off makes a native
  top-level window owned by Windfall's current HWND; the HWND is transient and
  never stored. Floating-only plugin editors may refuse attachment. This is
  window ownership, not an embedded inspector pane.

Processing policy edits replace the prepared native owner; editor preferences
apply on opening its window. Integrator acceptance must check active dirty-state
capture, undo/replacement races and helper-owned attached windows. Threaded
processing, cross-platform editor ownership and external plugin acceptance keep
`fmt-host-wrapper-options` partial.
