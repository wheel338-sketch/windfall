//! A writer of FL Studio project files, for tests.
//!
//! It turns an [`FlpProject`] into the bytes of a file, event by event, in
//! the order files saved by FL Studio have them. Every fixture the tests
//! read is made here, from values the tests choose: no file that FL Studio
//! or anyone else wrote is part of this repository.
//!
//! The writer follows the version the project names: text is UTF-16 from
//! 11.5 on, the tempo is one event from 10 on, the mixer's flags and slot
//! numbers move at 12, and the playlist's tracks are counted from 499 from
//! 12.9.1 on. A project without a version is written the modern way.

#![allow(dead_code)]

use windfall_flp::{
    Arrangement, Channel, ChannelParams, ControlTarget, ControlValue, EnvelopeLfo, FileFormat,
    FlVersion, FlpProject, Header, Insert, InsertParam, Pattern, PlaylistItem, PlaylistSource,
    Plugin, RawValue, SlotParam, TimeMarker,
};

/// The version fixtures are written as unless a test picks another.
pub const MODERN: &str = "20.8.4.2576";

/// An empty project of the given version and time base, ready to be
/// filled in.
pub fn project(version: &str, ppq: u16) -> FlpProject {
    FlpProject {
        header: Some(Header {
            format: FileFormat::Project,
            channel_count: 0,
            ppq,
        }),
        version_text: Some(version.to_owned()),
        ..FlpProject::default()
    }
}

/// Builds the event stream of a file one event at a time.
#[derive(Default)]
pub struct Events {
    pub bytes: Vec<u8>,
    utf16: bool,
}

impl Events {
    pub fn new(utf16: bool) -> Self {
        Self {
            bytes: Vec::new(),
            utf16,
        }
    }

    pub fn byte(&mut self, id: u8, value: u8) {
        assert!(id < 64);
        self.bytes.extend([id, value]);
    }

    pub fn word(&mut self, id: u8, value: u16) {
        assert!((64..128).contains(&id));
        self.bytes.push(id);
        self.bytes.extend(value.to_le_bytes());
    }

    pub fn dword(&mut self, id: u8, value: u32) {
        assert!((128..192).contains(&id));
        self.bytes.push(id);
        self.bytes.extend(value.to_le_bytes());
    }

    pub fn data(&mut self, id: u8, data: &[u8]) {
        assert!(id >= 192);
        self.bytes.push(id);
        let mut length = data.len();
        loop {
            let low = (length & 0x7F) as u8;
            length >>= 7;
            if length == 0 {
                self.bytes.push(low);
                break;
            }
            self.bytes.push(low | 0x80);
        }
        self.bytes.extend(data);
    }

    pub fn text(&mut self, id: u8, text: &str) {
        let data: Vec<u8> = if self.utf16 {
            text.encode_utf16()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect()
        } else {
            text.bytes().chain([0]).collect()
        };
        self.data(id, &data);
    }

    pub fn raw(&mut self, id: u8, value: &RawValue) {
        match value {
            RawValue::Byte(value) => self.byte(id, *value),
            RawValue::Word(value) => self.word(id, *value),
            RawValue::DWord(value) => self.dword(id, *value),
            RawValue::Data(data) => self.data(id, data),
        }
    }
}

/// Wraps an event stream in the two chunks of a file.
pub fn file(header: Header, events: &[u8]) -> Vec<u8> {
    let mut bytes = b"FLhd".to_vec();
    bytes.extend(6_u32.to_le_bytes());
    bytes.extend(header.format.raw().to_le_bytes());
    bytes.extend(header.channel_count.to_le_bytes());
    bytes.extend(header.ppq.to_le_bytes());
    bytes.extend(b"FLdt");
    bytes.extend((events.len() as u32).to_le_bytes());
    bytes.extend(events);
    bytes
}

/// How a version lays a file out.
#[derive(Clone, Copy)]
struct Layout {
    utf16: bool,
    /// The tempo is event 156, not events 93 and 66.
    one_tempo_event: bool,
    /// Inserts keep their flags in event 236 and slots end with event 98.
    modern_mixer: bool,
    /// Arrangements are announced, and tracks are counted from 499.
    tall: bool,
    /// Playlist items are 60 bytes.
    long_items: bool,
    /// Notes are 20 bytes.
    short_notes: bool,
}

