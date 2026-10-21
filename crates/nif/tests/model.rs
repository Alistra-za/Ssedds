mod builder;
use builder::scenes;
use nif::model::read_model;

fn read(f: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/fixtures/{f}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

#[test]
fn shared_alpha_and_set_have_two_users() {
    let m = read_model(&scenes::shared_alpha_and_set()).unwrap(); // shapes "A","B" share alpha 4 and set 3
    let a = m
        .shapes
        .iter()
        .find(|s| s.name.as_deref() == Some("A"))
        .unwrap();
    assert!(m.problems.is_empty(), "{:?}", m.problems);
    assert_eq!(m.alphas[&a.alpha.unwrap()].prop.users.len(), 2);
    let set = m.lighting[&a.shader.unwrap()].texture_set.unwrap();
    assert_eq!(m.texsets[&set].users.len(), 2);
    assert!(m.problems.is_empty(), "{:?}", m.problems);
}

#[test]
fn managed_controllers_are_known() {
    let m = read_model(&scenes::managed_clip()).unwrap();
    assert!(m.managed(7), "flag 0x20 and named by a controlled block");
    assert!(!m.managed(1), "the manager itself is not managed");
    assert!(m.ctrls.contains_key(&7) && m.ctrls.contains_key(&9));
    assert_eq!(m.sequences.len(), 2);
    assert!(m.problems.is_empty(), "{:?}", m.problems);

    let f = read_model(&scenes::free_chain()).unwrap();
    assert_eq!(f.ctrls.keys().copied().collect::<Vec<_>>(), [1, 2]);
    assert!(!f.managed(1) && !f.managed(2));
    assert_eq!(f.interps.len(), 2);
    assert!(f.problems.is_empty(), "{:?}", f.problems);
}

#[test]
fn root_extras_in_list_order_with_null_skipped() {
    let m = read_model(&scenes::root_extras()).unwrap();
    assert_eq!(m.root, Some(7));
    let net = m.root_net.as_ref().unwrap();
    let blocks: Vec<u32> = m.extras_of(net).iter().map(|e| e.block).collect();
    assert_eq!(blocks, [0, 1, 2, 3, 4, 5, 6]);
    assert!(m.problems.is_empty(), "{:?}", m.problems);
}

#[test]
fn fixtures_read_without_problems() {
    for f in [
        "TestNifFile_Skinned_SE.nif",
        "TestNifFile_Furniture_Col_SE.nif",
        "TestNifFile_DeepGraph_SE.nif",
        "TestNifFile_Skinned_Dynamic_SE.nif",
        "TestNifFile_Optimize_LE_to_SE.nif",
    ] {
        let m = read_model(&read(f)).unwrap_or_else(|e| panic!("{f}: {e}"));
        assert!(m.problems.is_empty(), "{f}: {:?}", m.problems);
        assert!(!m.shapes.is_empty(), "{f}");
        assert!(m.root.is_some(), "{f}");
    }
}

#[test]
fn every_scene_reads() {
    for b in scenes::all() {
        assert!(read_model(&b).is_ok());
    }
    assert!(read_model(&[0u8; 8]).is_err());
}

/// v2 C2 (F2): a sequence no manager lists never plays, so the model drops it; with no manager, all stay.
#[test]
fn unlisted_sequence_is_not_live() {
    let b = scenes::managed_clip();
    let h = nif::header::Header::parse(&b).unwrap();
    let mut man = nif::ctrl::read_manager(&h, &b, 1).unwrap();
    man.sequences = vec![2];
    let mut doc = nif::write::NifDoc::open(&b).unwrap();
    doc.set_block(1, nif::clips::encode_manager(&man).unwrap());
    let one = doc.bytes();
    let m = read_model(&one).unwrap();
    assert!(m.problems.is_empty(), "{:?}", m.problems);
    let blocks = |m: &nif::model::NifModel| m.sequences.iter().map(|s| s.block).collect::<Vec<_>>();
    assert_eq!(blocks(&m), [2]);
    assert_eq!(blocks(&read_model(&b).unwrap()), [2, 3]);
    assert_eq!(
        blocks(&read_model(&scenes::managed_pair()).unwrap()),
        [1, 2]
    );
}
