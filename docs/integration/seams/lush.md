# Lush Space integration seam

2026-10-11 continuation: [E3-CIRCUITS.md](../../E3-CIRCUITS.md) specifies
appended pitchSemitones/pitchAmount/size controls, feed-forward wet pitch,
larger preparation rings and the updated conservative tail. Original descriptor
indices 0–7 and default plain hall audio remain intact. A dedicated editor and
full-row summary acceptance tests are authored; they are not executed here.

`crates/windfall-dsp/src/lush/` provides `LushSpace` and `LushSpaceParams`.
The processor implements `Effect`; its Copy, Default, serde camelCase and
`ts_rs::TS` parameter struct implements `ParamSet` through `param_set!`.
The display name is **Lush Space**. Its parity target is **`fx-luxeverb`**,
with **partial** coverage as an algorithmic hall.

Lush Space is not a convolution hall and not a replacement for Reverb or
Room. Two cross-coupled serial allpass tanks use nominal loop times of
428 and 490 ms, contrasting with Reverb's sixteen-line feedback network
and Room's small parallel comb circuit. Each tank includes a modulated
allpass, two long delays and a second allpass. Separate input diffusers
and five early taps per channel build density before the moving late tail.

## Parameters

The descriptor order is stable. JSON names, units, ranges and defaults:

| Field / JSON id | Unit | Range | Default |
| --- | --- | --- | --- |
| `decay_s` / `decayS` | seconds | 0–8 | 3.5 |
| `pre_delay_ms` / `preDelayMs` | milliseconds | 0–120 | 20 |
| `damping` | fraction | 0–1 | 0.45 |
| `early_level` / `earlyLevel` | fraction | 0–1 | 0.35 |
| `modulation_rate_hz` / `modulationRateHz` | Hz | 0–2 | 0.27 |
| `modulation_depth_ms` / `modulationDepthMs` | milliseconds | 0–2 | 0.7 |
| `width` | fraction | 0–1 | 1 |
| `mix` | fraction | 0–1 | 0.3 |

Decay is nominal low-frequency RT60. Zero selects a short 0.18 second
decay, with feedback strictly below unity. Damping lowers the loop lowpass
corner from 18 kHz toward 1.1 kHz. Linear fractional allpass reads add mild
treble loss; modulation and damping make the measured decay frequency
dependent. Rate zero holds the modulation at its reset phase; depth zero
removes delay modulation. Width scales wet side only, preserving wet mid
at zero. Early reflections have their own gain and do not feed the tanks.

## Realtime and limits

Construction and `prepare` may allocate; `prepare` sizes fixed delay lines
for the sample rate. `process`, `set_params`, `reset`, and the inherited
tempo setter perform no allocation, reallocation, free, locking or IO.
Controls glide over 20 ms after processing begins. Values supplied before
the first sample after prepare/reset apply immediately; empty blocks do
not consume that first-sample state. Processing and modulation clocks are
sample based, so irregular block splits give identical output.

Sample rates are sanitized to 1–384,000 Hz, with a 48,000 Hz fallback for
NaN/Inf. Parameters use their descriptor defaults for nonfinite values.
Nonfinite audio becomes silence; finite input is bounded to ±1,000 and
recursive/output state to ±1,000,000, with subnormal flushing. The low-rate
edge remains safe, but a credible hall response requires a normal audio
sample rate. Delay lengths round to the nearest sample.

There is zero dry-path latency. `tail_samples` and the conservative
`gap_samples` report an explicit silence lifetime of
`ceil((1.5 * max(decayS, 0.18) + 0.77) * sampleRate)` samples, retaining
the longest requested lifetime since reset once audio processing starts.
This protects circulating audio when decay is shortened. The maximum is
about 12.77 seconds, allowing an eight-second RT60 to fall farther below
audibility. The final 20 ms fade to exact silence, then tank histories are
cleared without releasing storage. At decay zero the lifetime is about
1.04 seconds. New input recovers the wet gain over 20 ms if it arrives
during that fade. Reset clears audio and restores deterministic phases.

This implementation has no impulse-response loader, convolution,
freeze/infinite-tail mode, tempo synchronization, size control, external
sidechain or preset bank. It does not claim full parity with its target.

## Registration handoff

The module is intentionally left unregistered. Integration should add
`pub mod lush;` to the crate root and wire the new types into the host's
effect kind/parameter/processor registries, project representation,
descriptor tests and TypeScript exports. None of those shared surfaces
are changed by this implementation. No binding generation is required
for the isolated implementation tests.

## Verification

From Git Bash, with the temporary `pub mod lush;` line present:

```bash
source scripts/msvc-env.sh
cargo test -p windfall-dsp --lib lush
```

Remove only that temporary line after the command completes, rereading
the crate root before insertion and removal to preserve concurrent work.
Format only the new Rust files with `rustfmt --edition 2024`.

Tests cover parameter metadata/serde/TS agreement, early and late impulse
arrival timing, modulation rate/depth changing the late tail, mono width
preserving mid, short finite zero decay, an audible later eight-second
setting, high-frequency damping, exact irregular-block/automation
equivalence, nonfinite inputs/rates/parameters, deterministic reset and
bounded expiry after decay automation. The allocator test compiles the
actual production library and a separate executable using its existing
dependencies. Its self-check detects alloc/realloc/free, then asserts zero
of each across prepared/unprepared callbacks, automation, reset and tail
expiry, without competing with another module's global test allocator.

Implementation verification on 2026-10-08: the command above passed all
13 matching tests (10 Lush Space tests and three shared flush tests).
The separate allocator executable passed its detection self-check and
zero alloc/realloc/free callback assertions. `rustfmt --check --edition
2024` passed for all three new Rust files. The temporary crate-root
module line was removed after verification.
