//! The control side of a stream's analyzer captures: choosing a source,
//! running the analysis and reading its results.
//!
//! Everything here runs on a control thread. Preparing a tap allocates, and
//! is done with no lock held. The analysis itself, the FFT included, runs in
//! [`Controller::capture_poll`] on the caller's thread, never on the audio
//! thread, and holds only the captures' own lock: the controller's state is
//! looked at for a moment first and then let go.
//!
//! A selection is known by its [`CaptureTicket`]. Tickets are never reused,
//! not by another seat, another stream or another controller, so a result
//! can always be told from one that belongs to an earlier choice.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use windfall_project::EffectId;

use super::Controller;
use crate::analyzers::{
    self, AnalyzerWorker, Config, Counters, Layout, PrepareError, ProjectGeneration, PumpReport,
    Selection, SelectionGeneration, Snapshot, SnapshotReader, Source, Usage,
};
use crate::capture::{
    AudioCaptures, CAPTURE_OBSERVATIONS, CAPTURE_SEATS, CapturedObservation, SeatEndpoints,
};

// Zero is "no binding" on the audio side, so tickets start at one.
static NEXT_SELECTION: AtomicU64 = AtomicU64::new(1);
static NEXT_STREAM: AtomicU64 = AtomicU64::new(1);

#[allow(
    deprecated,
    reason = "fetch_update supports the workspace Rust 1.90 minimum"
)]
fn issue(counter: &AtomicU64) -> Option<u64> {
    counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |old| {
            old.checked_add(1)
        })
        .ok()
}

/// What to capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureRequest {
    /// The caller's count of the projects it has held, carried in every
    /// result so that one project's audio is never taken for another's.
    pub project: u64,
    pub source: Source,
    /// An effect whose own measurements come with the audio: a limiter's
    /// levels and gain reduction, a tuner's reading. It must sit on the
    /// track that is captured.
    pub effect: Option<EffectId>,
}

/// Names one accepted selection on one stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CaptureTicket {
    /// The stream it was made on. A reopened device is another stream.
    pub stream: u64,
    pub selection: u64,
    /// The rate of that stream.
    pub sample_rate: u32,
}

/// Why a selection was not accepted. The seat keeps what it had.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureRefusal {
    /// No stream is open, or the process had no tap slot left for it.
    NoStream,
    /// There is no such seat.
    Seat,
    /// The track is not in the project.
    SourceMissing,
    /// The effect is not in the project.
    EffectMissing,
    /// The effect is not on the captured track.
    EffectNotOnSource,
    /// The analyzer takes 8 to 384 kHz.
    RateUnsupported,
    /// The analyzer could not be prepared: a quota or an allocation.
    Preparation(PrepareError),
    /// The audio thread has not yet taken the selection made before this
    /// one. Try again after a device buffer.
    Busy,
    /// The ticket counter is used up. Nothing more can be selected.
    Exhausted,
}

impl std::fmt::Display for CaptureRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Analyzer capture refused: {self:?}")
    }
}
impl std::error::Error for CaptureRefusal {}

/// What a stream can capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureCapability {
    pub seats: usize,
    /// The open stream and its rate, if it has captures.
    pub stream: Option<(u64, u32)>,
    pub config: Config,
    pub layout: Layout,
    /// What the whole process has prepared right now.
    pub usage: Usage,
}

/// Where a selection stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureState {
    /// No stream is open.
    NoStream,
    /// The ticket is not this seat's selection: it was replaced, cleared, or
    /// made on an earlier stream.
    Stale,
    /// The captured track has left the project.
    SourceMissing,
    /// Selected, and no full analysis window has arrived yet.
    AwaitingAudio,
    /// A result is in [`CaptureView::snapshot`].
    Ready,
}

/// What the audio thread counted for a seat since its stream opened.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SeatCounters {
    pub dropped_observations: u64,
    pub missing_source_blocks: u64,
    pub latency_refusals: u64,
    pub refused_blocks: u64,
    pub deferred_installs: u64,
}

/// One look at a capture. Borrowed: copy out what is wanted.
pub struct CaptureView<'a> {
    pub state: CaptureState,
    /// The newest analysis of this exact selection. The same one is shown
    /// again until a newer one arrives: compare its frame range.
    pub snapshot: Option<&'a Snapshot>,
    /// What the bound effect measured since the last look, oldest first, at
    /// most [`CAPTURE_OBSERVATIONS`].
    pub observations: &'a [CapturedObservation],
    /// False once the bound effect has left the project or its track.
    pub effect_present: bool,
    pub counters: Counters,
    pub seat: SeatCounters,
    /// What this look's run of the analysis consumed.
    pub pump: PumpReport,
}

