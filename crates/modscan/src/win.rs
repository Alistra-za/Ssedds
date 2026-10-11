//! Win32 helpers: process snapshot, loaded modules, registry, file identity.
use std::path::PathBuf;
use windows::core::w;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};

fn exe_name(e: &PROCESSENTRY32W) -> String {
    let n = e
        .szExeFile
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(e.szExeFile.len());
    String::from_utf16_lossy(&e.szExeFile[..n])
}

fn new_entry() -> PROCESSENTRY32W {
    PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    }
}

/// All running process names, lowercase.
pub fn process_names() -> Vec<String> {
    let mut out = vec![];
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return out;
        };
        let mut e = new_entry();
        if Process32FirstW(snap, &mut e).is_ok() {
            loop {
                out.push(exe_name(&e).to_ascii_lowercase());
                if Process32NextW(snap, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    out
}

/// True when usvfs is injected into this process (we were started through MO2's virtual file system).
pub fn usvfs_loaded() -> bool {
    unsafe { GetModuleHandleW(w!("usvfs_x64.dll")).is_ok() }
}

/// Folder of ModOrganizer.exe if this process was started by MO2.
///
/// The usvfs DLL being loaded ranks first (plan 02 §1.2) but only tells us we are under MO2; the
/// folder still comes from the parent process image, and if the parent is not ModOrganizer.exe
/// (for example a launcher in between) the answer is `None`.
pub fn mo2_exe_dir() -> Option<PathBuf> {
    let _under_usvfs = usvfs_loaded();
    unsafe {
        let me = std::process::id();
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut e = new_entry();
        let mut parent = None;
        if Process32FirstW(snap, &mut e).is_ok() {
            loop {
                if e.th32ProcessID == me {
                    parent = Some(e.th32ParentProcessID);
                    break;
                }
                if Process32NextW(snap, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        let parent = parent?;
        let msnap = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, parent).ok()?;
        let mut m = MODULEENTRY32W {
            dwSize: std::mem::size_of::<MODULEENTRY32W>() as u32,
            ..Default::default()
        };
        let mut found = None;
        // The first module of a process snapshot is its executable image.
        if Module32FirstW(msnap, &mut m).is_ok() {
            let n = m.szExePath.iter().position(|&c| c == 0).unwrap_or(0);
            let p = PathBuf::from(String::from_utf16_lossy(&m.szExePath[..n]));
            if p.file_name()
                .is_some_and(|f| f.eq_ignore_ascii_case("ModOrganizer.exe"))
            {
                found = p.parent().map(PathBuf::from);
            }
        }
        let _ = CloseHandle(msnap);
        found
    }
}

/// HKCU\Software\Mod Organizer Team\Mod Organizer\CurrentInstance.
pub fn current_instance_name() -> Option<String> {
    unsafe {
        let mut buf = [0u16; 260];
        let mut len = std::mem::size_of_val(&buf) as u32;
        let r = RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Mod Organizer Team\\Mod Organizer"),
            w!("CurrentInstance"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len),
        );
        if r.is_err() {
            return None;
        }
        let n = buf.iter().position(|&c| c == 0).unwrap_or(0);
        Some(String::from_utf16_lossy(&buf[..n])).filter(|s| !s.is_empty())
    }
}
