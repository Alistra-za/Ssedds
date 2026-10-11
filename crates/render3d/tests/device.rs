use render3d::device::{Gpu, Target};

#[test]
fn clear_and_readback() {
    let gpu = Gpu::new(None).unwrap();
    eprintln!(
        "adapter: {} (warp: {}, luid: {:02x?})",
        gpu.adapter, gpu.warp, gpu.luid
    );
    assert!(!gpu.is_removed());
    let t = Target::new(&gpu, 4, 2).unwrap();
    t.clear(&gpu, [1.0, 0.5, 0.0, 1.0]);
    let px = t.readback_rgba8(&gpu).unwrap();
    assert_eq!(px.len(), 4 * 2 * 4);
    assert_eq!(&px[0..4], &[255, 128, 0, 255]);
}

#[test]
fn warp_fallback_renders() {
    let gpu = render3d::device::Gpu::new_warp().unwrap();
    assert!(gpu.warp);
    let t = Target::new(&gpu, 2, 2).unwrap();
    t.clear(&gpu, [0.0, 1.0, 0.0, 1.0]);
    assert_eq!(&t.readback_rgba8(&gpu).unwrap()[0..4], &[0, 255, 0, 255]);
}

#[test]
fn prefer_luid_picks_that_adapter() {
    let Some((name, luid)) = render3d::device::hardware_adapter_list().into_iter().next() else {
        eprintln!("no hardware adapter; skipping");
        return;
    };
    let gpu = Gpu::new(Some(luid)).unwrap();
    assert_eq!(gpu.adapter, name);
    assert_eq!(gpu.luid, luid);
    assert!(!gpu.warp);
}

#[test]
fn unknown_luid_falls_back() {
    let gpu = Gpu::new(Some([0xEE; 8])).unwrap();
    assert_ne!(gpu.luid, [0xEE; 8]);
    let t = Target::new(&gpu, 3, 1).unwrap();
    t.clear(&gpu, [0.25, 0.0, 1.0, 0.5]);
    assert_eq!(t.readback_rgba8(&gpu).unwrap(), [64, 0, 255, 128].repeat(3));
}
