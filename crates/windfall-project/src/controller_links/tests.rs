use super::*;

fn map() -> CompiledMap {
    ControlMap::default().compile().unwrap()
}

fn with(edit: impl FnOnce(&mut ControlMap)) -> CompiledMap {
    let mut map = ControlMap::default();
    edit(&mut map);
    map.compile().unwrap()
}

fn cc(raw: u8) -> SourceInput {
    SourceInput::seven_bit(raw)
}

/// Feeds a message, and tells the link the target took the value.
fn feed(
    state: &mut LinkState,
    map: &CompiledMap,
    raw: u8,
    current: &mut f32,
    pickup: bool,
) -> Motion {
    let motion = state.input(map, cc(raw), *current, pickup);
    if let Motion::Write(value) = motion {
        *current = value;
        state.wrote(value);
    }
    motion
}

#[test]
fn a_map_scales_inverts_and_shapes_the_source() {
    assert_eq!(map().shape(0.25), 0.25);
    let ranged = with(|map| {
        map.min = 0.2;
        map.max = 0.6;
    });
    assert_eq!(ranged.shape(0.0), 0.2);
    assert_eq!(ranged.shape(1.0), 0.6);
    assert!((ranged.shape(0.5) - 0.4).abs() < 1e-6);
    let inverted = with(|map| {
        map.min = 1.0;
        map.max = 0.0;
    });
    assert_eq!(inverted.shape(0.25), 0.75);
    let shaped = with(|map| {
        map.formula = Some("x ^ 2".into());
        map.max = 0.5;
    });
    assert_eq!(shaped.shape(0.5), 0.125);
    // A source that is not a number, or outside its range, is held to it.
    assert_eq!(map().shape(f32::NAN), 0.0);
    assert_eq!(map().shape(4.0), 1.0);
}

#[test]
fn a_bad_map_is_refused_with_a_reason() {
    for edit in [
        (|map: &mut ControlMap| map.min = -0.1) as fn(&mut ControlMap),
        |map| map.max = 1.5,
        |map| map.min = f32::NAN,
        |map| map.smoothing_ms = -1.0,
        |map| map.smoothing_ms = MAX_SMOOTHING_MS + 1.0,
        |map| map.smoothing_ms = f32::INFINITY,
        |map| map.formula = Some("x +".into()),
    ] {
        let mut map = ControlMap::default();
        edit(&mut map);
        assert!(!map.compile().unwrap_err().is_empty());
    }
    // An empty formula is no formula.
    let blank = with(|map| map.formula = Some("   ".into()));
    assert_eq!(blank.formula, None);
}

#[test]
fn without_pickup_a_position_moves_the_target_at_once() {
    let map = map();
    let mut state = LinkState::default();
    let mut current = 0.9;
    assert_eq!(
        feed(&mut state, &map, 0, &mut current, false),
        Motion::Write(0.0)
    );
    assert_eq!(
        feed(&mut state, &map, 127, &mut current, false),
        Motion::Write(1.0)
    );
    // The same position again changes nothing.
    assert_eq!(
        feed(&mut state, &map, 127, &mut current, false),
        Motion::None
    );
    assert!(!state.waiting());
}

#[test]
fn pickup_holds_until_the_hardware_crosses_the_value_from_either_side() {
    let map = map();
    for (start, toward) in [(10_u8, 1_i16), (120, -1)] {
        let mut state = LinkState::default();
        let mut current = 0.5;
        let mut raw = i16::from(start);
        // Every step short of the value is held and leaves it alone.
        loop {
            let next = raw + toward * 10;
            let crosses = (f32::from(next as u8) / 127.0 - 0.5) * f32::from(toward) >= 0.0;
            assert_eq!(
                feed(&mut state, &map, raw as u8, &mut current, true),
                Motion::Held
            );
            assert_eq!(current, 0.5);
            assert!(state.waiting());
            raw = next;
            if crosses {
                break;
            }
        }
        // The step that passes the value takes hold of it.
        let taken = feed(&mut state, &map, raw as u8, &mut current, true);
        assert_eq!(taken, Motion::Write(f32::from(raw as u8) / 127.0));
        assert!(!state.waiting());
        // From then on it follows, back the other way too.
        let back = (raw - toward * 30) as u8;
        assert_eq!(
            feed(&mut state, &map, back, &mut current, true),
            Motion::Write(f32::from(back) / 127.0)
        );
    }
}

