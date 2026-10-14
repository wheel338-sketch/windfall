use windfall_project::timeline::MeterMap;
use windfall_project::{
    MAX_SONG_TICKS, MeterChange, MeterChangeId, MusicalPosition, TimeSignature,
};

fn signature(numerator: u8, denominator: u8) -> TimeSignature {
    TimeSignature {
        numerator,
        denominator,
    }
}

#[test]
fn shared_ui_fixtures_prove_rust_positions_and_inverse() {
    #[derive(serde::Deserialize)]
    struct Fixture {
        legacy: TimeSignature,
        meters: Vec<MeterChange>,
        positions: Vec<Entry>,
    }
    #[derive(serde::Deserialize)]
    struct Entry {
        tick: u32,
        position: MusicalPosition,
    }
    let fixture: Fixture = serde_json::from_str(include_str!("meter-fixtures.json")).unwrap();
    let map = MeterMap::new(fixture.legacy, &fixture.meters).unwrap();
    for entry in fixture.positions {
        assert_eq!(map.tick_to_position(entry.tick).unwrap(), entry.position);
        assert_eq!(map.position_to_tick(entry.position).unwrap(), entry.tick);
    }
}

#[test]
fn unaligned_nonzero_changes_have_shortened_bars_and_checked_inverse() {
    let map = MeterMap::new(
        signature(4, 4),
        &[
            MeterChange {
                id: MeterChangeId(2),
                tick: 4_001,
                signature: signature(7, 8),
            },
            MeterChange {
                id: MeterChangeId(3),
                tick: 7_400,
                signature: signature(3, 4),
            },
        ],
    )
    .unwrap();
    for (tick, bar, beat, within) in [
        (0, 1, 1, 0),
        (3_840, 2, 1, 0),
        (4_000, 2, 1, 160),
        (4_001, 3, 1, 0),
        (4_481, 3, 2, 0),
        (7_360, 3, 7, 479),
        (7_361, 4, 1, 0),
        (7_399, 4, 1, 38),
        (7_400, 5, 1, 0),
        (8_360, 5, 2, 0),
        (10_280, 6, 1, 0),
    ] {
        let position = MusicalPosition {
            bar,
            beat,
            tick: within,
        };
        assert_eq!(map.tick_to_position(tick).unwrap(), position);
        assert_eq!(map.position_to_tick(position).unwrap(), tick);
    }
    assert!(
        map.position_to_tick(MusicalPosition {
            bar: 2,
            beat: 1,
            tick: 161
        })
        .is_err()
    );
    assert!(
        map.position_to_tick(MusicalPosition {
            bar: 4,
            beat: 1,
            tick: 39
        })
        .is_err()
    );
    assert!(
        map.position_to_tick(MusicalPosition {
            bar: 5,
            beat: 4,
            tick: 0
        })
        .is_err()
    );
    let last = map.tick_to_position(MAX_SONG_TICKS).unwrap();
    assert_eq!(map.position_to_tick(last).unwrap(), MAX_SONG_TICKS);
    assert!(
        map.position_to_tick(MusicalPosition {
            bar: u32::MAX,
            beat: 1,
            tick: 0
        })
        .is_err()
    );
}

#[test]
fn invalid_and_colliding_meter_ticks_never_divide_by_zero() {
    assert!(MeterMap::new(signature(4, 0), &[]).is_err());
    assert!(
        MeterMap::new(
            signature(4, 4),
            &[
                MeterChange {
                    id: MeterChangeId(2),
                    tick: 100,
                    signature: signature(7, 8)
                },
                MeterChange {
                    id: MeterChangeId(3),
                    tick: 100,
                    signature: signature(3, 4)
                },
            ]
        )
        .is_err()
    );
    let map = MeterMap::new(
        signature(3, 8),
        &[MeterChange {
            id: MeterChangeId(2),
            tick: 0,
            signature: signature(7, 16),
        }],
    )
    .unwrap();
    assert_eq!(map.tick_to_position(1_680).unwrap().bar, 2);
}

