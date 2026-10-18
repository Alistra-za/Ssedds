//! The shipped bc7e levels are byte-identical to the objects that produced the goldens. Any change to the
//! vendored bc7e source or objects (a new patch, a rebuilt object) must keep every existing level's output
//! for every flag combination; only a level added behind a new flag may encode differently.
//!
//! The hashes were recorded with the vendored objects at ISPC 1.31.0, bc7enc_rdo b943862 + patch 0001, over
//! four deterministic 64x64 images (opaque colour, cutout, smooth alpha, normal-like) at every level, with
//! and without perceptual weighting and the exact-opaque flag. Run with `SDC_RECORD_LEVEL_HASHES=1` to print
//! the current hashes when a recorded change is intended (needs a DECISIONS note).

use encoders_sys::{
    bc7e_compress_blocks, bc7e_compress_blocks_tuned, gather_blocks, Bc7Level, Bc7Params, Bc7Tune,
    BC7E_EXACT_OPAQUE_NATIVE,
};
use sha2::{Digest, Sha256};

const LEVELS: [Bc7Level; 7] = [
    Bc7Level::Ultrafast,
    Bc7Level::Veryfast,
    Bc7Level::Fast,
    Bc7Level::Basic,
    Bc7Level::Slow,
    Bc7Level::Veryslow,
    Bc7Level::Slowest,
];

/// [level][perceptual][exact_opaque] over the four images concatenated.
const RECORDED: [[[&str; 2]; 2]; 7] = [
    [
        [
            "ebb74007643dac6b2498480949120947d454c6c058503abd9d3324af7ecd497e",
            "d93524f7a50a201e968cf0b3f6bcbfbdb04a31bff295b25d253ade23dc39802c",
        ],
        [
            "e7405c2618ed7fa9e17c83771e1ba299bf7595709dc9a0dbac335a2c275b8b95",
            "e4eb4593508b03ee50f386a3722ee04d06141c23ec8a5ff16d21872890aef07f",
        ],
    ],
    [
        [
            "74e052168167698546f0d589dbfd483f5eded1ac7ac58899d45e4c9408beb23d",
            "d49f282b040ad97583f5581931703a0d930fde83ff45c6591a3b26631f7eaac2",
        ],
        [
            "ab7289e128f395979a66ba17a8aa8d704cef1d5ee12a7b4bcbf714faa94e1442",
            "d07247904c16e00d3f191b84278f6abb7b76f814c1bd8787c99b83a0795a9703",
        ],
    ],
    [
        [
            "5e910681863c7bc3cc8f066cef6abaea3fb5481f1595b85b906506949c67067c",
            "91c1f1c9c6915eccfed0318012585fadd17ecd6bcc943ece977e7ef7be84c943",
        ],
        [
            "ab7289e128f395979a66ba17a8aa8d704cef1d5ee12a7b4bcbf714faa94e1442",
            "d07247904c16e00d3f191b84278f6abb7b76f814c1bd8787c99b83a0795a9703",
        ],
    ],
    [
        [
            "ab2e35326356b88a6d61327ab9e8a59446e02ed97a4d1f52eb1bc9d93e67aa14",
            "b1016424ca3b33e6fcb08e720cac423dde7656629a4d2a42b9a26b399c63f013",
        ],
        [
            "8b0eb583854a4111e8f2817ccd2635bf6c42459be85b1d22560cd76661b708a5",
            "2599c4478144e576973381f21c390ce18d1d84e55f26d08f1325c865bb9ba005",
        ],
    ],
    [
        [
            "20ec1ac26bb0dbbaf586a3e579209e74d22116e41e59d6a0e908e71f43c30d33",
            "eeb64b77317c2b61032b29fc1b47698378109f6730521ce0f73d97e193f7dd76",
        ],
        [
            "b5092b1398e2a0112b962a0aea4d250585aff0249e3bece4c967d887bea89556",
            "9e81f2e1b73cf856a94644abeba672ad1fefc866d17f402fe483f9dbdfc024a6",
        ],
    ],
    [
        [
            "00b79d5499674261f452bfbb992c9d5a59ceca9f48a1f1e806a6e7f3119e5fb5",
            "9953bb3b4128bd6d7b3783c5102d91f7d93b47d79094b0cf2c406098cc1301fb",
        ],
        [
            "f2416583ef2ec4d328d60214dd02d5b214d43ed38efa84614bb3e4fc6ef88208",
            "d09f720f6416db003c731fce267e4efcc2cb85ba00e2337b501094ef7b59988c",
        ],
    ],
    [
        [
            "cb91d191729b657d7c7441306921894717e963cf3606ac67714930cd9bc6da14",
            "6080d6797adc5f7336b804e5ed3d5921698695f13e93d875365790ff0b5548c2",
        ],
        [
            "2c7cc21d963a4c9aa54996642a9db55cba88f1fc3a9a97091e8d47a002dbf26f",
            "9ebe10f76f9211103f8099781d9985708eb49cd3f92f46751ab6bbb28990ba6e",
        ],
    ],
];

