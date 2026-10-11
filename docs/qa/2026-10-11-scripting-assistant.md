# Assistant and scripting implementation hand-off

Branch: `gpt/w3-scripting-assistant`, base `87b87bc2439ec076d0f75c5b4d997c9db1faba95`.

Work in progress. All tests in this stream are **written, not executed**.
The worker is edit-only: no build, test, dependency installation, generation,
device access or network request was performed. No provider was selected.

Implementation scope: checked, revision/generation guarded command proposals;
offline help/intent provider; bounded Rust scripting with piano/controller
workers and a fixed realtime note-transform subset. No Python source compatibility.

The integrator owns generated bindings/WASM and validation. Per-row claims,
exact test commands, shared seams and remaining gates are updated below as
implementation lands.
