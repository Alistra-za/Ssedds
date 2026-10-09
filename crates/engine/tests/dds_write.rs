use engine::config::{Format, HeaderForm};
use engine::dds::*;

fn u(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().unwrap())
}

/// A full chain where level `i` is filled with the distinct byte `0x10 + i`.
fn chain(format: Format, w: u32, h: u32) -> Vec<Vec<u8>> {
    let mut v = vec![];
    let (mut w, mut h) = (w, h);
    loop {
        v.push(vec![0x10 + v.len() as u8; level_size(format, w, h)]);
        if w == 1 && h == 1 {
            break;
        }
        w = (w / 2).max(1);
        h = (h / 2).max(1);
    }
    v
}

#[test]
fn bc7_srgb_full_chain_header() {
    let d = DdsDesc {
        width: 256,
        height: 64,
        format: Format::Bc7,
        srgb: true,
        header: resolve_header(Format::Bc7, true, HeaderForm::Legacy),
    };
    assert_eq!(d.header, ResolvedHeader::Dx10);
    let levels = chain(Format::Bc7, 256, 64);
    let b = dds_bytes(&d, &levels).unwrap();
    assert_eq!(&b[..4], b"DDS ");
    assert_eq!(u(&b, 4), 124);
    assert_eq!(
        u(&b, 8),
        DDSD_CAPS
            | DDSD_HEIGHT
            | DDSD_WIDTH
            | DDSD_PIXELFORMAT
            | DDSD_LINEARSIZE
            | DDSD_MIPMAPCOUNT
    );
    assert_eq!(
        (u(&b, 12), u(&b, 16), u(&b, 20), u(&b, 28)),
        (64, 256, 64 * 16 * 16, 9)
    );
    assert_eq!(u(&b, 24), 1, "dwDepth");
    assert_eq!(u(&b, 80) & DDPF_NORMAL, 0);
    assert_eq!(&b[84..88], b"DX10");
    assert_eq!(
        u(&b, 108),
        DDSCAPS_TEXTURE | DDSCAPS_COMPLEX | DDSCAPS_MIPMAP
    );
    assert_eq!(
        (u(&b, 128), u(&b, 132), u(&b, 136), u(&b, 140), u(&b, 144)),
        (99, 3, 0, 1, 0)
    );
    assert!(b[32..76].iter().all(|&x| x == 0), "reserved1 must be zero");
    assert_payload(&b, 148, &levels);
}

/// The payload is exactly `levels` back to back from `off`, mip 0 first, with nothing after it.
fn assert_payload(b: &[u8], off: usize, levels: &[Vec<u8>]) {
    let mut at = off;
    for (i, l) in levels.iter().enumerate() {
        assert_eq!(&b[at..at + l.len()], &l[..], "level {i} at offset {at}");
        at += l.len();
    }
    assert_eq!(b.len(), at, "no bytes after the last level");
}

#[test]
fn legacy_forms() {
    assert_eq!(
        resolve_header(Format::Bc1, false, HeaderForm::Auto),
        ResolvedHeader::Legacy
    );
    assert_eq!(
        resolve_header(Format::Bc4, false, HeaderForm::Auto),
        ResolvedHeader::Dx10
    );
    assert_eq!(
        resolve_header(Format::Bc4, false, HeaderForm::Legacy),
        ResolvedHeader::Legacy
    );
    assert_eq!(
        resolve_header(Format::Bc1, true, HeaderForm::Auto),
        ResolvedHeader::Dx10
    );
    for (f, cc) in [
        (Format::Bc1, b"DXT1"),
        (Format::Bc3, b"DXT5"),
        (Format::Bc4, b"BC4U"),
        (Format::Bc5, b"BC5U"),
    ] {
        let d = DdsDesc {
            width: 4,
            height: 4,
            format: f,
            srgb: false,
            header: ResolvedHeader::Legacy,
        };
        let b = dds_bytes(&d, &[vec![0; level_size(f, 4, 4)]]).unwrap();
        assert_eq!(&b[84..88], cc);
        assert_eq!(u(&b, 8) & DDSD_MIPMAPCOUNT, 0);
        assert_eq!(u(&b, 108), DDSCAPS_TEXTURE);
        assert_eq!(b.len(), 128 + level_size(f, 4, 4));
    }
    assert_eq!(dxgi_format(Format::Bc6hUf16, false), 95);
    assert_eq!(dxgi_format(Format::Bc1, true), 72);
}

