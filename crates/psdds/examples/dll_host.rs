//! DC25 check for any host: load psdds.dll by path, encode, FreeLibrary, and the DLL must stay mapped because
//! psdds pinned itself (rayon workers are still parked in its code). Exiting with them parked must be safe too.
//! Usage: dll_host <path to psdds.dll>. MIT licence.
#![cfg(windows)]
use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    fn FreeLibrary(module: *mut c_void) -> i32;
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
}

fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
    s.encode_wide().chain(Some(0)).collect()
}

#[repr(C)]
struct Settings([u32; 8]);
#[repr(C)]
struct Buf {
    data: *mut u8,
    len: usize,
}
type Free = unsafe extern "C" fn(Buf);
type Encode = unsafe extern "C" fn(
    *const f32,
    u32,
    u32,
    u32,
    *const Settings,
    *mut Buf,
    *mut u8,
    usize,
) -> i32;

fn main() {
    let path = std::env::args_os()
        .nth(1)
        .expect("usage: dll_host <psdds.dll>");
    // SAFETY: exports are looked up by name and cast to the signatures declared in psdds.h; buffers outlive the calls.
    unsafe {
        let m = LoadLibraryW(wide(&path).as_ptr());
        assert!(!m.is_null(), "LoadLibraryW failed");
        let abi = GetProcAddress(m, c"psdds_abi_version".as_ptr().cast());
        let enc = GetProcAddress(m, c"psdds_encode".as_ptr().cast());
        assert!(!abi.is_null() && !enc.is_null(), "exports missing");
        let abi: unsafe extern "C" fn() -> u32 = std::mem::transmute(abi);
        println!("abi {}", abi());
        let enc: Encode = std::mem::transmute(enc);

        let px: Vec<f32> = (0..64 * 64 * 4).map(|i| (i % 251) as f32 / 250.0).collect();
        // BC7 colour, sRGB off, box filter, full mips, quality 0, alpha kept as data.
        let s = Settings([4, 0, 0, 0, 1, 0, 0, 0]);
        let mut buf = Buf {
            data: std::ptr::null_mut(),
            len: 0,
        };
        let mut err = [0u8; 256];
        let rc = enc(
            px.as_ptr(),
            64,
            64,
            1,
            &s,
            &mut buf,
            err.as_mut_ptr(),
            err.len(),
        );
        assert_eq!(rc, 0, "encode failed: {}", String::from_utf8_lossy(&err));
        println!("encoded {} bytes", buf.len);

        let free = GetProcAddress(m, c"psdds_free".as_ptr().cast());
        assert!(!free.is_null(), "psdds_free missing");
        let free: Free = std::mem::transmute(free);
        free(buf);
        assert!(FreeLibrary(m) != 0, "FreeLibrary failed");
        let name = wide(std::ffi::OsStr::new("psdds.dll"));
        if GetModuleHandleW(name.as_ptr()).is_null() {
            eprintln!("psdds.dll was unmapped after FreeLibrary");
            std::process::exit(1);
        }
    }
    std::thread::sleep(std::time::Duration::from_secs(2));
    println!("ok: psdds.dll stayed mapped; exiting with workers parked");
}