#[test]
fn tick_485_meter_starts_the_second_bar_and_tick_3845_starts_the_third() {
    let map = MeterMap::checked(
        signature(4, 4),
        &[MeterChange {
            id: MeterChangeId(2),
            tick: 485,
            signature: signature(7, 8),
        }],
    )
    .unwrap();
    assert_eq!(map.segments()[1].start_tick(), 485);
    assert_eq!(map.segments()[1].bar_origin_index(), 1);
    for (tick, bar, beat, within) in [
        (484, 1, 1, 484),
        (485, 2, 1, 0),
        (3844, 2, 7, 479),
        (3845, 3, 1, 0),
        (3846, 3, 1, 1),
        (7205, 4, 1, 0),
    ] {
        let position = MusicalPosition {
            bar,
            beat,
            tick: within,
        };
        assert_eq!(map.tick_to_position(tick).unwrap(), position);
        assert_eq!(map.position_to_tick(position).unwrap(), tick);
    }
    assert!(
        map.position_to_tick(MusicalPosition {
            bar: 1,
            beat: 1,
            tick: 485,
        })
        .is_err()
    );
}

#[test]
fn checked_segment_anchors_match_shortened_and_aligned_bars_without_tempo() {
    let map = MeterMap::new(
        signature(4, 4),
        &[
            MeterChange {
                id: MeterChangeId(2),
                tick: 4_001,
                signature: signature(7, 8),
            },
            MeterChange {
                id: MeterChangeId(3),
                tick: 7_361,
                signature: signature(3, 4),
            },
            MeterChange {
                id: MeterChangeId(4),
                tick: 7_400,
                signature: signature(7, 16),
            },
        ],
    )
    .unwrap();
    let anchors: Vec<_> = map
        .segments()
        .iter()
        .map(|segment| {
            (
                segment.start_tick(),
                segment.bar_origin_index(),
                segment.signature(),
            )
        })
        .collect();
    assert_eq!(
        anchors,
        [
            (0, 0, signature(4, 4)),
            (4_001, 2, signature(7, 8)),
            (7_361, 3, signature(3, 4)),
            (7_400, 4, signature(7, 16)),
        ]
    );
    for segment in map.segments() {
        assert_eq!(
            map.tick_to_position(segment.start_tick()).unwrap().bar - 1,
            segment.bar_origin_index()
        );
        assert!(segment.bar_origin_index() < i32::MAX as u32);
    }
    let zero = MeterMap::new(
        signature(4, 4),
        &[MeterChange {
            id: MeterChangeId(1),
            tick: 0,
            signature: signature(7, 8),
        }],
    )
    .unwrap();
    assert_eq!(zero.segments().len(), 1);
    assert_eq!(zero.segments()[0].bar_origin_index(), 0);
    assert_eq!(zero.segments()[0].signature(), signature(7, 8));
    let narrow = MeterMap::new(signature(1, 16), &[]).unwrap();
    assert!(narrow.tick_to_position(MAX_SONG_TICKS).unwrap().bar - 1 < i32::MAX as u32);
}

#[test]
fn typed_preparation_errors_preserve_the_document_string_adapter() {
    use windfall_project::timeline::{MAX_TIMELINE_ITEMS, MeterMapError};
    for (signature, cause, message) in [
        (
            signature(0, 4),
            MeterMapError::Numerator(0),
            "a time signature needs 1 to 16 beats per bar, not 0",
        ),
        (
            signature(4, 0),
            MeterMapError::Denominator(0),
            "a time signature's beat unit must be 2, 4, 8 or 16, not 0",
        ),
    ] {
        assert_eq!(MeterMap::checked(signature, &[]).err(), Some(cause));
        assert_eq!(
            MeterMap::new(signature, &[])
                .err()
                .map(|error| windfall_core::english::english(&error))
                .as_deref(),
            Some(message)
        );
    }
    let invalid = [MeterChange {
        id: MeterChangeId(2),
        tick: MAX_SONG_TICKS,
        signature: signature(7, 8),
    }];
    assert_eq!(
        MeterMap::checked(signature(4, 4), &invalid).err(),
        Some(MeterMapError::UnorderedOrOutOfBounds)
    );
    let oversized: Vec<_> = (0..=MAX_TIMELINE_ITEMS)
        .map(|tick| MeterChange {
            id: MeterChangeId(tick as u32 + 2),
            tick: tick as u32,
            signature: signature(4, 4),
        })
        .collect();
    assert_eq!(
        MeterMap::checked(signature(4, 4), &oversized).err(),
        Some(MeterMapError::TooManyChanges)
    );
}
