# Per-plugin wrapper settings

The native plugin inspector stores wrapper settings with each `PluginBinding`.
`SetPluginWrapper` uses ordinary document history, duplication and save/reopen.
Default settings are omitted so older projects keep their existing behavior.
These settings change preparation/lifecycle and are not audio automation targets.
The implementation and tests are **source candidates, written but not executed**;
see the [round-four hand-off](../qa/2026-10-12-r4-hosting.md).

- **Smart disable** stops native processing after a finite tail and a second of
  observed quiet input/output. Input, notes or controls wake it. Endless tails
  are never disabled. This preserves tail audio rather than skipping a silent
  input while a plugin is still sounding.
- **Native buffer** collects exactly B native samples with sample-offset events
  and sidechain input. It adds B samples of reported latency in both direct and
  helper hosting; bridge collection/scheduling latency remains additional.
  Variable uses incoming native blocks. Preparation reserves all buffers and
  event storage off audio. Pending parameter edits return to the inactive owner.
- **Processing** selects Automatic (the platform default), In process, Threaded
  or Bridged. Automatic uses the helper supervisor on Windows, macOS and Linux.
  The Unix transport is a source candidate compiled only on those Unix targets.
  In process shares
  desktop crash/hang exposure. Bridged uses actual isolated native execution,
  timeout failure, aligned dry fallback for effects and silence for instruments.
  Threaded keeps the native instance/editor on its creating owner and moves its
  prepared audio backend to one plugin worker. Two preallocated packets pass
  through one-entry queues. Collection and worker scheduling add two prepared
  maximum blocks to native latency (plus any fixed-buffer policy latency).
  Variable callback partitions preserve that exact sample delay. Audio neither
  waits nor signals an OS primitive; a missing reply at the next quantum or a
  full event packet latches an observable processing failure. Native completion,
  rather than submission, authorizes desktop parameter acknowledgement. Offline
  rendering explicitly waits on its render thread. Reset fences old replies;
  deactivation/capture joins the worker on control and retains newer collected
  parameter intent. Refused VST3 deactivation restarts the same pipeline on a
  new worker without changing the saved policy or latency. Threaded shares the
  desktop's native crash exposure, and control retirement cannot preempt a hung
  native call; Bridged supplies process termination for that case.
- **Native editor scale** calls CLAP/VST3 native scaling interfaces. Custom
  unsupported/refused scales fail visibly. Existing system DPI applies too.
- **Detached editor** removes the native parent. Turning it off makes a native
  top-level window owned by Windfall's current HWND; the HWND is transient and
  never stored. Floating-only plugin editors may refuse attachment. This is
  window ownership, not an embedded inspector pane.

Processing policy edits replace the prepared native owner; editor preferences
apply on opening its window. Integrator acceptance must check active dirty-state
capture, undo/replacement races and helper-owned attached windows. The Threaded
provider and UI are implemented in this source batch with deterministic tests;
none was built or run by the worker. Cocoa/X11 host-owned editor embedding and
the broader dynamic-capability/multi-output/capture-restart work remain software
items in the shared hosting rows. External plugin acceptance remains separate.