struct Selected {
    ticket: CaptureTicket,
    request: CaptureRequest,
    selection: Selection,
    worker: AnalyzerWorker,
    reader: SnapshotReader,
}

struct ControlSeat {
    endpoints: SeatEndpoints,
    selected: Option<Selected>,
    /// Reused for every look, so collecting observations allocates nothing.
    observations: Vec<CapturedObservation>,
}

/// The control side of one stream's captures. It lives with the stream's
/// link and is dropped with it, off the audio thread.
pub(crate) struct Captures {
    stream: u64,
    sample_rate: u32,
    seats: [ControlSeat; CAPTURE_SEATS],
}

pub(crate) type SharedCaptures = Arc<Mutex<Captures>>;

/// Prepares the captures of a new stream. Allocates; call with no lock held.
/// `None` when the process has no tap slots left, which leaves the stream
/// without captures and otherwise as it was.
pub(crate) fn attach(sample_rate: u32) -> Option<(AudioCaptures, SharedCaptures)> {
    let stream = issue(&NEXT_STREAM)?;
    let (audio, endpoints) = crate::capture::prepare().ok()?;
    let seats = endpoints.map(|endpoints| ControlSeat {
        endpoints,
        selected: None,
        observations: Vec::with_capacity(CAPTURE_OBSERVATIONS),
    });
    Some((
        audio,
        Arc::new(Mutex::new(Captures {
            stream,
            sample_rate,
            seats,
        })),
    ))
}

fn guard(captures: &SharedCaptures) -> MutexGuard<'_, Captures> {
    captures
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The captures of the stream a link belongs to, unless its processor is
/// gone: a stream with nobody at the other end captures nothing.
fn of(link: Option<&super::Link>) -> Option<SharedCaptures> {
    link.filter(|link| !link.messages.is_abandoned())
        .and_then(|link| link.captures.clone())
}

impl Controller {
    fn captures(&self) -> Option<SharedCaptures> {
        of(self.lock().link.as_ref())
    }

    /// What the open stream can capture, and what the process has in use.
    pub fn capture_capability(&self) -> CaptureCapability {
        let config = Config::default();
        let stream = self.captures().map(|captures| {
            let captures = guard(&captures);
            (captures.stream, captures.sample_rate)
        });
        CaptureCapability {
            seats: CAPTURE_SEATS,
            stream,
            config,
            layout: config.layout().expect("the default analyzer layout"),
            usage: analyzers::usage(),
        }
    }

    /// Points a seat at a source. The analyzer is prepared here, on the
    /// caller's thread, with no lock held; the audio thread takes it over at
    /// its next buffer. Until a result with the returned ticket arrives the
    /// selection is waiting for audio. A refusal leaves the seat as it was.
    pub fn capture_select(
        &self,
        seat: usize,
        request: CaptureRequest,
    ) -> Result<CaptureTicket, CaptureRefusal> {
        if seat >= CAPTURE_SEATS {
            return Err(CaptureRefusal::Seat);
        }
        let captures = {
            let state = self.lock();
            let plan = &state.plan;
            if let Source::TrackPostFader(track) = request.source
                && plan.track_ids.get(track.0).is_none()
            {
                return Err(CaptureRefusal::SourceMissing);
            }
            if let Some(effect) = request.effect {
                let Some(place) = plan.effect_ids.get(effect.0) else {
                    return Err(CaptureRefusal::EffectMissing);
                };
                let on = plan.tracks[plan.effect_places[place].0].id;
                if request.source != Source::TrackPostFader(on) {
                    return Err(CaptureRefusal::EffectNotOnSource);
                }
            }
            of(state.link.as_ref()).ok_or(CaptureRefusal::NoStream)?
        };
        let (stream, sample_rate) = {
            let captures = guard(&captures);
            (captures.stream, captures.sample_rate)
        };
        if !(8_000..=384_000).contains(&sample_rate) {
            return Err(CaptureRefusal::RateUnsupported);
        }
        let ticket = CaptureTicket {
            stream,
            selection: issue(&NEXT_SELECTION).ok_or(CaptureRefusal::Exhausted)?,
            sample_rate,
        };
        let selection = Selection {
            project: ProjectGeneration(request.project),
            generation: SelectionGeneration(ticket.selection),
            source: request.source,
        };
        // The one slow step, and nothing is locked while it runs.
        let (tap, worker, reader) = analyzers::prepare(selection, Config::default())
            .map_err(CaptureRefusal::Preparation)?;

        // Whatever is displaced or turned away is dropped after the lock.
        let displaced;
        let refused;
        {
            let mut captures = guard(&captures);
            let seat = &mut captures.seats[seat];
            seat.endpoints.installer.collect_retired();
            match seat.endpoints.installer.stage(tap) {
                Ok(()) => {
                    seat.endpoints.shared.bind(ticket.selection, request.effect);
                    displaced = seat.selected.replace(Selected {
                        ticket,
                        request,
                        selection,
                        worker,
                        reader,
                    });
                    refused = None;
                }
                Err(tap) => {
                    displaced = None;
                    refused = Some((tap, worker, reader));
                }
            }
        }
        drop(displaced);
        match refused {
            Some(candidate) => {
                drop(candidate);
                Err(CaptureRefusal::Busy)
            }
            None => Ok(ticket),
        }
    }