#[test]
fn pickup_takes_hold_at_once_when_the_hardware_is_already_there() {
    let map = map();
    let mut state = LinkState::default();
    // 64/127 is within one controller step of 0.5.
    let mut current = 0.5;
    assert_eq!(
        feed(&mut state, &map, 64, &mut current, true),
        Motion::Write(64.0 / 127.0)
    );
    // Exactly on the value: nothing to write, and hold is kept.
    let mut state = LinkState::default();
    let mut current = 1.0;
    assert_eq!(
        feed(&mut state, &map, 127, &mut current, true),
        Motion::None
    );
    assert_eq!(
        feed(&mut state, &map, 100, &mut current, true),
        Motion::Write(100.0 / 127.0)
    );
}

#[test]
fn pickup_starts_over_when_something_else_moves_the_target() {
    let map = map();
    let mut state = LinkState::default();
    let mut current = 0.0;
    assert_eq!(feed(&mut state, &map, 0, &mut current, true), Motion::None);
    assert!(matches!(
        feed(&mut state, &map, 40, &mut current, true),
        Motion::Write(_)
    ));
    // The mouse takes the control far away.
    current = 0.9;
    assert_eq!(feed(&mut state, &map, 41, &mut current, true), Motion::Held);
    assert_eq!(
        feed(&mut state, &map, 100, &mut current, true),
        Motion::Held
    );
    assert_eq!(current, 0.9);
    assert!(matches!(
        feed(&mut state, &map, 120, &mut current, true),
        Motion::Write(_)
    ));
}

#[test]
fn a_link_can_overrule_the_pickup_setting_both_ways() {
    let always = with(|map| map.pickup = PickupMode::On);
    let mut state = LinkState::default();
    let mut current = 0.5;
    assert_eq!(
        feed(&mut state, &always, 0, &mut current, false),
        Motion::Held
    );
    let never = with(|map| map.pickup = PickupMode::Off);
    let mut state = LinkState::default();
    assert_eq!(
        feed(&mut state, &never, 0, &mut current, true),
        Motion::Write(0.0)
    );
}

#[test]
fn endless_knobs_step_the_target_in_each_encoding() {
    for (mode, up_one, down_one, up_three, down_three) in [
        (
            SourceMode::RelativeTwosComplement,
            1_u8,
            127_u8,
            3_u8,
            125_u8,
        ),
        (SourceMode::RelativeSignedBit, 1, 65, 3, 67),
        (SourceMode::RelativeBinaryOffset, 65, 63, 67, 61),
    ] {
        assert_eq!(mode.steps(up_one), 1);
        assert_eq!(mode.steps(down_one), -1);
        assert_eq!(mode.steps(up_three), 3);
        assert_eq!(mode.steps(down_three), -3);
        let map = with(|map| map.mode = mode);
        let mut state = LinkState::default();
        let mut current = 0.5;
        // Pickup does not apply to a knob with no position.
        let Motion::Write(value) = feed(&mut state, &map, up_three, &mut current, true) else {
            panic!("an endless knob moves the target at once");
        };
        assert!((value - (0.5 + 3.0 / 127.0)).abs() < 1e-6);
        let Motion::Write(value) = feed(&mut state, &map, down_one, &mut current, true) else {
            panic!();
        };
        assert!((value - (0.5 + 2.0 / 127.0)).abs() < 1e-6);
    }
    assert_eq!(SourceMode::RelativeBinaryOffset.steps(64), 0);
    assert_eq!(SourceMode::Absolute.steps(99), 0);
}

