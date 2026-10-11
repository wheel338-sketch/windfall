//! The audio device, and the sounds that are not part of playback: notes
//! played by hand and files previewed from the browser.

use windfall_ipc::{AudioHost, AudioSettings, EngineStatus};
use windfall_project::ChannelId;

use super::Session;
use crate::events::Event;
use crate::paths;
use crate::sync::lock;

/// The settings to open the audio output with for a remembered request:
/// the request without a sample rate or a buffer size that its device says
/// it cannot do. Those fall back to the device's default for now, and the
/// request stays remembered as the user made it, because what does not fit
/// the device of today may fit the one plugged in tomorrow.
///
/// `devices` lists the audio devices of this machine, which can take a
/// while. It is called only for a request that names a sample rate or a
/// buffer size. A device that is not listed, or that does not say what it
/// supports, is left for the engine to judge.
pub fn openable(
    request: &AudioSettings,
    devices: impl FnOnce() -> Vec<AudioHost>,
) -> AudioSettings {
    if request.sample_rate.is_none() && request.buffer_frames.is_none() {
        return request.clone();
    }
    let hosts = devices();
    let host = hosts.iter().find(|host| match &request.host {
        Some(name) => host.name.eq_ignore_ascii_case(name),
        None => host.is_default,
    });
    let device = host.and_then(|host| {
        host.devices.iter().find(|device| match &request.device {
            Some(name) => device.name == *name,
            None => device.is_default,
        })
    });
    let Some(device) = device else {
        return request.clone();
    };

    let mut open = request.clone();
    if let Some(rate) = request.sample_rate
        && !device.sample_rates.is_empty()
        && !device.sample_rates.contains(&rate)
    {
        open.sample_rate = None;
    }
    if let Some(frames) = request.buffer_frames
        && (device.min_buffer_frames.is_some_and(|min| frames < min)
            || device.max_buffer_frames.is_some_and(|max| frames > max))
    {
        open.buffer_frames = None;
    }
    open
}

impl Session {
    pub fn engine_status(&self) -> EngineStatus {
        self.inner.audio.status()
    }

    /// Lists the audio hosts and devices of this machine. Can take a while.
    pub fn engine_devices(&self) -> Vec<AudioHost> {
        self.inner.audio.devices()
    }

    /// The audio output the user asked for, as it is remembered. `None` in
    /// a field means the default was asked for. What is open right now is
    /// in [`engine_status`](Self::engine_status).
    pub fn engine_settings(&self) -> AudioSettings {
        self.store().settings().audio.clone()
    }

    /// Opens the audio output with new settings and remembers them for the
    /// next start. Slow: a driver can take seconds to open. The returned
    /// status says how it went; a device that could not be opened is not an
    /// error of this call.
    ///
    /// What is remembered is the request exactly as given, where `None`
    /// means the device's default. What the engine reports back is never
    /// written into it. A sample rate or buffer size the device cannot do
    /// is left out when opening and stays in the request; see [`openable`].
    pub fn engine_configure(&self, settings: AudioSettings) -> EngineStatus {
        let _configuring = lock(&self.inner.configuring);
        let _recording = lock(&self.inner.recording);
        if _recording.is_some()
            || self
                .inner
                .recording_finishing
                .load(std::sync::atomic::Ordering::Acquire)
        {
            let mut status = self.engine_status();
            status.error = Some("Stop or cancel recording before changing audio settings.".into());
            return status;
        }
        // One at a time, so the request remembered last is also the one
        // opened last.
        drop(_recording);
        self.input_monitor_stop();
        self.store()
            .update(|stored| stored.audio = settings.clone());
        let open = openable(&settings, || self.inner.audio.devices());
        self.inner.audio.reconfigure(&open);
        // The device adapter attached the current ready plan off State. Cache
        // notifications queued during reopening use their own checked worker.
        {
            let state = self.state();
            self.push_project(&state); // bounded worker, never constructs under State
        }
        self.sync_transport();
        let mut told = lock(&self.inner.status);
        let status = self.inner.audio.status();
        *told = status.clone();
        self.emit(Event::EngineStatus(status.clone()));
        status
    }

    /// Tells the UI the engine status if it is not what it was last told.
    /// The realtime loop calls this about once a second, which is how a
    /// device that is unplugged or comes back gets announced.
    pub fn poll_engine_status(&self) {
        let mut told = lock(&self.inner.status);
        let status = self.inner.audio.status();
        if *told != status {
            *told = status.clone();
            self.emit(Event::EngineStatus(status));
        }
    }

    /// Plays a note on a channel right away, as from the UI keyboard.
    pub fn audition_note_on(&self, channel: ChannelId, key: u8, velocity: f32) {
        self.controller().note_on(channel, key, velocity);
        self.controller_played(channel, key, Some(velocity), std::time::Instant::now());
    }

    pub fn audition_note_off(&self, channel: ChannelId, key: u8) {
        self.controller().note_off(channel, key);
        self.controller_played(channel, key, None, std::time::Instant::now());
    }

    /// Plays an audio file once, straight to the master. Slow the first
    /// time a file is asked for, because it has to be decoded. A file that
    /// finishes decoding after a newer preview was asked for, or after
    /// [`preview_stop`](Self::preview_stop), is not played.
    pub fn preview_play(&self, path: &str) -> Result<(), String> {
        self.preview_play_checked(path, None)
    }

