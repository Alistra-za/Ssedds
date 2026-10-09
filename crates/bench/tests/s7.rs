use sdc_bench::{
    corpus::{bench_root, Rgba8},
    s7,
};

#[test]
fn box_down_averages_2x2_with_rounding() {
    let img = Rgba8 {
        width: 2,
        height: 2,
        px: vec![
            [0, 0, 0, 255],
            [1, 0, 0, 255],
            [1, 0, 0, 255],
            [1, 0, 0, 255],
        ],
    };
    let d = s7::box_down_rgba8(&img);
    assert_eq!((d.width, d.height), (1, 1));
    assert_eq!(d.px[0], [1, 0, 0, 255]); // (0+1+1+1)/4 = 0.75 -> 1
}

#[test]
#[ignore = "spike S7: runs nvcompress ~33 times (CPU BC7 Production, about a minute); run with --ignored"]
fn s7_nvtt_per_mip_feeding() {
    let work = tempfile::tempdir().unwrap();
    let r = s7::run(&bench_root(), work.path()).unwrap();
    println!("{r}");
    assert!(r.pass, "{r}");
}
