//! The BYOK wire format is a deliberately small subset of the proposal registry.
//! Schema and strict input shape share one source; checked Commands remain authority.
use crate::assistant::AssistantReply;
use crate::proposal::{CommandProposal, ProposalStamp};
use serde::Deserialize;
use serde_json::{Value, json};

fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object", "properties":properties, "required":required, "additionalProperties":false})
}

/// No plugin state, external IO, arbitrary UI action or nested Batch capability.
pub fn command_schema() -> Value {
    let string = json!({"type":"string"});
    let number = json!({"type":"number"});
    let id = json!({"type":"integer", "minimum":0, "maximum":u32::MAX});
    let boolean = json!({"type":"boolean"});
    let channel_patch = object(
        json!({"name":string,"group":string,"volume":number,"pan":number,"muted":boolean,"solo":boolean,"mixerTrack":id}),
        &[],
    );
    let track_patch = object(
        json!({"name":string,"volume":number,"pan":number,"muted":boolean,"solo":boolean}),
        &[],
    );
    json!({"oneOf":[
        object(json!({"type":{"const":"addPattern"},"name":string}), &["type"]),
        object(json!({"type":{"const":"addChannel"},"name":string,"instrument":{"const":"subtractiveSynth"},"mixerTrack":id}), &["type","instrument"]),
        object(json!({"type":{"const":"updateSettings"},"patch":object(json!({"tempoBpm":number}), &["tempoBpm"])}), &["type","patch"]),
        object(json!({"type":{"const":"updatePattern"},"id":id,"patch":object(json!({"name":string}), &["name"])}), &["type","id","patch"]),
        object(json!({"type":{"const":"updateChannel"},"id":id,"patch":channel_patch}), &["type","id","patch"]),
        object(json!({"type":{"const":"updateMixerTrack"},"id":id,"patch":track_patch}), &["type","id","patch"]),
        object(json!({"type":{"const":"setTrackOutput"},"id":id,"output":id}), &["type","id","output"])
    ]})
}

/// A small validator for the exact generated schema above, not a general schema VM.
fn shape(value: &Value, schema: &Value) -> bool {
    if let Some(alternatives) = schema.get("oneOf").and_then(Value::as_array) {
        return alternatives.iter().filter(|s| shape(value, s)).count() == 1;
    }
    if let Some(expected) = schema.get("const") {
        return value == expected;
    }
    match schema.get("type").and_then(Value::as_str) {
        Some("object") => {
            let (Some(fields), Some(properties)) = (
                value.as_object(),
                schema.get("properties").and_then(Value::as_object),
            ) else {
                return false;
            };
            schema
                .get("required")
                .and_then(Value::as_array)
                .is_none_or(|required| {
                    required
                        .iter()
                        .all(|key| key.as_str().is_some_and(|key| fields.contains_key(key)))
                })
                && fields
                    .iter()
                    .all(|(key, value)| properties.get(key).is_some_and(|s| shape(value, s)))
        }
        Some("string") => value.is_string(),
        Some("boolean") => value.is_boolean(),
        Some("number") => value.as_f64().is_some_and(f64::is_finite),
        Some("integer") => value.as_u64().is_some_and(|n| n <= u64::from(u32::MAX)),
        _ => false,
    }
}

/// Parse the model's inner JSON content. Only the caller supplies the stamp.
/// Returning a proposal never applies it; native/WASM admission validates again.
pub fn parse_reply(content: &str, stamp: ProposalStamp) -> Result<AssistantReply, String> {
    if content.len() > crate::proposal::MAX_PROPOSAL_BYTES {
        return Err("assistant reply byte limit exceeded".into());
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Reply {
        answer: String,
        label: String,
        explanation: String,
        commands: Vec<Value>,
    }
    let reply: Reply =
        serde_json::from_str(content).map_err(|_| "assistant returned malformed reply JSON")?;
    if reply.answer.trim().is_empty() || reply.answer.len() > 8192 || reply.commands.len() > 128 {
        return Err("assistant reply count/text limit exceeded".into());
    }
    let schema = command_schema();
    let mut commands = Vec::new();
    for value in reply.commands {
        if !shape(&value, &schema) {
            return Err("assistant returned a command outside the advertised schema".into());
        }
        commands.push(
            serde_json::from_value(value)
                .map_err(|_| "assistant returned malformed command fields")?,
        );
    }
    let proposal = if commands.is_empty() {
        None
    } else {
        let proposal = CommandProposal {
            stamp,
            label: reply.label,
            explanation: reply.explanation,
            commands,
        };
        proposal.batch()?;
        Some(proposal)
    };
    Ok(AssistantReply {
        answer: reply.answer,
        proposal,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn schema_and_parser_bound_strict_commands_and_keep_authoritative_stamp() {
        let stamp = ProposalStamp {
            generation: 2,
            revision: 3,
        };
        let valid = r#"{"answer":"Change tempo","label":"Tempo","explanation":"Review tempo","commands":[{"type":"updateSettings","patch":{"tempoBpm":127}}]}"#;
        let reply = parse_reply(valid, stamp).unwrap();
        assert_eq!(reply.proposal.unwrap().stamp, stamp);
        for invalid in [
            valid.replace("127", "null"),
            valid.replace("tempoBpm", "name"),
            valid.replace("\"tempoBpm\":127", "\"tempoBpm\":127,\"unknown\":1"),
            valid.replace("updateSettings", "batch"),
            valid.replace("\"answer\":", "\"stamp\":{},\"answer\":"),
            "```json\n{}\n```".into(),
        ] {
            assert!(parse_reply(&invalid, stamp).is_err());
        }
    }
}
