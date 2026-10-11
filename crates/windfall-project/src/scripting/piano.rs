use super::{Program, RunLimits, ScriptInput, ScriptNote, ScriptParameter};
use crate::proposal::{CommandProposal, ProposalStamp};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct PianoScriptRequest {
    pub stamp: ProposalStamp,
    pub pattern: crate::PatternId,
    pub channel: crate::ChannelId,
    pub selected: Vec<crate::NoteId>,
    pub source: String,
    pub parameters: Vec<f64>,
    pub replace: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PianoScriptPreview {
    pub parameters: Vec<ScriptParameter>,
    pub notes: Vec<ScriptNote>,
    pub proposal: CommandProposal,
}

pub fn piano_preview(
    document: &crate::Document,
    current: ProposalStamp,
    request: &PianoScriptRequest,
    cancelled: impl FnMut() -> bool,
) -> Result<PianoScriptPreview, String> {
    if request.stamp != current || document.revision() != current.revision {
        return Err("script preview is stale".into());
    }
    if request.selected.len() > super::MAX_NOTES {
        return Err("selection memory limit exceeded".into());
    }
    let project = document.project();
    if project.channel(request.channel).is_none() {
        return Err("script channel is missing".into());
    }
    let pattern = project
        .pattern(request.pattern)
        .ok_or("script pattern is missing")?;
    let lane = pattern.lanes.iter().find(|l| l.channel == request.channel);
    let mut selected = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for id in &request.selected {
        if !seen.insert(*id) {
            return Err("duplicate selected note".into());
        }
        let note = lane
            .and_then(|l| l.notes.iter().find(|n| n.id == *id))
            .ok_or("selected note is missing")?;
        selected.push(ScriptNote::from(note));
    }
    let program = Program::compile(&request.source).map_err(|e| e.to_string())?;
    let output = program
        .run(
            ScriptInput {
                selected: &selected,
                parameters: &request.parameters,
                ..Default::default()
            },
            RunLimits::default(),
            cancelled,
        )
        .map_err(|e| e.to_string())?;
    if !output.tempos.is_empty() || !output.feedback.is_empty() || output.mapped.is_some() {
        return Err("piano scripts may only emit notes".into());
    }
    if output.notes.is_empty() && (!request.replace || selected.is_empty()) {
        return Err("script emitted no changes".into());
    }
    let mut commands = Vec::new();
    if request.replace && !request.selected.is_empty() {
        commands.push(crate::Command::RemoveNotes {
            pattern: request.pattern,
            channel: request.channel,
            notes: request.selected.clone(),
        });
    }
    if !output.notes.is_empty() {
        commands.push(crate::Command::AddNotes {
            pattern: request.pattern,
            channel: request.channel,
            notes: output
                .notes
                .iter()
                .map(|n| crate::NoteInit {
                    start: n.start,
                    length: n.length,
                    key: n.key,
                    velocity: Some(n.velocity),
                    pan: Some(n.pan),
                    expression: None,
                })
                .collect(),
        });
    }
    let proposal = CommandProposal {
        stamp: current,
        label: "Piano script".into(),
        explanation: format!(
            "{} selected notes; {} emitted notes. {}. Emitted notes receive new IDs and default expression; Undo restores the originals.",
            selected.len(),
            output.notes.len(),
            if request.replace {
                "Replace selection"
            } else {
                "Append notes"
            }
        ),
        commands,
    };
    proposal.validate(document, current, false)?;
    Ok(PianoScriptPreview {
        parameters: program.parameters,
        notes: output.notes,
        proposal,
    })
}
