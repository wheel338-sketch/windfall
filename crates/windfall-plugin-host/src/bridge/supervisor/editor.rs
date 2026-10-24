//! Unix editor control requests; no window/object pointers leave the helper.
use super::*;
impl Control {
    pub fn set_editor_scale(&self, application: f64) -> Result<(), String> {
        crate::gui::scaling::content_scale(application, 1.0).map_err(|error| error.to_string())?;
        match self
            .call(Message::EditorScale { application }, Duration::from_secs(2))?
            .body
        {
            Message::Stopped => Ok(()),
            Message::Error { message } => Err(message),
            _ => Err("unexpected helper scale response".into()),
        }
    }
    pub fn editor_report(&self) -> Result<(bool, Option<[f64; 2]>), String> {
        match self
            .call(Message::EditorStatus, Duration::from_secs(2))?
            .body
        {
            Message::EditorReport { open, refused, .. } => Ok((open, refused)),
            Message::Error { message } => Err(message),
            _ => Err("unexpected helper editor status response".into()),
        }
    }
}
