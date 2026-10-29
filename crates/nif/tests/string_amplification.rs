//! trace 2026-10-28 #1: every block that names one string-table entry shares one decoded copy of it, so a small NIF
//! cannot multiply a large string into gigabytes. Each test compares the name pointers of many rows.
mod builder;
use builder::{Nif, W};
use std::collections::BTreeSet;

const BIG: usize = 100_000;

/// The file with its `BIG`-byte run of `x` (the large string-table entry) turned into 0x80 bytes, which
/// Windows-1252 decodes to three-byte `€`.
fn bytes_0x80(n: &Nif) -> Vec<u8> {
    let mut b = n.bytes();
    let at = b
        .windows(BIG)
        .position(|w| w.iter().all(|&c| c == b'x'))
        .unwrap();
    b[at..at + BIG].fill(0x80);
    b
}

/// 2000 zero-size BSTriShape blocks all start at the footer, whose root count 30 reads as the name index:
/// entry 30 is the large string. Footer: 30 zero roots.
#[test]
fn scan_shares_one_string_across_many_shape_names() {
    let mut n = Nif::new(100);
    for k in 0..30 {
        n.string(&format!("s{k}"));
    }
    assert_eq!(n.string(&"x".repeat(BIG)), 30);
    for _ in 0..2000 {
        n.block("BSTriShape", vec![]);
    }
    n.roots = vec![0; 30];
    let s = nif::scan::scan(&bytes_0x80(&n)).unwrap();
    assert_eq!(s.shapes.len(), 2000);
    let ptrs: BTreeSet<_> = s
        .shapes
        .iter()
        .map(|r| r.head.name.as_deref().map(|x| x.as_ptr()))
        .collect();
    assert_eq!(ptrs.len(), 1, "{} distinct name copies", ptrs.len());
    assert_eq!(
        s.shapes[0].head.name.as_deref().map(str::len),
        Some(3 * BIG)
    );
}

/// 1000 BSXFlags blocks named by the large string: `read_model` keeps one copy.
#[test]
fn read_model_shares_one_string_across_many_extra_names() {
    let mut n = Nif::new(100);
    let big = n.string(&"x".repeat(BIG));
    for _ in 0..1000 {
        n.block("BSXFlags", W::default().i32(big).u32(0).0);
    }
    let m = nif::model::read_model(&bytes_0x80(&n)).unwrap();
    assert_eq!(m.extras.len(), 1000);
    let ptrs: BTreeSet<_> = m
        .extras
        .values()
        .map(|e| e.name.as_deref().map(|x| x.as_ptr()))
        .collect();
    assert_eq!(ptrs.len(), 1, "{} distinct name copies", ptrs.len());
}

/// A NiControllerSequence named `name` with one controlled block per entry of `nodes`, no text keys.
fn sequence(name: i32, nodes: &[i32]) -> Vec<u8> {
    let mut w = W::default().i32(name).u32(nodes.len() as u32).u32(0);
    for &node in nodes {
        w = w
            .i32(-1)
            .i32(-1)
            .u8(0)
            .i32(node)
            .i32(-1)
            .i32(-1)
            .i32(-1)
            .i32(-1);
    }
    w.f(1.0)
        .i32(-1)
        .u32(0)
        .fs(&[1.0, 0.0, 1.0])
        .i32(-1)
        .i32(-1)
        .u16(0)
        .0
}

/// Sequences naming the large string share one copy: two by their controlled block's node (raw bytes), two by
/// their name (decoded). Each names it once, which the old per-sequence budget allowed.
#[test]
fn two_sequences_share_one_string() {
    let mut n = Nif::new(100);
    let big = n.string(&"x".repeat(BIG));
    n.block("NiControllerSequence", sequence(-1, &[big]));
    n.block("NiControllerSequence", sequence(-1, &[big]));
    n.block("NiControllerSequence", sequence(big, &[]));
    n.block("NiControllerSequence", sequence(big, &[]));
    let m = nif::model::read_model(&bytes_0x80(&n)).unwrap();
    assert!(m.problems.is_empty(), "{:?}", m.problems);
    assert_eq!(m.sequences.len(), 4);
    let nodes: BTreeSet<_> = m.sequences[..2]
        .iter()
        .map(|s| s.blocks[0].node.as_deref().map(|x| x.as_ptr()))
        .collect();
    assert_eq!(nodes.len(), 1, "{} distinct node copies", nodes.len());
    let names: BTreeSet<_> = m.sequences[2..]
        .iter()
        .map(|s| s.name.as_deref().map(|x| x.as_ptr()))
        .collect();
    assert_eq!(names.len(), 1, "{} distinct name copies", names.len());
}

