//! Saved wrapper policy, independent of native ABI and transient window handles.
use serde::{Deserialize, Serialize};
use std::hash::Hash;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PluginProcessingMode {
    /// The desktop's existing platform default (isolated helper on Windows).
    #[default]
    Automatic,
    InProcess,
    Threaded,
    Bridged,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
#[ts(export)]
pub struct PluginWrapperOptions {
    pub smart_disable: bool,
    /// Native block size, not the device or shared-memory packet size.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub fixed_buffer: Option<u32>,
    pub processing: PluginProcessingMode,
    pub editor_scale: f32,
    pub detached: bool,
}

impl Default for PluginWrapperOptions {
    fn default() -> Self {
        Self {
            smart_disable: false,
            fixed_buffer: None,
            processing: PluginProcessingMode::Automatic,
            editor_scale: 1.0,
            detached: true,
        }
    }
}
impl PluginWrapperOptions {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        if self
            .fixed_buffer
            .is_some_and(|frames| frames == 0 || frames > 8192)
        {
            return Err("native fixed buffer must be between 1 and 8192 frames");
        }
        if !self.editor_scale.is_finite() || !(0.5..=4.0).contains(&self.editor_scale) {
            return Err("native editor scale must be between 0.5 and 4");
        }
        Ok(())
    }
}
impl std::hash::Hash for PluginWrapperOptions {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.smart_disable.hash(state);
        self.fixed_buffer.hash(state);
        self.processing.hash(state);
        self.editor_scale.to_bits().hash(state);
        self.detached.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn variable_buffer_is_absent_matching_the_optional_generated_type() {
        let options = PluginWrapperOptions {
            smart_disable: true,
            ..Default::default()
        };
        let json = serde_json::to_value(options).unwrap();
        assert!(json.get("fixedBuffer").is_none());
        assert_eq!(
            serde_json::from_value::<PluginWrapperOptions>(json).unwrap(),
            options
        );
        let old_null = serde_json::json!({ "smartDisable": true, "fixedBuffer": null });
        assert_eq!(
            serde_json::from_value::<PluginWrapperOptions>(old_null).unwrap(),
            options
        );
    }
}
