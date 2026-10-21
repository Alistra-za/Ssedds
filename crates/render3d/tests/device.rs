use render3d::device::{Gpu, Target};

#[test]
fn clear_and_readback() {
    let gpu = Gpu::new().unwrap();
    eprintln!("adapter: {} (warp: {})", gpu.adapter, gpu.warp);
    assert!(!gpu.is_removed());
    let t = Target::new(&gpu, 4, 2).unwrap();
    t.clear(&gpu, [1.0, 0.5, 0.0, 1.0]);
    let px = t.readback_display(&gpu, false).unwrap();
    assert_eq!(px.len(), 4 * 2 * 4);
    assert_eq!(&px[0..4], &[255, 128, 0, 255]);
}

#[test]
fn warp_fallback_renders() {
    let gpu = render3d::device::Gpu::new_warp().unwrap();
    assert!(gpu.warp);
    let t = Target::new(&gpu, 2, 2).unwrap();
    t.clear(&gpu, [0.0, 1.0, 0.0, 1.0]);
    assert_eq!(
        &t.readback_display(&gpu, false).unwrap()[0..4],
        &[0, 255, 0, 255]
    );
}

/// The display readback is opaque whatever alpha the target holds.
#[test]
fn display_readback_is_opaque() {
    let gpu = Gpu::new_warp().unwrap();
    let t = Target::new(&gpu, 2, 1).unwrap();
    t.clear(&gpu, [0.25, 0.0, 1.0, 0.5]);
    assert_eq!(
        t.readback_display(&gpu, false).unwrap(),
        [64, 0, 255, 255].repeat(2)
    );
    assert_eq!(t.readback_display(&gpu, true).unwrap()[3], 255);
}

/// Deferred target: RT0 (`color`, RTV|SRV|UAV) + RT1-7 in the capture's formats, and RT0 as a UAV (D3 step 1).
#[test]
fn new_deferred_makes_eight_targets_and_a_main_uav() {
    use render3d::device::MRT_FORMATS;
    let gpu = Gpu::new_warp().unwrap();
    let t = Target::new_deferred(&gpu, 4, 2).unwrap();
    assert!(t.deferred());
    assert_eq!(t.mrt.len(), 8);
    assert!(t.main_uav.is_some());
    assert_eq!(t.rtvs().len(), 8);
    for (i, (tex, _, _)) in t.mrt.iter().enumerate() {
        let mut d = windows::Win32::Graphics::Direct3D11::D3D11_TEXTURE2D_DESC::default();
        unsafe { tex.GetDesc(&mut d) };
        assert_eq!(d.Format, MRT_FORMATS[i].0, "RT{i}");
    }
    assert!(!Target::new(&gpu, 4, 2).unwrap().deferred());
}