/// Fix round 1: 100k text keys naming one 100 KB string share one copy, and the line the controller panel shows
/// decodes at most `DISPLAY_CAP` bytes (about 10 GB of text before), then "…".
#[test]
fn text_key_line_is_capped() {
    let mut n = Nif::new(100);
    let big = n.string(&"x".repeat(BIG));
    let mut seq = sequence(-1, &[]);
    let at = seq.len() - 30; // the text-keys ref, after the weight f32
    seq[at..at + 4].copy_from_slice(&1i32.to_le_bytes());
    n.block("NiControllerSequence", seq);
    let mut tk = W::default().i32(-1).u32(100_000);
    for k in 0..100_000 {
        tk = tk.f(k as f32).i32(big);
    }
    n.block("NiTextKeyExtraData", tk.0);
    let m = nif::model::read_model(&bytes_0x80(&n)).unwrap();
    assert_eq!(m.sequences[0].text_keys.len(), 100_000);
    let line = m.sequences[0].text_keys_line();
    assert!(
        line.len() <= nif::text::DISPLAY_CAP + "…".len(),
        "{}",
        line.len()
    );
    assert!(line.starts_with('€') && line.ends_with('…'));
}

/// Many controlled blocks of one sequence naming the large string read (the copies are shared, so no budget
/// refuses them) and share one copy.
#[test]
fn one_sequence_shares_one_string_across_its_blocks() {
    let mut n = Nif::new(100);
    let big = n.string(&"x".repeat(BIG));
    n.block("NiControllerSequence", sequence(big, &[big; 50]));
    let m = nif::model::read_model(&bytes_0x80(&n)).unwrap();
    assert!(m.problems.is_empty(), "{:?}", m.problems);
    let nodes: BTreeSet<_> = m.sequences[0]
        .blocks
        .iter()
        .map(|cb| cb.node.as_deref().map(|x| x.as_ptr()))
        .collect();
    assert_eq!(nodes.len(), 1, "{} distinct node copies", nodes.len());
}

// ---- Note GB: bytes allocated on the calling thread, so a test can see a per-block decode without timing it.
thread_local! { static ALLOCATED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
struct Counting;
// SAFETY: forwards every call to the system allocator; only adds a thread-local counter.
unsafe impl std::alloc::GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: std::alloc::Layout) -> *mut u8 {
        let _ = ALLOCATED.try_with(|a| a.set(a.get() + l.size()));
        unsafe { std::alloc::System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: std::alloc::Layout) {
        unsafe { std::alloc::System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: std::alloc::Layout, n: usize) -> *mut u8 {
        let _ = ALLOCATED.try_with(|a| a.set(a.get() + n));
        unsafe { std::alloc::System.realloc(p, l, n) }
    }
}
#[global_allocator]
static COUNTING: Counting = Counting;

/// Bytes `f` allocated on this thread (freed bytes still count: it is the total that was made).
fn allocated<R>(f: impl FnOnce() -> R) -> (R, usize) {
    let before = ALLOCATED.with(|a| a.get());
    let r = f();
    (r, ALLOCATED.with(|a| a.get()) - before)
}

/// GB1: 2000 nodes naming the large string. `read_av_head` used to decode it for each (600 MB of copies) and
/// every caller dropped the name; it now reads no name at all.
#[test]
fn av_head_does_not_decode_the_name_per_node() {
    let mut n = Nif::new(100);
    let big = n.string(&"x".repeat(BIG));
    for _ in 0..2000 {
        let body = W::default()
            .i32(big)
            .u32(0)
            .i32(-1)
            .u32(0)
            .fs(&[0.0; 3])
            .fs(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0])
            .f(1.0)
            .i32(-1)
            .0;
        n.block("NiNode", body);
    }
    let bytes = bytes_0x80(&n);
    let h = nif::header::Header::parse(&bytes).unwrap();
    let (_, made) = allocated(|| {
        for &at in h.offsets() {
            let mut c = nif::cursor::Cursor::new(&bytes, at);
            nif::geom::read_av_head(&mut c).unwrap();
        }
    });
    assert!(made < 1 << 20, "{made} bytes allocated reading 2000 heads");
}

