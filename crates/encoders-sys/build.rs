use std::path::PathBuf;

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let v = root.join("vendor");
    let bc7e = v.join("ispc-objs/bc7e");
    let tc = v.join("ispc-objs/ispctc");

    let header = std::fs::read_to_string(bc7e.join("bc7e_ispc.h"))
        .expect("vendor/ispc-objs/bc7e/bc7e_ispc.h");
    let patched = header.contains("m_exact_opaque_alpha");
    println!("cargo::rustc-check-cfg=cfg(sdc_bc7e_patched)");
    if patched {
        println!("cargo::rustc-cfg=sdc_bc7e_patched");
    }

    let mut b = cc::Build::new();
    b.cpp(true)
        .std("c++17")
        .flag("/EHsc")
        .define("NOMINMAX", None)
        .define("NDEBUG", None)
        .define("SDC_BC7E_PATCHED", if patched { "1" } else { "0" })
        .include(v.join("bc7enc_rdo"))
        .include(v.join("ispctc"))
        .include(&bc7e)
        .include(&tc)
        .file(root.join("shim/sdc_shim.cpp"))
        .file(v.join("bc7enc_rdo/rgbcx.cpp"))
        .file(v.join("ispctc/ispc_texcomp.cpp"));
    for s in ["", "_sse2", "_sse4", "_avx", "_avx2"] {
        b.object(bc7e.join(format!("bc7e{s}.obj")));
        b.object(tc.join(format!("kernel{s}.obj")));
    }
    b.compile("sdc_encoders");

    println!("cargo::rerun-if-changed=shim/sdc_shim.cpp");
    println!("cargo::rerun-if-changed=vendor");
}
