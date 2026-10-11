//! The Windfall audio engine: realtime playback, offline rendering and the
//! audio device.
//!
//! The control side compiles a [`Project`](windfall_project::Project) and a
//! [`SamplePool`] into an immutable plan and sends it to the audio thread
//! over a lock-free queue. The audio thread swaps plans between buffers and
//! sends the old one back to be freed, so it never allocates, locks or
//! blocks. A [`Processor`] is that audio thread's whole job with no device
//! attached; [`Engine`] feeds one to a sound card, [`render`] runs one as
//! fast as it will go, and both therefore produce the same audio.
//!
//! # Effects and instruments
//!
//! Each mixer track runs what plays into it through its effects, in order,
//! and then through its fader and pan to its meter, its output and its
//! sends. A channel plays a sample or an instrument. Both kinds of
//! processor come from `windfall-dsp` and are hosted the same way: built
//! and prepared on the control side when a plan needs one that the audio
//! thread does not have, handed over with the plan, kept from plan to plan
//! for as long as the project has the effect or the channel, told of new
//! settings at the start of a buffer, and sent back to be dropped once
//! they are gone. An effect is known by its id and an instrument by its
//! channel's, so neither loses what it remembers to an edit: a reverb's
//! tail carries on through a moved fader, a new note or an effect moved to
//! another track.
//!
//! A sounding edit never clicks where the engine can help it. An effect
//! that joins a track sound is passing through fades in over 5 ms, one
//! that leaves the project fades out, and an instrument whose channel is
//! gone stops with a short fade. Two things do switch at once: effects
//! that change places within one chain, and the track an effect was on
//! when it moves to another.
//!
//! # Latency
//!
//! A limiter puts its output out late by its look-ahead, and the synth by
//! a few samples. Wherever signals meet in the mixer, the earlier ones are
//! delayed to match the latest, so every path from a note to the output
//! takes the same time: a track's sampler voices, each of its instruments,
//! and each track that feeds it through its output or a send have a delay
//! of their own. That time is [`Controller::latency_frames`]. A delay that
//! has to change, because a look-ahead was moved or an effect came or
//! went, crossfades in step with the effect that caused it, and the paths
//! stay lined up while it does. The playhead reported to the UI is taken
//! back by the same time, so it shows what is heard, and [`render`] leaves
//! that much off the front of what it returns.
//!
//! # What plays
//!
//! - Pattern mode loops the transport's pattern. Song mode plays the
//!   playlist's pattern and audio clips and stops, or loops, at the end of
//!   the last clip.
//! - An audio clip starts on the frame of its start tick and then plays at
//!   its own speed, through its mixer track. Playback that begins inside
//!   one begins at the right place in its audio. A clip that starts on the
//!   first frame of its audio plays it untouched; every other start, and
//!   every stop, is faded over 3 ms.
//! - A note starts on the first output frame at or after its tick, whatever
//!   the buffer size.
//! - Swing delays the second sixteenth-note step of each pair of steps. At
//!   full swing that step lands two thirds of the way through the pair.
//! - Stopping returns the playhead to where playback last started.
//! - A note on a sampler with an envelope or loop ends on its own tick, wherever a
//!   tempo change moves that. The attack is a straight line, and decay and
//!   release are exponential curves that reach their target exactly when
//!   their time is up.
//! - A note with no velocity is silent and takes no voice, but cuts what
//!   any other note on its channel would cut.
//! - On an instrument channel a note starts on its frame and is let go on
//!   its tick, as on a sampler with an envelope. Its velocity is the
//!   instrument's velocity. Per-note pan places the instrument voice. A key
//!   holds one note at a time, so of two overlapping
//!   notes on one key the key comes up when the one that started later
//!   ends.
//! - Stopping fades every instrument out in a few milliseconds. Seeking
//!   while playing ends the notes the pattern started and leaves the ones
//!   played by hand.
//! - In song mode an automation clip moves its target along its curve: a
//!   fader, a pan, a send, a setting of an effect or instrument, or the
//!   tempo. What a target is at any place in the song depends on that
//!   place alone, so playback can begin anywhere. The curves are read
//!   every 64 frames and followed without steps, and nothing they do is
//!   written back to the project.
//! - A song that stops, at its end or because it is told to, keeps every
//!   automated target where it was for as long as anything is still
//!   sounding, so a tail rings out under the fader and through the effect
//!   settings the song left. The targets then return to their stored
//!   values over 100 ms, or sooner when the transport is used or
//!   something is played by hand. [`render`] never lets go.
//! - A tempo curve moves the song's own clock. Notes, clip edges and the
//!   playhead are placed through a tempo map that is exact from one
//!   anchor, so a song with a tempo ramp is as long as its curve says, to
//!   the frame, whatever the buffer size.
//!
//! # Limits
//!
//! Any number of notes may start on one frame or inside one buffer, and the
//! audio is the same for every buffer size. The one hard limit is the
//! number of sampler voices: 256 sound at full level. One more steals the quietest
//! voice that is already releasing, or else the voice that started first,
//! and the stolen voice fades out over 4 ms instead of stopping dead. The
//! choice depends only on the notes, so it is the same every time.
//!
//! 128 audio clips sound at once ([`MAX_AUDIO_CLIPS`]). One more is not
//! started, and stays silent for its whole length. Clips take their slots
//! as they start, and clips that start on one tick in the order of their
//! ids, so the choice is the same every time and the same in a render.
//! How many are left out is reported: by [`Controller::frame`] while they
//! would be sounding, and by [`render_reporting`] for the whole song.
//!
//! # Mute and solo
//!
//! A muted channel or mixer track is silent, soloed or not. While any
//! channel is soloed, only soloed channels are heard. While any mixer track
//! is soloed, a track is heard only if it is soloed, feeds a soloed track
//! through its output or sends, or lies on the way from a soloed track to
//! the master. Channel solo and track solo do not affect each other.