    pub fn browser_preview(
        &self,
        path: &str,
        token: &windfall_ipc::LibraryFileToken,
    ) -> Result<(), String> {
        self.preview_play_checked(path, Some(token))
    }

    fn preview_play_checked(
        &self,
        path: &str,
        browser: Option<&windfall_ipc::LibraryFileToken>,
    ) -> Result<(), String> {
        let file = paths::absolute(path)?;
        let ticket = {
            let mut latest = lock(&self.inner.preview);
            *latest += 1;
            *latest
        };
        // Pinning a network-drive file can be slow too. It belongs to this
        // request's original ticket, so a later preview/stop still wins.
        let pinned = browser
            .map(|t| self.inner.library.pin_file(t, path))
            .transpose()?;
        let browser = pinned.as_ref();
        let buffer = self.inner.cache.decode(&file)?;
        if let Some(token) = browser {
            self.inner.library.check_file(token, path)?;
        }
        let _library = browser.map(|t| self.inner.library.guard(t)).transpose()?;
        // Held from the check until the engine has the preview. A stop or
        // a newer preview either came before, and this one is dropped, or
        // comes after it and wins. Neither can land in between.
        let latest = lock(&self.inner.preview);
        if *latest == ticket {
            #[cfg(test)]
            self.pause("preview:send");
            self.controller().preview(buffer);
        }
        Ok(())
    }

    pub fn preview_stop(&self) {
        // Also cancels a preview that is still being decoded.
        let mut latest = lock(&self.inner.preview);
        *latest += 1;
        self.controller().stop_preview();
    }
}

#[cfg(test)]
mod tests {
    use windfall_ipc::AudioDevice;

    use super::*;

    fn device(name: &str, is_default: bool) -> AudioDevice {
        AudioDevice {
            output_channels: Vec::new(),
            name: name.to_owned(),
            is_default,
            sample_rates: vec![44_100, 48_000],
            min_buffer_frames: Some(64),
            max_buffer_frames: Some(2_048),
        }
    }

    fn hosts() -> Vec<AudioHost> {
        let wide = AudioDevice {
            sample_rates: vec![8_000, 44_100, 48_000, 96_000],
            min_buffer_frames: None,
            max_buffer_frames: None,
            ..device("Interface", false)
        };
        let silent_about_itself = AudioDevice {
            sample_rates: Vec::new(),
            ..wide.clone()
        };
        vec![
            AudioHost {
                name: "WASAPI".to_owned(),
                is_default: true,
                devices: vec![device("Speakers", true), wide],
            },
            AudioHost {
                name: "ASIO".to_owned(),
                is_default: false,
                devices: vec![AudioDevice {
                    name: "Mystery".to_owned(),
                    ..silent_about_itself
                }],
            },
        ]
    }

    fn request(
        host: Option<&str>,
        device: Option<&str>,
        sample_rate: Option<u32>,
        buffer_frames: Option<u32>,
    ) -> AudioSettings {
        AudioSettings {
            output_channels: None,
            host: host.map(str::to_owned),
            device: device.map(str::to_owned),
            sample_rate,
            buffer_frames,
        }
    }

    #[test]
    fn what_the_device_cannot_do_falls_back_to_its_default() {
        let open = |request: &AudioSettings| openable(request, hosts);

        // The default device of the default host.
        let asked = request(None, None, Some(8_000), Some(80));
        assert_eq!(open(&asked), request(None, None, None, Some(80)));
        let asked = request(None, None, Some(48_000), Some(16));
        assert_eq!(open(&asked), request(None, None, Some(48_000), None));
        let asked = request(None, None, Some(96_000), Some(4_096));
        assert_eq!(open(&asked), request(None, None, None, None));
        let asked = request(None, None, Some(44_100), Some(64));
        assert_eq!(open(&asked), asked);

        // A named device is judged by what it says of itself, and a limit
        // it does not state is not held against the request.
        let asked = request(Some("wasapi"), Some("Interface"), Some(8_000), Some(16));
        assert_eq!(open(&asked), asked);
        let asked = request(Some("WASAPI"), Some("Interface"), Some(22_050), Some(16));
        assert_eq!(
            open(&asked),
            request(Some("WASAPI"), Some("Interface"), None, Some(16))
        );
        let asked = request(Some("ASIO"), Some("Mystery"), Some(22_050), Some(7));
        assert_eq!(open(&asked), asked);
    }

    #[test]
    fn a_device_that_is_not_listed_is_left_to_the_engine() {
        for asked in [
            request(Some("JACK"), None, Some(8_000), Some(16)),
            request(None, Some("Unplugged"), Some(8_000), Some(16)),
            // The host has no default device to go by.
            request(Some("ASIO"), None, Some(8_000), Some(16)),
        ] {
            assert_eq!(openable(&asked, hosts), asked);
        }
        assert_eq!(
            openable(&request(None, None, Some(8_000), None), Vec::new),
            request(None, None, Some(8_000), None)
        );
    }

    #[test]
    fn the_devices_are_not_listed_when_there_is_nothing_to_check() {
        let asked = request(Some("WASAPI"), Some("Speakers"), None, None);
        let open = openable(&asked, || panic!("the devices were listed"));
        assert_eq!(open, asked);
    }
}
