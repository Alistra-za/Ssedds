//! Note FC12 (CN45): the zip's size is estimated from the planned outputs before anything is written, so a zip over
//! the ZIP format's limits is refused up front instead of being skipped after the export.
use engine::zip::{estimate, ZipEstimate, MAX_BYTES, MAX_ENTRIES};

#[test]
fn zip_over_the_limit_blocks_before_the_write() {
    let e = estimate([10u64, 20, 30]);
    assert_eq!(
        e,
        ZipEstimate {
            entries: 3,
            bytes: 60
        }
    );
    assert!(e.fits());
    // 65,535 entries is the EOCD's ZIP64 sentinel: one fewer still fits.
    assert!(estimate(std::iter::repeat_n(1u64, MAX_ENTRIES - 1)).fits());
    assert!(!estimate(std::iter::repeat_n(1u64, MAX_ENTRIES)).fits());
    // 4 GiB of uncompressed outputs (an upper bound on the stored bytes) is over; a byte less fits.
    assert!(estimate([MAX_BYTES - 1]).fits());
    assert!(!estimate([MAX_BYTES / 2, MAX_BYTES / 2 + 1]).fits());
    assert_eq!(MAX_ENTRIES, 65_535);
    assert_eq!(MAX_BYTES, u64::from(u32::MAX));
}
