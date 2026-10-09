use sha2::{Digest, Sha256};
use std::path::Path;

#[test]
fn every_vendored_file_matches_sha256sums() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor");
    let sums = std::fs::read_to_string(root.join("SHA256SUMS")).expect("vendor/SHA256SUMS missing");
    let mut n = 0;
    for line in sums.lines().filter(|l| !l.trim().is_empty()) {
        let (hash, rel) = line
            .split_once("  ")
            .expect("format: <sha256>  <relative path>");
        let bytes = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
        assert_eq!(
            hex::encode(Sha256::digest(&bytes)),
            hash,
            "hash mismatch: {rel}"
        );
        n += 1;
    }
    assert!(n >= 22, "expected at least 22 vendored files, found {n}");
}
