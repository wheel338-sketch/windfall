//! The per-channel expression map as a document edit. The map itself, and
//! what it does to a voice, is `windfall_dsp::ExpressionMap`.

use crate::{Channel, CommandError, ExpressionMap};

/// The channel with `map` in place of the one it has. A map with a number
/// outside its range is refused, not corrected, so a caller that sent a bad
/// value finds out and the document keeps what it had.
pub(crate) fn applied(channel: &Channel, map: ExpressionMap) -> Result<Channel, CommandError> {
    if !map.valid() {
        return Err(CommandError::invalid(problem(&map)));
    }
    Ok(Channel {
        expression_map: map,
        ..channel.clone()
    })
}

pub(crate) fn check(channel: &Channel) -> Result<(), String> {
    if channel.expression_map.valid() {
        Ok(())
    } else {
        Err(format!(
            "channel {}: {}",
            channel.id.0,
            problem(&channel.expression_map)
        ))
    }
}

fn problem(map: &ExpressionMap) -> &'static str {
    let routes = [map.modulation_x, map.modulation_y, map.release];
    if routes.iter().any(|route| !route.valid()) {
        "an expression mapping amount must be from 0 to 1"
    } else if !(map.pitch_range_cents.is_finite()
        && (1.0..=4800.0).contains(&map.pitch_range_cents))
    {
        "the expression pitch range must be from 1 to 4800 cents"
    } else {
        "the pitch bend range must be from 1 to 96 semitones"
    }
}
