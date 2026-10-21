use encoders_sys::{bc7e_compress_blocks, gather_blocks, Bc7Level, Bc7Params};

const U5: Bc7Params = Bc7Params {
    level: Bc7Level::Veryslow,
    perceptual: false,
    exact_opaque: false,
};

fn decode(block: &[u8]) -> Vec<[u8; 4]> {
    let mut out = [0u8; 64];
    bcdec_rs::bc7(block, &mut out, 16);
    out.chunks(4).map(|c| [c[0], c[1], c[2], c[3]]).collect()
}

#[test]
fn gather_blocks_orders_blocks_row_major_and_replicates_edges() {
    // 5x3 image, texel value = (x, y, 0, 255)
    let px: Vec<[u8; 4]> = (0..3)
        .flat_map(|y| (0..5).map(move |x| [x as u8, y as u8, 0, 255]))
        .collect();
    let b = gather_blocks(&px, 5, 3);
    assert_eq!(b.len(), 2 * 16); // 2x1 blocks
    assert_eq!(b[0], [0, 0, 0, 255]);
    assert_eq!(b[5], [1, 1, 0, 255]); // block 0, row 1, col 1
    assert_eq!(b[15], [3, 2, 0, 255]); // block 0, row 3 replicates row 2
    assert_eq!(b[16], [4, 0, 0, 255]); // block 1 starts at x = 4
    assert_eq!(b[17], [4, 0, 0, 255]); // x = 5 replicates x = 4
}

#[test]
fn solid_block_round_trips_exactly() {
    let px = vec![[200u8, 100, 50, 255]; 16];
    let mut out = [0u8; 16];
    bc7e_compress_blocks(&px, &mut out, U5);
    assert!(decode(&out).iter().all(|p| *p == [200, 100, 50, 255]));
}

#[test]
fn opaque_gradient_decodes_alpha_at_least_254() {
    let px: Vec<[u8; 4]> = (0..16)
        .map(|i| [i as u8 * 16, 255 - i as u8 * 10, 7 * i as u8, 255])
        .collect();
    let mut out = [0u8; 16];
    bc7e_compress_blocks(&px, &mut out, U5);
    assert!(decode(&out).iter().all(|p| p[3] >= 254));
}

#[test]
#[should_panic(expected = "16 bytes per block")]
fn rejects_mismatched_output_length() {
    let px = vec![[0u8; 4]; 16];
    let mut out = [0u8; 8];
    bc7e_compress_blocks(&px, &mut out, U5);
}

#[test]
#[cfg(not(sdc_bc7e_patched))]
#[should_panic(expected = "exact_opaque needs the patched bc7e objects")]
fn exact_opaque_without_native_support_panics() {
    let px = vec![[0u8, 0, 0, 255]; 16];
    let mut out = [0u8; 16];
    bc7e_compress_blocks(
        &px,
        &mut out,
        Bc7Params {
            exact_opaque: true,
            ..U5
        },
    );
}

#[test]
#[allow(clippy::assertions_on_constants)] // the constant is what this test checks (build.rs cfg)
fn patched_objects_are_vendored() {
    assert!(encoders_sys::BC7E_EXACT_OPAQUE_NATIVE);
    assert_eq!(
        encoders_sys::versions().bc7e_patch,
        Some("0001-exact-opaque-alpha")
    );
}

#[test]
fn exact_opaque_decodes_alpha_exactly_255_where_upstream_gives_254() {
    // A dark opaque ramp that upstream bc7e u5 encodes in mode 6 with a p-bit 0 endpoint (alpha 254).
    let px: Vec<[u8; 4]> = (0..16u8).map(|i| [i, i / 2, 100 + i, 255]).collect();
    let (mut off, mut on) = ([0u8; 16], [0u8; 16]);
    bc7e_compress_blocks(&px, &mut off, U5);
    bc7e_compress_blocks(
        &px,
        &mut on,
        Bc7Params {
            exact_opaque: true,
            ..U5
        },
    );
    assert!(
        decode(&off).iter().any(|p| p[3] == 254),
        "flag off: {off:02x?}"
    );
    assert_eq!(on[0] & 0x7f, 0x40, "flag on: still mode 6: {on:02x?}");
    assert!(
        decode(&on).iter().all(|p| p[3] == 255),
        "flag on: {on:02x?}"
    );
}

/// Note CP: a tune that sets only the weights leaves every other level parameter alone: unit weights give the plain
/// call's bytes at each level, and other weights reach bc7e.
#[test]
fn tuned_with_unit_weights_matches_the_plain_call() {
    use encoders_sys::{bc7e_compress_blocks_tuned, Bc7Tune};
    let mut s = 0x9e37_79b9_u32;
    let px: Vec<[u8; 4]> = (0..64 * 16)
        .map(|i| {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let b = s.to_le_bytes();
            // Every fourth block opaque, so the exact-opaque tables are exercised too.
            let a = if (i / 16) % 4 == 0 { 255 } else { b[3] };
            [b[0], b[1], b[2], a]
        })
        .collect();
    let tune = |weights| Bc7Tune {
        weights: Some(weights),
        ..Default::default()
    };
    let levels = [
        Bc7Level::Ultrafast,
        Bc7Level::Veryfast,
        Bc7Level::Fast,
        Bc7Level::Basic,
        Bc7Level::Slow,
        Bc7Level::Veryslow,
        Bc7Level::Slowest,
        Bc7Level::Max,
    ];
    let exacts: &[bool] = if encoders_sys::BC7E_EXACT_OPAQUE_NATIVE {
        &[false, true]
    } else {
        &[false]
    };
    let mut checked = 0;
    for level in levels {
        if level == Bc7Level::Max && !encoders_sys::BC7E_MAX_NATIVE {
            continue;
        }
        for &exact_opaque in exacts {
            let p = Bc7Params {
                level,
                exact_opaque,
                ..U5
            };
            let what = format!("{level:?} exact={exact_opaque}");
            let mut plain = vec![0u8; 64 * 16];
            bc7e_compress_blocks(&px, &mut plain, p);
            let mut tuned = vec![0u8; 64 * 16];
            bc7e_compress_blocks_tuned(&px, &mut tuned, p, tune([1, 1, 1, 1]));
            assert_eq!(plain, tuned, "{what}");
            bc7e_compress_blocks_tuned(&px, &mut tuned, p, tune([1, 1, 1, 8]));
            assert_ne!(plain, tuned, "{what}: the weights reach bc7e");
            checked += 1;
        }
    }
    // The vendored objects carry both patches: every level, both exact-opaque settings.
    assert_eq!(checked, 16);
}
