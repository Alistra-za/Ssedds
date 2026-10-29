// NiObjectNET + NiAVObject prefix reader shared by bstrishape.rs and graph.rs
mod builder;
use builder::{Nif, W};
use nif::cursor::Cursor;
use nif::geom::{read_av_head, AvHead};
use nif::header::Header;

#[test]
fn av_head_reads_transform_and_stops_before_the_subclass_fields() {
    let mut f = Nif::new(100);
    let name = f.string("Root"); // named, but the head does not decode it
    // name, 2 extra-data refs, controller, flags, t, R (row triples), scale, collision | then NiNode: 0 children
    let body = W::default()
        .i32(name)
        .u32(2)
        .i32(7)
        .i32(8)
        .i32(-1)
        .u32(14)
        .fs(&[1.0, 2.0, 3.0])
        .fs(&[0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0])
        .f(2.0)
        .i32(-1)
        .u32(0)
        .0;
    f.block("NiNode", body);
    let bytes = f.bytes();
    let h = Header::parse(&bytes).unwrap();
    let mut c = Cursor::new(&bytes, h.offsets()[0]);
    let av = read_av_head(&mut c).unwrap();
    assert_eq!(
        av,
        AvHead {
            transform: [
                [0.0, 2.0, 0.0, 1.0],
                [2.0, 0.0, 0.0, 2.0],
                [0.0, 0.0, 2.0, 3.0]
            ],
        }
    );
    assert_eq!(c.u32().unwrap(), 0, "cursor is at the NiNode child count");
}

#[test]
fn av_head_truncation_is_an_error() {
    let mut f = Nif::new(100);
    f.block("NiNode", W::default().i32(-1).u32(0).0);
    let bytes = f.bytes();
    let h = Header::parse(&bytes).unwrap();
    let end = h.offsets()[0] + h.sizes[0] as usize;
    let mut c = Cursor::new(&bytes[..end], h.offsets()[0]);
    assert!(read_av_head(&mut c).is_err());
}
