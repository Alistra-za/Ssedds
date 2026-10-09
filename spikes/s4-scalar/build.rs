fn main() {
    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .flag("/EHsc")
        // The ISPC objects are built with --opt=disable-assertions; match that for the scalar port.
        .define("NDEBUG", None)
        .opt_level(3)
        .file("shim.cpp")
        .compile("s4scalar");
    println!("cargo::rerun-if-changed=shim.cpp");
    println!("cargo::rerun-if-changed=vendor/basisu_bc7e_scalar.cpp");
    println!("cargo::rerun-if-changed=vendor/basisu_bc7e_scalar.h");
}