pub mod analyzers;
mod automation;
pub mod channel_voice;
mod channel_voice_runtime;
mod clip_processing;
mod clips;
mod controller;
pub(crate) use controller::preparation as project_preparation;
mod device;
mod message;
mod metronome;
pub mod midi_hardware;
mod mixer;
mod hardware_output;
mod note_glide;
mod note_curves;
pub mod modular;
mod plan;
pub mod plugins;
mod pool;
mod processor;
mod rack;
mod ramp;
pub mod recording_clock;
pub mod recording_monitor;
pub mod recording_disk;
mod render;
pub mod sample_preprocess;
pub mod sampler_processing;
mod sequencer;
mod shared;
mod spectrum;
mod capture;
mod waveform_meter;
mod state;
mod stems;
mod tempo;
mod timeline;
pub use timeline::MAX_NAVIGATION_TRANSITIONS;
#[cfg(test)]
mod test_alloc;
mod voice;

pub use clips::MAX_AUDIO_CLIPS;
pub use controller::{
    Controller, PreparationSnapshot, PreparedProject, PreparedPublication, ProjectPreparationError,
    ProjectPublicationIntent, ProjectPublicationLease, ProjectRetirement, PublicationRefusal,
    StreamStats,
};
pub use controller::Playhead;
pub use device::Engine;
pub use pool::SamplePool;
pub use processor::Processor;
pub use render::{RenderError, RenderOptions, Rendered, TAIL_SILENCE_DB, render, render_reporting};
pub use stems::{
    Stem, StemError, StemMode, StemOptions, Streamed, render_stems, render_streaming,
    render_streaming_checked, stems,
};
pub use stems::{
    PlaylistStem, PlaylistStemError, PlaylistStemOptions, playlist_stem_project, playlist_stems,
    render_playlist_stems,
};
pub use voice::{PREVIEW_GAIN_DB, loop_crossfade_mix};
pub use capture::{CAPTURE_OBSERVATIONS, CAPTURE_SEATS, CapturedObservation};
pub use controller::capture_control::{
    CaptureCapability, CaptureRefusal, CaptureRequest, CaptureState, CaptureTicket, CaptureView,
    SeatCounters,
};

/// Lets the unit tests of code that runs on the audio thread count its
/// allocator calls.
#[cfg(test)]
#[global_allocator]
static ALLOCATOR: test_alloc::CountingAllocator = test_alloc::CountingAllocator;

/// Bounded offline sample editing; never used by an audio callback.
pub mod audio_edit;
/// Bounded native audio input capture.
pub mod recording;
/// Capture endpoints and render-endpoint loopback inputs.
pub mod recording_endpoint;
/// Bounded background history of the output and of chosen mixer tracks.
pub mod audio_logger;

#[cfg(test)]
mod qa_recording_mixer;
#[cfg(test)]
mod qa_mixer_pdc_ports;
