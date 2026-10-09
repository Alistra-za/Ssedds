//! Physical memory size, for the default `--mem-budget` (50 % of RAM, spec §5.9).

/// Fallback when the size cannot be read: 8 GiB.
const FALLBACK: u64 = 8 << 30;

#[cfg(windows)]
mod win {
    #[repr(C)]
    pub struct MemoryStatusEx {
        pub length: u32,
        pub memory_load: u32,
        pub total_phys: u64,
        pub avail_phys: u64,
        pub total_page_file: u64,
        pub avail_page_file: u64,
        pub total_virtual: u64,
        pub avail_virtual: u64,
        pub avail_extended_virtual: u64,
    }

    #[link(name = "kernel32")]
    extern "system" {
        pub fn GlobalMemoryStatusEx(buf: *mut MemoryStatusEx) -> i32;
    }
}

/// Total physical memory in bytes (`GlobalMemoryStatusEx`), or 8 GiB when it cannot be read.
#[cfg(windows)]
pub fn total_ram() -> u64 {
    let mut m = win::MemoryStatusEx {
        length: std::mem::size_of::<win::MemoryStatusEx>() as u32,
        memory_load: 0,
        total_phys: 0,
        avail_phys: 0,
        total_page_file: 0,
        avail_page_file: 0,
        total_virtual: 0,
        avail_virtual: 0,
        avail_extended_virtual: 0,
    };
    // SAFETY: `m` is a correctly sized, writable MEMORYSTATUSEX with `dwLength` set, as the API requires.
    if unsafe { win::GlobalMemoryStatusEx(&mut m) } != 0 && m.total_phys > 0 {
        m.total_phys
    } else {
        FALLBACK
    }
}

/// Total physical memory in bytes; the tool targets Windows only, so other hosts get the fallback.
#[cfg(not(windows))]
pub fn total_ram() -> u64 {
    FALLBACK
}

#[cfg(test)]
mod tests {
    #[test]
    fn total_ram_is_plausible() {
        let r = super::total_ram();
        assert!(r >= 256 << 20, "{r}");
    }
}
