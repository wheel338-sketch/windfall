//! Small in-test SF2 writer, including linked stereo, sm24 and explicit routing.
pub fn bytes() -> Vec<u8> {
    fn chunk(id: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = id.to_vec();
        out.extend((data.len() as u32).to_le_bytes());
        out.extend(data);
        if !data.len().is_multiple_of(2) {
            out.push(0);
        }
        out
    }
    fn list(id: &[u8; 4], entries: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut out = id.to_vec();
        for (id, data) in entries {
            out.extend(chunk(id, data));
        }
        chunk(b"LIST", &out)
    }
    fn records(values: &[(u16, i16)]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|&(a, b)| a.to_le_bytes().into_iter().chain(b.to_le_bytes()))
            .collect()
    }
    let mut phdr = vec![0; 76];
    phdr[62..64].copy_from_slice(&1u16.to_le_bytes());
    let mut inst = vec![0; 44];
    inst[42..44].copy_from_slice(&1u16.to_le_bytes());
    let mut shdr = vec![0; 138];
    for index in 0..2 {
        let h = &mut shdr[index * 46..(index + 1) * 46];
        for (field, value) in [
            (20, index * 8192),
            (24, (index + 1) * 8192),
            (28, index * 8192 + 1),
            (32, (index + 1) * 8192 - 1),
            (36, 48000),
        ] {
            h[field..field + 4].copy_from_slice(&(value as u32).to_le_bytes());
        }
        h[40] = 60;
        h[42..44].copy_from_slice(&((1 - index) as u16).to_le_bytes());
        h[44..46].copy_from_slice(&(if index == 0 { 4u16 } else { 2u16 }).to_le_bytes());
    }
    let generators = [
        (5, 120),
        (6, 60),
        (7, 120),
        (8, 10000),
        (9, 120),
        (10, 120),
        (11, 120),
        (13, 30),
        (15, 200),
        (16, 200),
        (21, -12000),
        (22, -1200),
        (23, -12000),
        (24, -1200),
        (25, -32768),
        (26, -12000),
        (27, -12000),
        (28, -1200),
        (29, 200),
        (30, -1200),
        (31, 10),
        (32, 10),
        (33, -32768),
        (34, -12000),
        (35, -12000),
        (36, -1200),
        (37, 60),
        (38, -1200),
        (39, 10),
        (40, 10),
        (54, 3),
        (57, 1),
        (53, 0),
        (0, 0),
    ];
    let mut imod = Vec::new();
    for (src, dst, amount, secondary, transform) in [
        (0x0094u16, 0x8001u16, 1i16, 0u16, 0u16),
        (127, 5, 1200, 0x0095, 0),
        (0, 0, 0, 0, 0),
    ] {
        for value in [src, dst, amount as u16, secondary, transform] {
            imod.extend(value.to_le_bytes());
        }
    }
    let mut out = b"sfbk".to_vec();
    out.extend(list(b"INFO", &[(b"ifil", vec![2, 0, 4, 0])]));
    out.extend(list(
        b"sdta",
        &[
            (
                b"smpl",
                (0..16384)
                    .flat_map(|i| (((i % 64) as i16 - 32) * 100).to_le_bytes())
                    .collect(),
            ),
            (b"sm24", vec![128; 16384]),
        ],
    ));
    out.extend(list(
        b"pdta",
        &[
            (b"phdr", phdr),
            (b"pbag", records(&[(0, 0), (1, 0)])),
            (b"pmod", vec![0; 10]),
            (b"pgen", records(&[(41, 0), (0, 0)])),
            (b"inst", inst),
            (
                b"ibag",
                records(&[(0, 0), ((generators.len() - 1) as u16, 2)]),
            ),
            (b"imod", imod),
            (b"igen", records(&generators)),
            (b"shdr", shdr),
        ],
    ));
    chunk(b"RIFF", &out)
}
