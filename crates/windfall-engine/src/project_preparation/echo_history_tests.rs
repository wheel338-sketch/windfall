//! Small-budget real-owner lifecycle tests: no gigabyte allocation is needed.
use super::*;
use crate::effect_host::HistoryPool;
use windfall_dsp::{EffectKind, EffectParams};
use windfall_project::{Command, Document, EffectId, PluginTarget, SampleId, SamplePath, TrackId};

const RATE: u32 = 8_000;
const HEADROOM: usize = 64 * 1024;

fn patch(kind: EffectKind) -> EffectParams {
    let mut params = kind.default_params();
    for (index, info) in kind.descriptors().iter().enumerate() {
        if info.id == "units.0.sync" {
            params.set(index, 0.0);
        } else if info.id == "units.0.timeMs" || info.id.ends_with(".delayMs") {
            params.set(index, 30.0);
        }
    }
    params
}

fn fixture(kind: EffectKind) -> (Document, SamplePool, ChannelId, EffectId) {
    let mut doc = Document::new(Project::new("shared echo history"));
    let sample = SampleId(
        doc.dispatch(
            Command::AddSample {
                name: "Original constant probe".into(),
                path: SamplePath::External("echo-history-probe.wav".into()),
            },
            None,
        )
        .unwrap()
        .created[0],
    );
    let channel = ChannelId(
        doc.dispatch(
            Command::AddChannel {
                name: None,
                sample: Some(sample),
                instrument: None,
                index: None,
                mixer_track: None,
            },
            None,
        )
        .unwrap()
        .created[0],
    );
    let effect = EffectId(
        doc.dispatch(
            Command::AddEffect {
                track: TrackId::MASTER,
                kind,
                index: None,
            },
            None,
        )
        .unwrap()
        .created[0],
    );
    doc.dispatch(
        Command::SetEffectParams {
            track: TrackId::MASTER,
            effect,
            params: patch(kind),
        },
        None,
    )
    .unwrap();
    let mut pool = SamplePool::new();
    pool.insert(
        sample,
        AudioBuffer::from_interleaved(RATE, 1, vec![0.25; RATE as usize * 2]),
    );
    (doc, pool, channel, effect)
}

fn start(
    doc: &Document,
    pool: &SamplePool,
    channel: ChannelId,
    budget: Arc<HistoryPool>,
) -> (Processor, Controller) {
    let controller = Controller::new();
    let mut plan = Plan::empty();
    plan.echo_history = budget;
    controller.lock().plan = Arc::new(plan);
    controller.set_project(doc.project(), pool);
    let processor = controller.attach(RATE);
    controller.note_on(channel, 60, 1.0);
    (processor, controller)
}

fn audio(processor: &mut Processor, frames: usize) -> Vec<f32> {
    let mut result = vec![0.0; frames * 2];
    for block in result.chunks_mut(128) {
        assert_eq!(
            crate::test_alloc::allocator_calls(|| processor.process(block)),
            0
        );
    }
    assert!(result.iter().all(|sample| sample.is_finite()));
    result
}

fn replace(doc: &mut Document, effect: EffectId, kind: EffectKind) -> EffectId {
    EffectId(
        doc.dispatch(
            Command::ReplaceEffect {
                track: TrackId::MASTER,
                effect,
                kind,
            },
            None,
        )
        .unwrap()
        .created[0],
    )
}

#[test]
fn shared_echo_retained_and_peak_refusals_preserve_installed_pcm_for_both_delay_kinds() {
    for (old, new) in [
        (EffectKind::EchoBank, EffectKind::FrequencyDelay),
        (EffectKind::FrequencyDelay, EffectKind::EchoBank),
    ] {
        let old_bytes = old.history_bytes(RATE as f32).unwrap();
        let new_bytes = new.history_bytes(RATE as f32).unwrap();
        for peak_refusal in [false, true] {
            let retained = old_bytes + new_bytes - usize::from(!peak_refusal);
            let peak = old_bytes + new_bytes + HEADROOM - usize::from(peak_refusal);
            let budget = Arc::new(HistoryPool::with_limits(retained, peak));
            let (mut doc, samples, channel, effect) = fixture(old);
            let (mut processor, controller) = start(&doc, &samples, channel, budget.clone());
            let (mut twin, _twin_controller) =
                start(&doc, &samples, channel, Arc::new(HistoryPool::default()));
            assert_eq!(audio(&mut processor, 1_000), audio(&mut twin, 1_000));
            assert_eq!(budget.occupied(), old_bytes);
            let prior = controller.preparation_snapshot();
            replace(&mut doc, effect, new);
            controller.set_project(doc.project(), &samples);
            let expected_required = old_bytes + new_bytes + if peak_refusal { HEADROOM } else { 0 };
            let expected_limit = if peak_refusal { peak } else { retained };
            assert_eq!(
                controller.project_preparation_error(),
                Some(ProjectPreparationError::MemoryLimit {
                    required: expected_required,
                    limit: expected_limit,
                })
            );
            assert_eq!(budget.occupied(), old_bytes);
            let current = controller.preparation_snapshot();
            assert!(Arc::ptr_eq(&prior.plan, &current.plan));
            let after = audio(&mut processor, 1_000);
            assert!(after.iter().any(|sample| sample.abs() > 0.001));
            assert_eq!(after, audio(&mut twin, 1_000));
        }
    }
}

