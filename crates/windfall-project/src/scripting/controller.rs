//! Pure controller adapter. Existing profile/link semantics remain the source
//! and action registry; scripts can map normalized controls, tempo and CC feedback.
use super::{ControllerScript, Program, RunLimits, ScriptInput};
use crate::{Command, LinkTarget, Project, SettingsPatch};

#[derive(Debug)]
pub struct ControllerScriptResult {
    pub commands: Vec<Command>,
    pub feedback: Vec<[u8; 3]>,
    pub mapped: Option<f32>,
}

pub fn controller_event(
    project: &Project,
    binding: &ControllerScript,
    value: f64,
    number: u8,
    bank: u32,
    cancelled: impl FnMut() -> bool,
) -> Result<ControllerScriptResult, String> {
    if binding
        .feedback_channel
        .is_some_and(|c| !(1..=16).contains(&c))
    {
        return Err("invalid feedback channel".into());
    }
    let asset = project
        .settings
        .scripts
        .asset(&binding.script)
        .ok_or("controller script revision is missing")?;
    if asset.kind != super::ScriptKind::Controller
        || asset.language_version != super::SCRIPT_VERSION
    {
        return Err("invalid controller script kind/version".into());
    }
    let program = Program::compile(&asset.source).map_err(|e| e.to_string())?;
    let output = program
        .run(
            ScriptInput {
                value,
                number,
                parameters: &binding.parameters,
                ..Default::default()
            },
            RunLimits {
                steps: 5000,
                notes: 0,
                messages: 16,
            },
            cancelled,
        )
        .map_err(|e| e.to_string())?;
    let mut commands: Vec<_> = output
        .tempos
        .into_iter()
        .map(|tempo| Command::UpdateSettings {
            patch: SettingsPatch {
                tempo_bpm: Some(tempo),
                ..Default::default()
            },
        })
        .collect();
    if let Some(value) = output
        .mapped
        .filter(|_| !matches!(binding.target, Some(LinkTarget::Action { .. })))
    {
        let target = match binding.target {
            Some(LinkTarget::Param { target }) => Some(target),
            Some(LinkTarget::Slot { slot }) => slot.resolve(project, bank),
            _ => None,
        }
        .ok_or("controller script mapping target is unavailable")?;
        let range = project
            .automation_range(&target)
            .ok_or("controller script mapping target is unavailable")?;
        commands.push(
            crate::controller_links::target_command(project, &target, range.value(value))
                .ok_or("controller script target has no checked command")?,
        );
    }
    let feedback = if let Some(channel) = binding.feedback_channel {
        output
            .feedback
            .into_iter()
            .map(|mut packet| {
                packet[0] |= channel - 1;
                packet
            })
            .collect()
    } else {
        Vec::new()
    };
    Ok(ControllerScriptResult {
        commands,
        feedback,
        mapped: output.mapped,
    })
}