impl Layout {
    fn of(project: &FlpProject) -> Self {
        let version = project.version().unwrap_or(FlVersion::new(20, 8, 4));
        Self {
            utf16: (version.major, version.minor) >= (11, 5),
            one_tempo_event: version.major >= 10,
            modern_mixer: version.major >= 12,
            tall: version >= FlVersion::new(12, 9, 1),
            long_items: (version.major, version.minor) >= (20, 99),
            short_notes: version.major < 9,
        }
    }
}

/// The bytes of the file that says what `project` says.
pub fn write(project: &FlpProject) -> Vec<u8> {
    let layout = Layout::of(project);
    let mut events = Events::new(layout.utf16);
    if let Some(version) = &project.version_text {
        let mut text = version.clone().into_bytes();
        text.push(0);
        events.data(199, &text);
    }
    settings(&mut events, project, layout);
    for group in &project.channel_groups {
        events.text(231, group);
    }
    if !project.initial_controls.is_empty() {
        events.data(216, &control_records(&project.initial_controls));
    }
    for raw in &project.uninterpreted {
        events.raw(raw.id, &raw.value);
    }
    for pattern in &project.patterns {
        pattern_content(&mut events, pattern, layout);
    }
    for link in &project.remote_controllers {
        let mut record = vec![0_u8; 20];
        record[2..6].copy_from_slice(&link.source.to_le_bytes());
        record[8..12].copy_from_slice(&link.location.to_le_bytes());
        record[12..16].copy_from_slice(&link.flags.to_le_bytes());
        record[16..20].copy_from_slice(&link.smoothing.to_le_bytes());
        events.data(227, &record);
    }
    for channel in &project.channels {
        write_channel(&mut events, project, channel);
    }
    for pattern in &project.patterns {
        pattern_details(&mut events, pattern);
    }
    for arrangement in &project.arrangements {
        write_arrangement(&mut events, arrangement, layout);
    }
    if let Some(current) = project.current_arrangement {
        events.word(100, current);
    }
    mixer(&mut events, project, layout);
    let header = project.header.unwrap_or(Header {
        format: FileFormat::Project,
        channel_count: project.channels.len() as u16,
        ppq: 96,
    });
    file(header, &events.bytes)
}

fn settings(events: &mut Events, project: &FlpProject, layout: Layout) {
    let settings = &project.settings;
    if let Some(tempo) = settings.tempo_millibpm {
        if layout.one_tempo_event {
            events.dword(156, tempo);
        } else {
            events.word(93, (tempo % 1000) as u16);
            events.word(66, (tempo / 1000) as u16);
        }
    }
    if let Some(pattern) = settings.current_pattern {
        events.word(67, pattern);
    }
    if let Some(on) = settings.loop_active {
        events.byte(9, u8::from(on));
    }
    if let Some(swing) = settings.swing {
        events.byte(11, swing);
    }
    if let Some(pitch) = settings.main_pitch {
        events.word(80, pitch as u16);
    }
    if let Some(numerator) = settings.numerator {
        events.byte(17, numerator);
    }
    if let Some(denominator) = settings.denominator {
        events.byte(18, denominator);
    }
    if let Some(law) = settings.pan_law {
        events.byte(23, law);
    }
    if let Some(on) = settings.play_truncated_notes {
        events.byte(30, u8::from(on));
    }
    if let Some(volume) = settings.main_volume {
        events.byte(12, volume);
    }
    let texts = [
        (194, &settings.title),
        (206, &settings.genre),
        (207, &settings.author),
        (202, &settings.data_path),
        (197, &settings.url),
    ];
    for (id, text) in texts {
        if let Some(text) = text {
            events.text(id, text);
        }
    }
    if let Some(comments) = &settings.comments {
        let id = if settings.comments_are_rtf { 198 } else { 195 };
        events.text(id, comments);
    }
}

pub fn control_records(controls: &[ControlValue]) -> Vec<u8> {
    let mut data = Vec::new();
    for control in controls {
        data.extend([0_u8; 4]);
        data.extend(control.location.to_le_bytes());
        data.extend(control.value.to_le_bytes());
    }
    data
}