#[test]
fn resolve_header_matrix() {
    use Format::*;
    use HeaderForm::*;
    for f in [Bc1, Bc3, Bc4, Bc5, Bc6hUf16, Bc7] {
        for srgb in [false, true] {
            // Explicit dx10 is always DX10.
            assert_eq!(resolve_header(f, srgb, Dx10), ResolvedHeader::Dx10);
            // No legacy form for BC6H/BC7 or any _SRGB, whatever is asked.
            if srgb || matches!(f, Bc6hUf16 | Bc7) {
                assert_eq!(
                    resolve_header(f, srgb, Legacy),
                    ResolvedHeader::Dx10,
                    "{f:?} srgb={srgb}"
                );
                assert_eq!(
                    resolve_header(f, srgb, Auto),
                    ResolvedHeader::Dx10,
                    "{f:?} srgb={srgb}"
                );
            }
        }
    }
    assert_eq!(resolve_header(Bc3, false, Auto), ResolvedHeader::Legacy);
    assert_eq!(resolve_header(Bc5, false, Auto), ResolvedHeader::Dx10);
    assert_eq!(resolve_header(Bc5, false, Legacy), ResolvedHeader::Legacy);
    assert_eq!(resolve_header(Bc1, false, Dx10), ResolvedHeader::Dx10);
    assert_eq!(legacy_fourcc(Bc7), None);
    assert_eq!(legacy_fourcc(Bc6hUf16), None);
}

#[test]
fn dxgi_values() {
    let all = [
        (Format::Bc1, false, 71),
        (Format::Bc1, true, 72),
        (Format::Bc3, false, 77),
        (Format::Bc3, true, 78),
        (Format::Bc4, false, 80),
        (Format::Bc4, true, 80),
        (Format::Bc5, false, 83),
        (Format::Bc5, true, 83),
        (Format::Bc6hUf16, false, 95),
        (Format::Bc7, false, 98),
        (Format::Bc7, true, 99),
    ];
    for (f, s, v) in all {
        assert_eq!(dxgi_format(f, s), v, "{f:?} srgb={s}");
    }
}

#[test]
fn level_size_rounds_up_to_whole_blocks() {
    assert_eq!(level_size(Format::Bc1, 1, 1), 8);
    assert_eq!(level_size(Format::Bc7, 2, 3), 16);
    assert_eq!(level_size(Format::Bc1, 5, 5), 4 * 8);
    assert_eq!(level_size(Format::Bc5, 8, 4), 2 * 16);
    assert_eq!(level_size(Format::Bc4, 16384, 16384), 4096 * 4096 * 8);
}

#[test]
fn non_square_chain_all_formats_header_consistent() {
    for f in [
        Format::Bc1,
        Format::Bc3,
        Format::Bc4,
        Format::Bc5,
        Format::Bc6hUf16,
        Format::Bc7,
    ] {
        let header = resolve_header(f, false, HeaderForm::Auto);
        let d = DdsDesc {
            width: 37,
            height: 5,
            format: f,
            srgb: false,
            header,
        };
        let levels = chain(f, 37, 5);
        let b = dds_bytes(&d, &levels).unwrap();
        let off = if header == ResolvedHeader::Dx10 {
            148
        } else {
            128
        };
        assert_eq!(u(&b, 28), 6, "{f:?} mip count for 37x5");
        assert_eq!(
            u(&b, 20) as usize,
            level_size(f, 37, 5),
            "{f:?} linear size is mip 0"
        );
        assert_eq!(u(&b, 76), 32, "pixel format size");
        assert_eq!(u(&b, 80), DDPF_FOURCC);
        assert!(
            b[88..108].iter().all(|&x| x == 0),
            "RGB bit count and masks must be zero"
        );
        assert!(
            b[108 + 4..128].iter().all(|&x| x == 0),
            "caps2..4 and reserved2 must be zero"
        );
        assert_eq!(u(&b, 24), 1, "{f:?} dwDepth");
        assert_payload(&b, off, &levels);
    }
}

#[test]
fn single_level_has_no_mip_flags() {
    let d = DdsDesc {
        width: 8,
        height: 8,
        format: Format::Bc7,
        srgb: false,
        header: ResolvedHeader::Dx10,
    };
    let b = dds_bytes(&d, &[vec![0; level_size(Format::Bc7, 8, 8)]]).unwrap();
    assert_eq!(u(&b, 8) & DDSD_MIPMAPCOUNT, 0);
    assert_eq!(u(&b, 108), DDSCAPS_TEXTURE);
    assert_eq!(u(&b, 28), 1);
}

#[test]
fn atomic_write_replaces_and_leaves_no_temp() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("sub/x.dds");
    write_atomic(&p, b"one").unwrap();
    write_atomic(&p, b"two").unwrap();
    assert_eq!(std::fs::read(&p).unwrap(), b"two");
    assert_eq!(std::fs::read_dir(p.parent().unwrap()).unwrap().count(), 1);
}

#[test]
fn atomic_write_overwrites_preexisting_file_and_cleans_up_on_error() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("y.dds");
    std::fs::write(&p, b"old contents that are longer").unwrap();
    write_atomic(&p, b"new").unwrap();
    assert_eq!(std::fs::read(&p).unwrap(), b"new");

    // Target is a directory: rename must fail and the temp file must not be left behind.
    let target = d.path().join("dir.dds");
    std::fs::create_dir(&target).unwrap();
    assert!(write_atomic(&target, b"x").is_err());
    let leftovers: Vec<_> = std::fs::read_dir(d.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "temp left behind: {leftovers:?}");
}