/// A small deterministic generator (xorshift) so the images never depend on a crate.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 32) as u32
    }
    fn byte(&mut self) -> u8 {
        (self.next() & 255) as u8
    }
}

const W: u32 = 64;

fn image(f: impl Fn(u32, u32, &mut Rng) -> [u8; 4]) -> Vec<[u8; 4]> {
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);
    (0..W * W).map(|i| f(i % W, i / W, &mut r)).collect()
}

/// Opaque colour: two gradients, a sharp edge and noise.
fn colour() -> Vec<[u8; 4]> {
    image(|x, y, r| {
        let edge = if (x / 9 + y / 7) % 3 == 0 { 90 } else { 0 };
        [
            (x * 4).min(255) as u8,
            (y * 4).min(255) as u8,
            (128 + edge + (r.byte() % 24) as u32).min(255) as u8,
            255,
        ]
    })
}

/// Cutout: colour with alpha 0 or 255 and an anti-aliased ring.
fn cutout() -> Vec<[u8; 4]> {
    image(|x, y, r| {
        let dx = x as f64 - 31.5;
        let dy = y as f64 - 31.5;
        let d = (dx * dx + dy * dy).sqrt();
        let a = ((24.0 - d) * 64.0).clamp(0.0, 255.0) as u8;
        [200 - (x as u8 / 2), 60 + (y as u8), r.byte(), a]
    })
}

/// Smooth alpha: a soft blob over varying colour.
fn smooth() -> Vec<[u8; 4]> {
    image(|x, y, _| {
        let dx = x as f64 - 20.0;
        let dy = y as f64 - 40.0;
        let a = (255.0 * (-(dx * dx + dy * dy) / 600.0).exp()) as u8;
        [(x * 3) as u8, (255 - y * 2) as u8, 128, a]
    })
}

/// Normal-like: unit vectors around +Z with ripples, alpha 255.
fn normal() -> Vec<[u8; 4]> {
    image(|x, y, _| {
        let nx = (x as f64 * 0.3).sin() * 0.4;
        let ny = (y as f64 * 0.21).cos() * 0.4;
        let nz = (1.0 - nx * nx - ny * ny).max(0.0).sqrt();
        let q = |v: f64| ((v * 0.5 + 0.5) * 255.0).round() as u8;
        [q(nx), q(ny), q(nz), 255]
    })
}

fn hash(level: Bc7Level, perceptual: bool, exact_opaque: bool) -> String {
    let p = Bc7Params {
        level,
        perceptual,
        exact_opaque,
    };
    let mut h = Sha256::new();
    for img in [colour(), cutout(), smooth(), normal()] {
        let blocks = gather_blocks(&img, W, W);
        let mut out = vec![0u8; blocks.len()];
        bc7e_compress_blocks(&blocks, &mut out, p);
        h.update(&out);
    }
    hex::encode(h.finalize())
}

#[test]
fn every_shipped_level_is_byte_identical_to_the_recorded_objects() {
    let record = std::env::var_os("SDC_RECORD_LEVEL_HASHES").is_some();
    let mut diffs = vec![];
    for (li, level) in LEVELS.iter().enumerate() {
        for perceptual in [false, true] {
            for exact in [false, true] {
                if exact && !BC7E_EXACT_OPAQUE_NATIVE {
                    continue;
                }
                let got = hash(*level, perceptual, exact);
                if record {
                    println!("{li} {perceptual} {exact} {got}");
                }
                let want = RECORDED[li][perceptual as usize][exact as usize];
                if got != want {
                    diffs.push(format!(
                        "{level:?} perceptual={perceptual} exact={exact}: {got}"
                    ));
                }
            }
        }
    }
    if record {
        return;
    }
    assert!(
        diffs.is_empty(),
        "bc7e output changed for shipped levels (goldens would change):\n{}",
        diffs.join("\n")
    );
}

#[test]
fn tuning_with_no_overrides_matches_the_level() {
    let p = Bc7Params {
        level: Bc7Level::Veryslow,
        perceptual: false,
        exact_opaque: false,
    };
    let img = colour();
    let blocks = gather_blocks(&img, W, W);
    let mut a = vec![0u8; blocks.len()];
    let mut b = vec![0u8; blocks.len()];
    bc7e_compress_blocks(&blocks, &mut a, p);
    bc7e_compress_blocks_tuned(&blocks, &mut b, p, Bc7Tune::default());
    assert_eq!(a, b);
}