/// The notes and recorded control changes of a pattern, which files put
/// before the channels.
fn pattern_content(events: &mut Events, pattern: &Pattern, layout: Layout) {
    if pattern.notes.is_empty() && pattern.control_events.is_empty() {
        return;
    }
    events.word(65, pattern.iid);
    if !pattern.control_events.is_empty() {
        let mut data = Vec::new();
        for change in &pattern.control_events {
            data.extend(change.position.to_le_bytes());
            data.extend(change.location.to_le_bytes());
            data.extend(change.value.to_le_bytes());
        }
        events.data(223, &data);
    }
    if !pattern.notes.is_empty() {
        let mut data = Vec::new();
        for note in &pattern.notes {
            data.extend(note.position.to_le_bytes());
            data.extend(note.flags.to_le_bytes());
            data.extend(note.channel.to_le_bytes());
            data.extend(note.length.to_le_bytes());
            if layout.short_notes {
                data.extend([
                    note.key as u8,
                    note.fine_pitch,
                    0,
                    note.midi_channel,
                    note.pan,
                    note.velocity,
                    note.mod_x,
                    note.mod_y,
                ]);
            } else {
                data.extend(note.key.to_le_bytes());
                data.extend(note.group.to_le_bytes());
                data.extend([
                    note.fine_pitch,
                    0,
                    note.release,
                    note.midi_channel,
                    note.pan,
                    note.velocity,
                    note.mod_x,
                    note.mod_y,
                ]);
            }
        }
        events.data(224, &data);
    }
}

/// The name, colour, length and markers of a pattern, which files put
/// after the first channel.
fn pattern_details(events: &mut Events, pattern: &Pattern) {
    events.word(65, pattern.iid);
    if let Some(name) = &pattern.name {
        events.text(193, name);
    }
    if let Some(color) = pattern.color {
        events.dword(150, file_color(color));
    }
    if let Some(length) = pattern.length {
        events.dword(164, length);
    }
    markers(events, &pattern.markers);
}

fn markers(events: &mut Events, markers: &[TimeMarker]) {
    for marker in markers {
        events.dword(148, (u32::from(marker.kind) << 24) | marker.position);
        if let Some(numerator) = marker.numerator {
            events.byte(33, numerator);
        }
        if let Some(denominator) = marker.denominator {
            events.byte(34, denominator);
        }
        if let Some(name) = &marker.name {
            events.text(205, name);
        }
    }
}

/// 0xRRGGBB as a file stores a colour: red in the lowest byte.
pub fn file_color(color: u32) -> u32 {
    let [_, red, green, blue] = color.to_be_bytes();
    u32::from_le_bytes([red, green, blue, 0])
}

fn plugin_events(events: &mut Events, plugin: &Plugin, insert: u32, slot: u32) {
    events.text(201, &plugin.internal_name);
    if let Some(generator) = plugin.generator {
        let mut wrapper = vec![0_u8; 52];
        wrapper[0..4].copy_from_slice(&insert.to_le_bytes());
        wrapper[4..8].copy_from_slice(&slot.to_le_bytes());
        wrapper[16..20].copy_from_slice(&(u32::from(generator) << 4 | 0x40).to_le_bytes());
        events.data(212, &wrapper);
    }
}