fn desc(w: u32, h: u32, format: Format, srgb: bool, header: ResolvedHeader) -> DdsDesc {
    DdsDesc {
        width: w,
        height: h,
        format,
        srgb,
        header,
    }
}

#[test]
fn full_chain_len_counts_to_one_by_one() {
    assert_eq!(full_chain_len(1, 1), 1);
    assert_eq!(full_chain_len(256, 64), 9);
    assert_eq!(full_chain_len(37, 5), 6);
    assert_eq!(full_chain_len(16384, 1), 15);
}

#[test]
fn partial_chain_is_allowed_but_levels_are_checked() {
    // A chain cut short of 1x1 is accepted (the mip count follows `levels`); only an over-long one is not.
    let d = desc(256, 64, Format::Bc7, false, ResolvedHeader::Dx10);
    let levels = chain(Format::Bc7, 256, 64);
    let b = dds_bytes(&d, &levels[..3]).unwrap();
    assert_eq!(u(&b, 28), 3);
    assert_payload(&b, 148, &levels[..3]);
}

#[test]
fn empty_levels_is_an_error() {
    let d = desc(8, 8, Format::Bc7, false, ResolvedHeader::Dx10);
    assert_eq!(dds_bytes(&d, &[]), Err(DdsError::NoLevels));
}

#[test]
fn zero_dimension_is_an_error() {
    let d = desc(0, 8, Format::Bc7, false, ResolvedHeader::Dx10);
    assert_eq!(
        dds_bytes(&d, &[vec![]]),
        Err(DdsError::ZeroDimension {
            width: 0,
            height: 8
        })
    );
}

#[test]
fn more_levels_than_the_full_chain_is_an_error() {
    let d = desc(256, 64, Format::Bc7, false, ResolvedHeader::Dx10);
    let mut levels = chain(Format::Bc7, 256, 64);
    levels.push(vec![0; 16]); // a tenth level below 1x1
    assert_eq!(
        dds_bytes(&d, &levels),
        Err(DdsError::TooManyLevels {
            levels: 10,
            max: 9,
            width: 256,
            height: 64
        })
    );
}

#[test]
fn wrong_level_length_is_an_error() {
    let d = desc(256, 64, Format::Bc7, false, ResolvedHeader::Dx10);
    let full = chain(Format::Bc7, 256, 64);
    for (i, delta) in [(0usize, -16i64), (4, 16), (8, -1), (2, 1)] {
        let mut levels = full.clone();
        let expected = levels[i].len();
        let actual = (expected as i64 + delta) as usize;
        levels[i].resize(actual, 0);
        assert_eq!(
            dds_bytes(&d, &levels),
            Err(DdsError::LevelSize {
                level: i,
                expected,
                actual
            }),
            "level {i}"
        );
    }
    // A level the size of the previous mip (an off-by-one-mip stitching bug).
    let mut levels = full.clone();
    levels[1] = levels[0].clone();
    assert!(matches!(
        dds_bytes(&d, &levels),
        Err(DdsError::LevelSize { level: 1, .. })
    ));
}

#[test]
fn legacy_header_without_a_legacy_form_is_an_error_not_a_panic() {
    for (f, srgb) in [
        (Format::Bc7, false),
        (Format::Bc7, true),
        (Format::Bc6hUf16, false),
        (Format::Bc1, true),
        (Format::Bc3, true),
    ] {
        let d = desc(4, 4, f, srgb, ResolvedHeader::Legacy);
        assert_eq!(
            dds_bytes(&d, &[vec![0; level_size(f, 4, 4)]]),
            Err(DdsError::NoLegacyForm { format: f, srgb }),
            "{f:?} srgb={srgb}"
        );
    }
}

#[test]
fn parallel_writes_into_one_directory_do_not_collide() {
    let dir = tempfile::tempdir().unwrap();
    let n = 16usize;
    std::thread::scope(|s| {
        for t in 0..n {
            let dir = dir.path();
            s.spawn(move || {
                for round in 0..20usize {
                    let name = format!("out{t}.dds");
                    let payload = vec![(t * 7 + round) as u8; 4096 + t];
                    write_atomic(&dir.join(&name), &payload).unwrap();
                }
            });
        }
    });
    let mut names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names.len(), n, "no stray temp files: {names:?}");
    for t in 0..n {
        let b = std::fs::read(dir.path().join(format!("out{t}.dds"))).unwrap();
        assert_eq!(b, vec![(t * 7 + 19) as u8; 4096 + t], "writer {t}");
    }
}

#[test]
fn atomic_write_to_a_read_only_target_is_an_io_error_and_leaves_no_temp() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("ro.dds");
    std::fs::write(&p, b"keep").unwrap();
    let mut perms = std::fs::metadata(&p).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&p, perms.clone()).unwrap();
    let r = write_atomic(&p, b"new");
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    std::fs::set_permissions(&p, perms).unwrap();
    assert!(r.is_err(), "replacing a read-only file must fail");
    assert_eq!(std::fs::read(&p).unwrap(), b"keep");
    let tmps = std::fs::read_dir(d.path())
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")
        })
        .count();
    assert_eq!(tmps, 0);
}
