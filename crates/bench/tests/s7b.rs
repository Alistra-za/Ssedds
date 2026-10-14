use sdc_bench::{
    corpus::{bench_root, HalfImage},
    s7b,
};

#[test]
fn box_down_rgba16f_averages_in_float() {
    let h = |v: f32| half::f16::from_f32(v).to_bits();
    let img = HalfImage {
        width: 2,
        height: 2,
        px: vec![
            [h(1.0), 0, 0, h(1.0)],
            [h(3.0), 0, 0, h(1.0)],
            [h(0.0), 0, 0, h(1.0)],
            [h(4.0), 0, 0, h(1.0)],
        ],
    };
    let d = s7b::box_down_rgba16f(&img);
    assert_eq!((d.width, d.height), (1, 1));
    assert_eq!(d.px[0], [h(2.0), 0, 0, h(1.0)]);
}

#[test]
#[ignore = "spike S7b: runs nvcompress on BC7 and BC6H levels (minutes); run with --ignored"]
fn s7b_nvtt_dds_input_and_bc6h_per_level() {
    let work = tempfile::tempdir().unwrap();
    let r = s7b::run(&bench_root(), work.path()).unwrap();
    println!("{r}");
    assert!(r.pass, "{r}");
}