fn write_channel(events: &mut Events, project: &FlpProject, channel: &Channel) {
    events.word(64, channel.iid);
    events.byte(21, channel.kind.raw());
    if let Some(plugin) = &channel.plugin {
        plugin_events(events, plugin, 0, u32::MAX);
    }
    if let Some(name) = &channel.name {
        events.text(203, name);
    }
    if let Some(color) = channel.color {
        events.dword(128, file_color(color));
    }
    if let Some(plugin) = &channel.plugin
        && !plugin.state.is_empty()
    {
        events.data(213, &plugin.state);
    }
    if let Some(enabled) = channel.enabled {
        events.byte(0, u8::from(enabled));
    }
    if let Some(flags) = channel.fx_flags {
        events.word(70, flags);
    }
    if let Some(swing) = channel.swing {
        events.word(97, swing);
    }
    if let Some(shift) = channel.time_shift {
        events.word(89, shift);
    }
    if let Some(preamp) = channel.preamp {
        events.word(74, preamp);
    }
    if let Some(insert) = channel.insert {
        events.byte(22, insert as u8);
    }
    for &child in &channel.layer_children {
        events.word(94, child);
    }
    if let (Some(pan), Some(volume), Some(pitch)) = (channel.pan, channel.volume, channel.pitch) {
        let mut levels = Vec::new();
        levels.extend(pan.to_le_bytes());
        levels.extend(volume.to_le_bytes());
        levels.extend(pitch.to_le_bytes());
        levels.extend([0_u8; 12]);
        events.data(219, &levels);
    }
    if let Some(polyphony) = channel.polyphony {
        let mut data = Vec::new();
        data.extend(polyphony.max.to_le_bytes());
        data.extend(polyphony.slide.to_le_bytes());
        data.push(polyphony.flags);
        events.data(221, &data);
    }
    if let Some(params) = &channel.params {
        events.data(215, &channel_params(params));
    }
    if let Some(cut) = channel.cut {
        events.dword(132, u32::from(cut.cuts) | (u32::from(cut.cut_by) << 16));
    }
    if let Some(root) = channel.root_note {
        events.dword(135, root);
    }
    if let Some(group) = channel.group {
        events.dword(145, group as u32);
    }
    if let Some(curve) = &channel.automation {
        let mut data = vec![0_u8; 21];
        data[0] = 1;
        data[4] = 64;
        data[17..21].copy_from_slice(&(curve.points.len() as u32).to_le_bytes());
        for point in &curve.points {
            data.extend(point.offset.to_le_bytes());
            data.extend(point.value.to_le_bytes());
            data.extend(point.tension.to_le_bytes());
            data.extend(point.mode.to_le_bytes());
            data.extend([0_u8; 2]);
        }
        // FL Studio writes more after the points, which nothing reads.
        data.extend([0_u8; 16]);
        events.data(234, &data);
    }
    for envelope in &channel.envelopes {
        events.data(218, &envelope_record(envelope));
    }
    if let Some(flags) = channel.sampler_flags {
        events.dword(143, flags);
    }
    if let Some(path) = &channel.sample_path {
        events.text(196, path);
    }
    // The oldest files list a channel's steps under each pattern in turn.
    for pattern in &project.patterns {
        let steps = pattern
            .legacy_steps
            .iter()
            .filter(|step| step.channel == channel.iid);
        for (index, step) in steps.enumerate() {
            if index == 0 {
                events.word(65, pattern.iid);
            }
            events.word(91, step.raw);
        }
    }
}

/// Event 215. Which fields are written follows from which are set: the
/// 64-bit ones make it the 168 bytes of FL Studio 20, and without them it
/// is the 112 bytes of FL Studio 11.
fn channel_params(params: &ChannelParams) -> Vec<u8> {
    let long = params.sample_start.is_some() || params.sample_length.is_some();
    let mut data = vec![0_u8; if long { 168 } else { 112 }];
    data[0..4].copy_from_slice(&u32::MAX.to_le_bytes());
    let mut put = |at: usize, value: u32| data[at..at + 4].copy_from_slice(&value.to_le_bytes());
    put(40, params.arp_direction.unwrap_or(0));
    put(44, params.arp_range.unwrap_or(0));
    put(48, params.arp_chord.unwrap_or(0));
    put(52, params.arp_time.unwrap_or(0));
    put(56, params.arp_gate.unwrap_or(0));
    put(92, params.arp_repeat.unwrap_or(0));
    put(96, params.stretch_time.unwrap_or(0));
    put(100, params.stretch_pitch.unwrap_or(0) as u32);
    put(104, params.stretch_multiplier.unwrap_or(0) as u32);
    put(108, params.stretch_mode.unwrap_or(0) as u32);
    if long {
        data[128..136].copy_from_slice(&params.sample_start.unwrap_or(0.0).to_le_bytes());
        data[136..144].copy_from_slice(&params.sample_length.unwrap_or(1.0).to_le_bytes());
    }
    data
}