#[test]
fn endless_knobs_stop_at_the_ends_of_the_map_and_follow_its_direction() {
    let map = with(|map| {
        map.mode = SourceMode::RelativeBinaryOffset;
        map.min = 0.25;
        map.max = 0.75;
    });
    let mut state = LinkState::default();
    let mut current = 0.74;
    assert_eq!(
        feed(&mut state, &map, 127, &mut current, false),
        Motion::Write(0.75)
    );
    assert_eq!(
        feed(&mut state, &map, 127, &mut current, false),
        Motion::None
    );
    current = 0.26;
    state.wrote(0.26);
    assert_eq!(
        feed(&mut state, &map, 0, &mut current, false),
        Motion::Write(0.25)
    );
    // A map that runs backwards turns the knob's direction round.
    let backwards = with(|map| {
        map.mode = SourceMode::RelativeBinaryOffset;
        map.min = 1.0;
        map.max = 0.0;
    });
    let mut state = LinkState::default();
    let mut current = 0.5;
    let Motion::Write(value) = feed(&mut state, &backwards, 65, &mut current, false) else {
        panic!();
    };
    assert!(value < 0.5);
}

#[test]
fn a_toggle_button_flips_between_the_ends_on_each_press_only() {
    let map = with(|map| {
        map.mode = SourceMode::Toggle;
        map.min = 0.2;
        map.max = 0.8;
    });
    let mut state = LinkState::default();
    let mut current = 0.2;
    let press = SourceInput::button(true);
    let release = SourceInput::button(false);
    assert_eq!(state.input(&map, press, current, true), Motion::Write(0.8));
    current = 0.8;
    // Holding and letting go do nothing.
    assert_eq!(state.input(&map, press, current, true), Motion::None);
    assert_eq!(state.input(&map, release, current, true), Motion::None);
    assert_eq!(state.input(&map, press, current, true), Motion::Write(0.2));
}

#[test]
fn a_button_press_is_seen_once() {
    let mut state = LinkState::default();
    assert!(state.pressed(SourceInput::button(true)));
    assert!(!state.pressed(SourceInput::button(true)));
    assert!(!state.pressed(SourceInput::button(false)));
    assert!(state.pressed(cc(127)));
    assert!(!state.pressed(cc(0)));
    // A controller counts as down from half way up.
    assert!(state.pressed(cc(64)));
}

#[test]
fn smoothing_glides_to_the_value_and_settles_on_it() {
    let map = with(|map| map.smoothing_ms = 100.0);
    let mut state = LinkState::default();
    let current = 0.0;
    assert_eq!(state.input(&map, cc(127), current, false), Motion::None);
    assert!(state.gliding());
    // One time constant covers 1 - 1/e of the way.
    let Motion::Write(first) = state.advance(&map, 0.1) else {
        panic!("a glide writes its steps");
    };
    assert!((first - (1.0 - (-1.0_f32).exp())).abs() < 1e-5);
    let mut last = first;
    let mut steps = 0;
    while state.gliding() {
        let Motion::Write(value) = state.advance(&map, 0.005) else {
            panic!();
        };
        assert!(value >= last, "a glide never turns back");
        last = value;
        steps += 1;
        assert!(steps < 10_000, "a glide ends");
    }
    assert_eq!(last, 1.0);
    assert_eq!(state.advance(&map, 0.005), Motion::None);
    // No time, or time that is not a number, moves nothing.
    assert_eq!(state.input(&map, cc(0), 1.0, false), Motion::None);
    assert_eq!(state.advance(&map, 0.0), Motion::None);
    assert_eq!(state.advance(&map, f32::NAN), Motion::None);
    assert!(state.gliding());
}

#[test]
fn a_new_value_during_a_glide_redirects_it_from_where_it_is() {
    let map = with(|map| map.smoothing_ms = 50.0);
    let mut state = LinkState::default();
    state.input(&map, cc(127), 0.0, false);
    let Motion::Write(part) = state.advance(&map, 0.05) else {
        panic!();
    };
    state.wrote(part);
    state.input(&map, cc(0), part, false);
    let Motion::Write(next) = state.advance(&map, 0.001) else {
        panic!();
    };
    assert!(
        next < part && next > part - 0.05,
        "no jump: {part} -> {next}"
    );
}

#[test]
fn smoothing_and_pickup_work_together() {
    let map = with(|map| map.smoothing_ms = 20.0);
    let mut state = LinkState::default();
    assert_eq!(state.input(&map, cc(0), 0.5, true), Motion::Held);
    assert!(!state.gliding());
    assert_eq!(state.advance(&map, 0.01), Motion::None);
    assert_eq!(state.input(&map, cc(100), 0.5, true), Motion::None);
    assert!(state.gliding());
}