/// GB2: 500 BSLightingShaderProperty blocks naming the large string; `decode_lighting` of each shares one copy.
#[test]
fn lighting_shares_one_string_across_many_properties() {
    let mut n = Nif::new(100);
    let big = n.string(&"x".repeat(BIG));
    for _ in 0..500 {
        let body = W::default()
            .u32(0)
            .i32(big)
            .u32(0)
            .i32(-1)
            .u32(0)
            .u32(0)
            .fs(&[0.0; 4])
            .i32(-1)
            .fs(&[0.0; 3])
            .f(1.0)
            .u32(0)
            .fs(&[1.0, 0.0, 80.0])
            .fs(&[1.0; 3])
            .fs(&[1.0; 3])
            .0;
        n.block("BSLightingShaderProperty", body);
    }
    let bytes = bytes_0x80(&n);
    let h = nif::header::Header::parse(&bytes).unwrap();
    // all kept alive at once: a dropped copy's address is reused by the next
    let all: Vec<_> = (0..500)
        .map(|i| nif::props::decode_lighting(&h, &bytes, i).unwrap())
        .collect();
    let ptrs: BTreeSet<_> = all
        .iter()
        .map(|l| l.name.as_deref().map(|x| x.as_ptr()))
        .collect();
    assert_eq!(ptrs.len(), 1, "{} distinct name copies", ptrs.len());
}

/// GB3: 800 NiStringExtraData blocks whose value is the large string; `read_model` keeps one copy of it.
#[test]
fn read_model_shares_one_string_across_many_string_values() {
    let mut n = Nif::new(100);
    let big = n.string(&"x".repeat(BIG));
    for _ in 0..800 {
        n.block("NiStringExtraData", W::default().i32(-1).i32(big).0);
    }
    let m = nif::model::read_model(&bytes_0x80(&n)).unwrap();
    assert_eq!(m.extras.len(), 800);
    let ptrs: BTreeSet<_> = m
        .extras
        .values()
        .map(|e| match &e.value {
            Some(nif::extra::ExtraValue::Str(s)) => s.as_ptr(),
            v => panic!("{v:?}"),
        })
        .collect();
    assert_eq!(ptrs.len(), 1, "{} distinct value copies", ptrs.len());
}

/// GB4: 2000 shapes on one BSLightingShaderProperty whose texture set holds a 50 KB slot path (about 200 MB when
/// each row copies it): the rows share one slot array.
#[test]
fn scan_shares_the_slots_of_one_shader_across_many_shapes() {
    use builder::scenes::{bstri, lsp, texset};
    let mut n = Nif::new(100);
    n.block("BSLightingShaderProperty", lsp(0, 1));
    n.block("BSShaderTextureSet", texset(&[&"a".repeat(50_000)]));
    for _ in 0..2000 {
        n.block("BSTriShape", bstri(0, -1));
    }
    n.roots = vec![];
    let s = nif::scan::scan(&n.bytes()).unwrap();
    assert_eq!(s.shapes.len(), 2000);
    let ptrs: BTreeSet<_> = s
        .shapes
        .iter()
        .map(|r| match &r.shader {
            nif::scan::Shader::Lighting(_, slots) => slots.as_ptr(),
            o => panic!("{o:?}"),
        })
        .collect();
    assert_eq!(ptrs.len(), 1, "{} distinct slot arrays", ptrs.len());
}