fn envelope_record(envelope: &EnvelopeLfo) -> Vec<u8> {
    let fields = [
        envelope.flags,
        u32::from(envelope.enabled),
        envelope.predelay,
        envelope.attack,
        envelope.hold,
        envelope.decay,
        envelope.sustain,
        envelope.release,
        envelope.amount as u32,
    ];
    let mut data: Vec<u8> = fields.into_iter().flat_map(u32::to_le_bytes).collect();
    let tail = [
        envelope.lfo_predelay,
        envelope.lfo_attack,
        envelope.lfo_amount.map(|v| v as u32),
        envelope.lfo_speed,
        envelope.lfo_shape.map(|v| v as u32),
        envelope.attack_tension.map(|v| v as u32),
        envelope.decay_tension.map(|v| v as u32),
        envelope.release_tension.map(|v| v as u32),
    ];
    if let Some(last) = tail.iter().rposition(Option::is_some) {
        for value in &tail[..=last] {
            data.extend(value.unwrap_or(0).to_le_bytes());
        }
    }
    data
}

fn write_arrangement(events: &mut Events, arrangement: &Arrangement, layout: Layout) {
    if layout.tall {
        events.word(99, arrangement.index);
        if let Some(name) = &arrangement.name {
            events.text(241, name);
        }
    }
    let mut data = Vec::new();
    for item in &arrangement.items {
        data.extend(playlist_item(item, layout));
    }
    events.data(233, &data);
    for item in &arrangement.legacy_items {
        events.dword(129, u32::from(item.bar) | (u32::from(item.pattern) << 16));
    }
    markers(events, &arrangement.markers);
    for track in &arrangement.tracks {
        let mut data = Vec::new();
        data.extend(track.iid.to_le_bytes());
        data.extend(file_color(track.color.unwrap_or(0)).to_le_bytes());
        data.extend(0_u32.to_le_bytes());
        data.push(u8::from(track.enabled));
        data.extend(track.height.to_le_bytes());
        data.extend([0_u8; 49]);
        events.data(238, &data);
        if let Some(name) = &track.name {
            events.text(239, name);
        }
    }
}

fn playlist_item(item: &PlaylistItem, layout: Layout) -> Vec<u8> {
    const BASE: u16 = 0x5000;
    let top: u16 = if layout.tall { 499 } else { 198 };
    let block = !layout.tall && item.track > top;
    let (base, index, start, end) = match item.source {
        PlaylistSource::Pattern { .. } if block => (0, 0x6000, u32::MAX, u32::MAX),
        PlaylistSource::Pattern {
            pattern,
            start,
            end,
        } => (
            BASE,
            BASE + pattern,
            start.unwrap_or(u32::MAX),
            end.unwrap_or(u32::MAX),
        ),
        PlaylistSource::Channel {
            channel,
            start,
            end,
        } => (
            BASE,
            channel,
            start.unwrap_or(-1.0).to_bits(),
            end.unwrap_or(-1.0).to_bits(),
        ),
    };
    let row = match item.source {
        PlaylistSource::Pattern { pattern, .. } if block => 999 - pattern,
        _ => top - item.track,
    };
    let mut data = Vec::new();
    data.extend(item.position.to_le_bytes());
    data.extend(base.to_le_bytes());
    data.extend(index.to_le_bytes());
    data.extend(item.length.to_le_bytes());
    data.extend(row.to_le_bytes());
    data.extend(item.group.to_le_bytes());
    data.extend([0x78, 0]);
    data.extend(item.flags.to_le_bytes());
    data.extend([0x40, 0x64, 0x80, 0x80]);
    data.extend(start.to_le_bytes());
    data.extend(end.to_le_bytes());
    if layout.long_items {
        let extra = item.extra.expect("a file this new has the extra fields");
        data.extend(extra.id.to_le_bytes());
        data.extend(extra.fade_in.to_le_bytes());
        data.extend(extra.fade_in_tension.to_le_bytes());
        data.extend(extra.fade_out.to_le_bytes());
        data.extend(extra.fade_out_tension.to_le_bytes());
        data.extend(extra.gain.to_le_bytes());
        data.extend(extra.fade_flags.to_le_bytes());
    }
    data
}

fn mixer(events: &mut Events, project: &FlpProject, layout: Layout) {
    let mixer = &project.mixer;
    if let Some(on) = mixer.delay_compensation {
        events.byte(29, u8::from(on));
    }
    let mut controls = mixer.other_controls.clone();
    for (index, insert) in mixer.inserts.iter().enumerate() {
        write_insert(events, insert, index as u32, layout);
        controls.extend(insert_controls(insert, index as u16));
    }
    if !controls.is_empty() {
        events.data(225, &control_records(&controls));
    }
}