#[test]
fn sources_normalize_seven_and_fourteen_bits() {
    assert_eq!(cc(0).value, 0.0);
    assert_eq!(cc(127).value, 1.0);
    assert_eq!(cc(200).raw, 127);
    assert_eq!(SourceInput::fourteen_bit(0).value, 0.0);
    assert_eq!(SourceInput::fourteen_bit(16_383).value, 1.0);
    assert_eq!(SourceInput::fourteen_bit(8_192).raw, 64);
    assert_eq!(SourceInput::fourteen_bit(u16::MAX).value, 1.0);
    assert_eq!(SourceInput::button(true).value, 1.0);
    assert_eq!(SourceInput::button(false).value, 0.0);
}

#[test]
fn built_in_profiles_are_valid_distinct_and_matched_by_port_name() {
    let profiles = builtin_profiles();
    assert!(profiles.len() >= 3);
    for (at, profile) in profiles.iter().enumerate() {
        assert_eq!(profile.problem(), None, "{}", profile.id);
        assert!(!profile.controls.is_empty());
        assert!(profiles[..at].iter().all(|other| other.id != profile.id));
        // A profile survives its own file format.
        let json = serde_json::to_string(profile).unwrap();
        let back: ControllerProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, profile);
        // No profile can name one project's ids.
        assert!(
            profile
                .controls
                .iter()
                .all(|control| !matches!(control.target, LinkTarget::Param { .. }))
        );
    }
    assert_eq!(
        profile_for_port("nanoKONTROL2 1 SLIDER/KNOB").unwrap().id,
        "korg-nanokontrol2"
    );
    assert_eq!(
        profile_for_port("Mackie Control Universal").unwrap().id,
        "mackie-control"
    );
    assert_eq!(profile_for_port("Microsoft GS Wavetable Synth"), None);
}

#[test]
fn a_profile_has_relative_knobs_and_transport_buttons() {
    let mackie = builtin_profiles()
        .into_iter()
        .find(|profile| profile.id == "mackie-control")
        .unwrap();
    let links = mackie.links(Some("Mackie Control"));
    assert_eq!(links.len(), mackie.controls.len());
    let pot = links
        .iter()
        .find(|link| {
            link.target
                == LinkTarget::Slot {
                    slot: SlotTarget::MixerPan { index: 2 },
                }
        })
        .unwrap();
    assert_eq!(pot.map.mode, SourceMode::RelativeSignedBit);
    assert_eq!(
        pot.source,
        ControlSource::Midi {
            device: Some("Mackie Control".into()),
            channel: Some(1),
            control: MidiControl::Cc { number: 18 },
        }
    );
    for action in [
        ControlAction::Play,
        ControlAction::Stop,
        ControlAction::BankNext,
    ] {
        assert!(
            links
                .iter()
                .any(|link| link.target == LinkTarget::Action { action })
        );
    }
    let fader = links
        .iter()
        .find(|link| {
            link.target
                == LinkTarget::Slot {
                    slot: SlotTarget::MasterVolume,
                }
        })
        .unwrap();
    assert!(matches!(
        fader.source,
        ControlSource::Midi {
            channel: Some(9),
            control: MidiControl::PitchBend,
            ..
        }
    ));
}

#[test]
fn a_malformed_profile_is_refused() {
    let good = builtin_profiles().remove(0);
    let mut bad = good.clone();
    bad.id = "Has Spaces".into();
    assert!(bad.problem().is_some());
    let mut bad = good.clone();
    bad.name = " ".into();
    assert!(bad.problem().is_some());
    let mut bad = good.clone();
    bad.controls[0].channel = Some(17);
    assert!(bad.problem().is_some());
    let mut bad = good.clone();
    bad.controls[0].target = LinkTarget::Param {
        target: AutomationTarget::Tempo,
    };
    assert!(bad.problem().unwrap().contains("every project"));
    let mut bad = good.clone();
    let repeat = bad.controls[0].clone();
    bad.controls.push(repeat);
    assert!(bad.problem().unwrap().contains("same source"));
    let mut bad = good;
    bad.controls[0].map.formula = Some("nope(x)".into());
    assert!(bad.problem().is_some());
}
