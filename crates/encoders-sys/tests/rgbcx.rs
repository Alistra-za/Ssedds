use encoders_sys::*;

// The ramp channels hold 16 distinct values 17 apart, so an 8-value BC4 palette needs endpoints
// near 247 and 9 to stay within 10 (max error 9). rgbcx only searches +-radius around min/max,
// and radius 5 (BC45_RADIUS_DEFAULT) cannot reach them (max error 12), so these ramps use the max radius.
fn ramp_block() -> [[u8; 4]; 16] {
    let mut b = [[0u8; 4]; 16];
    for (i, p) in b.iter_mut().enumerate() {
        *p = [
            (i * 17) as u8,
            (255 - i * 17) as u8,
            (i * 5) as u8,
            (i * 16) as u8,
        ];
    }
    b
}

#[test]
fn bc1_with_transparent_black_off_always_decodes_opaque() {
    // Black texels are what tempt rgbcx into 3-colour transparent-black mode.
    let mut b = ramp_block();
    for p in b.iter_mut().take(6) {
        *p = [0, 0, 0, 255];
    }
    let enc = bc1_encode_block(&b, RGBCX_MAX_LEVEL, true, false);
    for model in [
        Bc1Model::Ideal,
        Bc1Model::Nvidia,
        Bc1Model::Amd,
        Bc1Model::IdealRound4,
    ] {
        assert!(
            bc1_decode_block(&enc, model).iter().all(|p| p[3] == 255),
            "{model:?}"
        );
    }
}

#[test]
fn bc4_ramp_is_close() {
    let b = ramp_block();
    let enc = bc4_encode_block(&b, 0, BC45_RADIUS_MAX);
    let mut dec = [0u8; 16];
    bcdec_rs::bc4(&enc, &mut dec, 4, false);
    for (i, d) in dec.iter().enumerate() {
        assert!(
            (*d as i32 - b[i][0] as i32).abs() <= 10,
            "texel {i}: {d} vs {}",
            b[i][0]
        );
    }
}

#[test]
fn bc5_encodes_red_and_green() {
    let b = ramp_block();
    let enc = bc5_encode_block(&b, BC45_RADIUS_MAX);
    let mut dec = [0u8; 32];
    bcdec_rs::bc5(&enc, &mut dec, 8, false);
    for i in 0..16 {
        assert!((dec[i * 2] as i32 - b[i][0] as i32).abs() <= 10);
        assert!((dec[i * 2 + 1] as i32 - b[i][1] as i32).abs() <= 10);
    }
}

#[test]
fn bc3_keeps_alpha() {
    let b = ramp_block();
    let enc = bc3_encode_block(&b, RGBCX_MAX_LEVEL, BC45_RADIUS_MAX);
    let mut dec = [0u8; 64];
    bcdec_rs::bc3(&enc, &mut dec, 16);
    for i in 0..16 {
        assert!((dec[i * 4 + 3] as i32 - b[i][3] as i32).abs() <= 10);
    }
}

#[test]
fn bc4_reads_the_requested_channel() {
    let b = ramp_block();
    let max_err = |ch: usize| {
        let enc = bc4_encode_block(&b, ch, BC45_RADIUS_DEFAULT);
        let mut dec = [0u8; 16];
        bcdec_rs::bc4(&enc, &mut dec, 4, false);
        (0..16)
            .map(|i| (dec[i] as i32 - b[i][ch] as i32).abs())
            .max()
            .unwrap()
    };
    // Channel 2 (0..=75) is nearly representable; channel 0 (0..=255) is not.
    assert!(max_err(2) <= 5, "channel 2: {}", max_err(2));
    assert!(max_err(0) > 5, "channel 0: {}", max_err(0));
}

#[test]
fn larger_search_radius_never_hurts_bc4() {
    let b = ramp_block();
    let err = |rad: u32| {
        let enc = bc4_encode_block(&b, 0, rad);
        let mut dec = [0u8; 16];
        bcdec_rs::bc4(&enc, &mut dec, 4, false);
        (0..16)
            .map(|i| (dec[i] as i32 - b[i][0] as i32).pow(2))
            .sum::<i32>()
    };
    assert!(err(BC45_RADIUS_MAX) <= err(BC45_RADIUS_DEFAULT));
}

// M6 Track D: the palette rgbcx searches with must equal what bcdec (and D3D hardware) decodes, for every endpoint
// pair in both the 8-value and the 6-value mode; otherwise the search optimises for a palette nobody decodes.
#[test]
fn bc4_search_model_matches_bcdec_for_every_endpoint_pair() {
    for e0 in 0..=255u8 {
        for e1 in 0..=255u8 {
            // Texel i uses selector i % 8 (3 bits per texel after the two endpoint bytes).
            let sels: u64 = (0..16u64).fold(0, |acc, i| acc | ((i % 8) << (3 * i)));
            let mut blk = [0u8; 8];
            blk[..2].copy_from_slice(&[e0, e1]);
            blk[2..].copy_from_slice(&sels.to_le_bytes()[..6]);
            let mut dec = [0u8; 16];
            bcdec_rs::bc4(&blk, &mut dec, 4, false);
            assert_eq!(bc4_block_values(e0, e1), dec[..8], "endpoints ({e0}, {e1})");
        }
    }
}