fn write_insert(events: &mut Events, insert: &Insert, index: u32, layout: Layout) {
    if let Some(color) = insert.color {
        events.dword(149, file_color(color));
    }
    if let Some(name) = &insert.name {
        events.text(204, name);
    }
    match (insert.flags, layout.modern_mixer) {
        (Some(flags), true) => {
            let mut data = vec![0_u8; 12];
            data[4..8].copy_from_slice(&flags.to_le_bytes());
            events.data(236, &data);
        }
        (flags, _) => {
            if let Some(flags) = flags {
                events.byte(27, flags as u8);
            }
            events.data(236, &[0, 0, 0, 0, 1]);
        }
    }
    for slot in &insert.slots {
        // A slot's wrapper carries its number, which is all that files
        // from before event 98 have.
        let plugin = Plugin {
            generator: Some(slot.plugin.generator.unwrap_or(false)),
            ..slot.plugin.clone()
        };
        plugin_events(events, &plugin, index, u32::from(slot.index));
        if let Some(name) = &slot.name {
            events.text(203, name);
        }
        if let Some(color) = slot.color {
            events.dword(128, file_color(color));
        }
        events.data(213, &slot.plugin.state);
        if layout.modern_mixer {
            events.word(98, u16::from(slot.index));
        }
    }
    let mut routing = vec![0_u8; if layout.modern_mixer { 127 } else { 105 }];
    for route in &insert.routes {
        routing[usize::from(route.target)] = 1;
    }
    events.data(235, &routing);
    if let Some(input) = insert.input {
        events.dword(154, input as u32);
    }
    events.dword(
        147,
        insert
            .output
            .expect("every insert of a fixture has an output") as u32,
    );
}

fn insert_controls(insert: &Insert, index: u16) -> Vec<ControlValue> {
    let mut controls = Vec::new();
    let mut push = |target: ControlTarget, value: Option<i32>| {
        if let Some(value) = value {
            controls.push(ControlValue {
                location: target.encode(),
                value,
            });
        }
    };
    for slot in &insert.slots {
        let target = |param| ControlTarget::Slot {
            insert: index,
            slot: u16::from(slot.index),
            param,
        };
        push(target(SlotParam::Enabled), slot.enabled.map(i32::from));
        push(target(SlotParam::Mix), slot.mix);
    }
    for route in &insert.routes {
        let target = ControlTarget::Route {
            insert: index,
            target: route.target,
        };
        push(target, route.level);
    }
    let own = |param| ControlTarget::Insert {
        insert: index,
        param,
    };
    push(own(InsertParam::Volume), insert.volume);
    push(own(InsertParam::Pan), insert.pan);
    push(own(InsertParam::StereoSeparation), insert.stereo_separation);
    for (band, values) in insert.eq.iter().enumerate() {
        let band = band as u8;
        push(own(InsertParam::EqGain(band)), values.gain);
        push(own(InsertParam::EqFrequency(band)), values.frequency);
        push(own(InsertParam::EqWidth(band)), values.width);
    }
    controls
}

/// The state FL Studio's wrapper saves for a hosted plugin: a number, then
/// records of an id, a 64-bit length and data.
pub fn wrapper_state(records: &[(u32, &[u8])]) -> Vec<u8> {
    let mut state = 10_u32.to_le_bytes().to_vec();
    for (id, data) in records {
        state.extend(id.to_le_bytes());
        state.extend((data.len() as u64).to_le_bytes());
        state.extend(*data);
    }
    state
}

/// The state of a plugin of FL Studio's own: a row of 32-bit numbers.
pub fn numbers(values: &[i32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

/// The report as `{:?}` shows it, with every line, title and problem in the
/// English words the review shows: what a test that looks for a phrase in
/// the report reads.
pub fn words(report: &windfall_flp::report::ImportReport) -> String {
    let english = windfall_core::english::english;
    let mut report = report.clone();
    for category in &mut report.categories {
        for line in &mut category.lines {
            line.text = english(&line.text);
        }
    }
    for problem in &mut report.read_problems {
        *problem = english(problem);
    }
    format!("{report:?}")
}