    /// Stops a seat's capture. False if the audio thread has not yet taken
    /// the selection made before, in which case nothing changed.
    pub fn capture_clear(&self, seat: usize) -> bool {
        let Some(captures) = self.captures() else {
            return true;
        };
        let displaced;
        {
            let mut captures = guard(&captures);
            let Some(seat) = captures.seats.get_mut(seat) else {
                return true;
            };
            seat.endpoints.installer.collect_retired();
            if seat.selected.is_none() {
                return true;
            }
            if !seat.endpoints.installer.clear() {
                return false;
            }
            seat.endpoints.shared.bind(0, None);
            displaced = seat.selected.take();
        }
        drop(displaced);
        true
    }

    /// Runs the analysis of what has arrived for a selection and shows the
    /// result to `visit`. Call it regularly, 60 times a second or so: the
    /// audio thread's queue holds 32 blocks, and audio that finds it full
    /// is dropped, counted, and breaks the analysis until a fresh window
    /// has arrived.
    pub fn capture_poll<R>(
        &self,
        seat: usize,
        ticket: CaptureTicket,
        visit: impl FnOnce(CaptureView<'_>) -> R,
    ) -> R {
        let idle = |state| CaptureView {
            state,
            snapshot: None,
            observations: &[],
            effect_present: false,
            counters: Counters::default(),
            seat: SeatCounters::default(),
            pump: PumpReport::default(),
        };
        if seat >= CAPTURE_SEATS {
            return visit(idle(CaptureState::Stale));
        }
        // The plan is looked at under the controller's lock, and that lock
        // is released before any analysis runs.
        let state = self.lock();
        let Some(captures) = of(state.link.as_ref()) else {
            drop(state);
            return visit(idle(CaptureState::NoStream));
        };
        let mut captures = guard(&captures);
        let stream = captures.stream;
        let seat = &mut captures.seats[seat];
        let Some(selected) = seat
            .selected
            .as_mut()
            .filter(|selected| selected.ticket == ticket && stream == ticket.stream)
        else {
            drop(state);
            return visit(idle(CaptureState::Stale));
        };
        let plan = &state.plan;
        let source_present = match selected.request.source {
            Source::MixOutput => true,
            Source::TrackPostFader(track) => plan.track_ids.get(track.0).is_some(),
        };
        let effect_present = selected.request.effect.is_none_or(|effect| {
            plan.effect_ids.get(effect.0).is_some_and(|place| {
                Source::TrackPostFader(plan.tracks[plan.effect_places[place].0].id)
                    == selected.request.source
            })
        });
        drop(state);

        seat.endpoints.installer.collect_retired();
        seat.observations.clear();
        while seat.observations.len() < CAPTURE_OBSERVATIONS {
            let Ok(observation) = seat.endpoints.observations.pop() else {
                break;
            };
            if observation.selection == ticket.selection {
                seat.observations.push(observation);
            }
        }
        let shared = &seat.endpoints.shared;
        let counted = SeatCounters {
            dropped_observations: shared.dropped_observations.load(Ordering::Relaxed),
            missing_source_blocks: shared.missing_source_blocks.load(Ordering::Relaxed),
            latency_refusals: shared.latency_refusals.load(Ordering::Relaxed),
            refused_blocks: shared.refused_blocks.load(Ordering::Relaxed),
            deferred_installs: seat.endpoints.installer.deferred_installs(),
        };
        // Free a snapshot the reader still holds before the analysis wants
        // one, then read what the analysis produced.
        selected.reader.poll(selected.selection);
        let pump = selected.worker.pump();
        let counters = selected.reader.counters();
        let snapshot = selected.reader.poll(selected.selection);
        let state = if !source_present {
            CaptureState::SourceMissing
        } else if snapshot.is_some() {
            CaptureState::Ready
        } else {
            CaptureState::AwaitingAudio
        };
        visit(CaptureView {
            state,
            snapshot: if source_present { snapshot } else { None },
            observations: &seat.observations,
            effect_present,
            counters,
            seat: counted,
            pump,
        })
    }
}
