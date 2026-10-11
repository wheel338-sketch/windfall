//! Provider-neutral assistant. This module has no transport, SDK or credentials.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use crate::proposal::{CommandProposal, ProposalStamp};
use crate::{ChannelId, ChannelPatch, Command, InstrumentKind, Project, SettingsPatch, TrackId};

pub const LOCAL_HELP: &str = include_str!("../../../docs/ASSISTANT-HELP.txt");

/// Immutable provider input. A future model adapter chooses what context to send
/// only after the user chooses provider/privacy policy. Never hand it a Session.
pub struct AssistantContext<'a> { pub project: &'a Project, pub stamp: ProposalStamp, pub help: &'a str }

pub trait AssistantProvider: Send + Sync {
    fn respond(&self, request: &str, context: AssistantContext<'_>) -> Result<AssistantReply, String>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AssistantReply { pub answer: String, pub proposal: Option<CommandProposal> }

pub struct LocalRuleProvider;
impl AssistantProvider for LocalRuleProvider {
    fn respond(&self, request: &str, context: AssistantContext<'_>) -> Result<AssistantReply, String> {
        if request.len() > 4096 || request.trim().is_empty() { return Err("request must contain 1..4096 bytes".into()); }
        let request = request.trim();
        let lower = request.to_ascii_lowercase();
        let mut commands = Vec::new();
        let (label, answer) = if lower.starts_with("create pattern") || lower.starts_with("add pattern") {
            let name = request.splitn(3, ' ').nth(2).filter(|name| !name.trim().is_empty()).map(str::to_owned);
            commands.push(Command::AddPattern { name });
            ("Assistant: create pattern", "Review the new pattern before applying it.")
        } else if lower.starts_with("add channel") {
            let name = request.splitn(3, ' ').nth(2).filter(|name| !name.trim().is_empty()).map(str::to_owned);
            commands.push(Command::AddChannel { name, sample: None, instrument: Some(InstrumentKind::SubtractiveSynth), index: None, mixer_track: None });
            ("Assistant: add channel", "Add a Windfall subtractive synth channel and its mixer track.")
        } else if lower.starts_with("set tempo ") || lower.starts_with("tempo ") {
            let tempo = lower.split_whitespace().last().and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite() && (crate::MIN_TEMPO_BPM..=crate::MAX_TEMPO_BPM).contains(v)).ok_or("tempo is outside project bounds")?;
            commands.push(Command::UpdateSettings { patch: SettingsPatch { tempo_bpm: Some(tempo), ..Default::default() } });
            ("Assistant: set tempo", "Review the tempo change; Undo restores the previous tempo.")
        } else if lower.starts_with("route channel ") {
            let words: Vec<_> = lower.split_whitespace().collect();
            if words.len() != 6 || words[3] != "to" || words[4] != "track" { return Err("use route channel ID to track ID".into()); }
            let channel = ChannelId(words[2].parse().map_err(|_| "invalid channel id")?);
            let track = TrackId(words[5].parse().map_err(|_| "invalid track id")?);
            if context.project.channel(channel).is_none() || context.project.mixer.track(track).is_none() { return Err("route target is missing; use the displayed IDs".into()); }
            commands.push(Command::UpdateChannel { id: channel, patch: ChannelPatch { mixer_track: Some(track), ..Default::default() } });
            ("Assistant: route channel", "Review the channel's mixer destination.")
        } else if lower.starts_with("mute channel ") || lower.starts_with("unmute channel ") {
            let channel = ChannelId(lower.split_whitespace().last().and_then(|id| id.parse().ok()).ok_or("invalid channel id")?);
            if context.project.channel(channel).is_none() { return Err("channel is missing".into()); }
            commands.push(Command::UpdateChannel { id: channel, patch: ChannelPatch { muted: Some(lower.starts_with("mute ")), ..Default::default() } });
            ("Assistant: channel mute", "Review the channel mute change.")
        } else {
            // Local help is authored with a keyword list followed by a tab and
            // an answer. No arbitrary external lookup is performed.
            let answer = context.help.lines().find_map(|line| {
                let (keywords, answer) = line.split_once('\t')?;
                keywords.split('|').any(|word| lower.contains(word)).then_some(answer)
            }).unwrap_or("Offline assistant: ask about patterns, channels, tempo, routing, scripts, recording or undo. Commands: create pattern NAME; add channel NAME; set tempo BPM; route channel ID to track ID; mute/unmute channel ID.");
            return Ok(AssistantReply { answer: answer.into(), proposal: None });
        };
        let proposal = CommandProposal { stamp: context.stamp, label: label.into(), explanation: answer.into(), commands };
        proposal.batch()?;
        Ok(AssistantReply { answer: answer.into(), proposal: Some(proposal) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn offline_intents_and_help_are_deterministic_and_do_not_edit() {
        let project = Project::new("Assistant");
        let before = project.clone();
        for prompt in ["create pattern Verse", "add channel Bass", "set tempo 120", "how do I undo?", "help with scripts"] {
            let input = || AssistantContext { project: &project, stamp: ProposalStamp { generation: 3, revision: 0 }, help: LOCAL_HELP };
            assert_eq!(LocalRuleProvider.respond(prompt, input()).unwrap(), LocalRuleProvider.respond(prompt, input()).unwrap());
        }
        assert_eq!(project, before);
        assert!(LocalRuleProvider.respond("set tempo NaN", AssistantContext { project: &project, stamp: ProposalStamp { generation: 3, revision: 0 }, help: LOCAL_HELP }).is_err());
    }
}