#[test]
fn shared_echo_charge_survives_fade_and_retirement_capture_until_control_side_drop() {
    let bank = EffectKind::EchoBank.history_bytes(RATE as f32).unwrap();
    let bands = EffectKind::FrequencyDelay
        .history_bytes(RATE as f32)
        .unwrap();
    let budget = Arc::new(HistoryPool::with_limits(
        bank + bands,
        bank + bands + HEADROOM,
    ));
    let (mut doc, samples, channel, effect) = fixture(EffectKind::EchoBank);
    let (mut processor, controller) = start(&doc, &samples, channel, budget.clone());
    audio(&mut processor, 1_000);
    let incoming = replace(&mut doc, effect, EffectKind::FrequencyDelay);
    controller.set_project(doc.project(), &samples);
    assert_eq!(controller.project_preparation_error(), None);
    assert_eq!(budget.occupied(), bank + bands);
    // Outgoing unit is still heard and fades; its payload remains charged.
    audio(&mut processor, 1_000);
    assert_eq!(budget.occupied(), bank + bands);
    // A later publication prunes the now-finished departure into old-state garbage.
    controller.set_project(doc.project(), &samples);
    assert_eq!(controller.project_preparation_error(), None);
    audio(&mut processor, 64);
    let mut retirement = ProjectRetirement::default();
    controller.take_retired(&mut retirement);
    assert_eq!(budget.occupied(), bank + bands);
    let snapshot = controller.preparation_snapshot();
    replace(&mut doc, incoming, EffectKind::EchoBank);
    controller.set_project(doc.project(), &samples);
    assert!(matches!(
        controller.project_preparation_error(),
        Some(ProjectPreparationError::MemoryLimit { .. })
    ));
    assert!(Arc::ptr_eq(
        &snapshot.plan,
        &controller.preparation_snapshot().plan
    ));
    assert_eq!(budget.occupied(), bank + bands);
    drop(retirement);
    assert_eq!(budget.occupied(), bands);
    controller.set_project(doc.project(), &samples);
    assert_eq!(controller.project_preparation_error(), None);
    assert_eq!(budget.occupied(), bank + bands);
    audio(&mut processor, 64);
}

#[test]
fn queued_echo_candidates_and_cancelled_ready_tokens_share_the_installed_budget() {
    let bank = EffectKind::EchoBank.history_bytes(RATE as f32).unwrap();
    let bands = EffectKind::FrequencyDelay
        .history_bytes(RATE as f32)
        .unwrap();
    let budget = Arc::new(HistoryPool::with_limits(
        bank + bands,
        bank + bands + HEADROOM,
    ));
    let (mut doc, samples, channel, effect) = fixture(EffectKind::EchoBank);
    let (mut processor, controller) = start(&doc, &samples, channel, budget.clone());
    audio(&mut processor, 1_000);
    replace(&mut doc, effect, EffectKind::FrequencyDelay);
    let ready = controller
        .preparation_snapshot()
        .prepare(doc.project(), &samples, ProjectPublicationIntent::Edit)
        .unwrap();
    assert_eq!(budget.occupied(), bank + bands);
    assert!(matches!(
        controller.preparation_snapshot().prepare(
            doc.project(),
            &samples,
            ProjectPublicationIntent::Edit
        ),
        Err(ProjectPreparationError::MemoryLimit { .. })
    ));
    // Discarding a stale/cancelled candidate releases its actual prepared owner.
    drop(ready);
    assert_eq!(budget.occupied(), bank);
    controller.set_project(doc.project(), &samples);
    assert_eq!(controller.project_preparation_error(), None);
    assert_eq!(budget.occupied(), bank + bands);
    // Queue owns the candidate before the callback has seen it.
    let first = doc.project().mixer.tracks[0].effects[0].id;
    replace(&mut doc, first, EffectKind::EchoBank);
    controller.set_project(doc.project(), &samples);
    assert!(matches!(
        controller.project_preparation_error(),
        Some(ProjectPreparationError::MemoryLimit { .. })
    ));
    assert_eq!(budget.occupied(), bank + bands);
    audio(&mut processor, 64);
}

#[test]
fn echo_parameter_edits_reuse_the_owner_even_when_the_shared_budget_is_full() {
    for kind in [EffectKind::EchoBank, EffectKind::FrequencyDelay] {
        let bytes = kind.history_bytes(RATE as f32).unwrap();
        let budget = Arc::new(HistoryPool::with_limits(bytes, bytes + HEADROOM));
        let (mut doc, samples, channel, effect) = fixture(kind);
        let (mut processor, controller) = start(&doc, &samples, channel, budget.clone());
        audio(&mut processor, 512);
        let target = PluginTarget::Effect { effect };
        let before = windfall_project::PluginPreset::capture(doc.project(), target).unwrap();
        let index = kind
            .descriptors()
            .iter()
            .position(|info| info.id == "wet")
            .unwrap();
        doc.dispatch(
            Command::SetEffectParam {
                track: TrackId::MASTER,
                effect,
                param: index as u32,
                value: 0.6,
            },
            None,
        )
        .unwrap();
        controller.set_project(doc.project(), &samples);
        assert_eq!(controller.project_preparation_error(), None);
        assert_eq!(budget.occupied(), bytes);
        assert_ne!(
            before,
            windfall_project::PluginPreset::capture(doc.project(), target).unwrap()
        );
        audio(&mut processor, 512);
        assert_eq!(budget.occupied(), bytes);
    }
}
