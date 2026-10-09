use encoders_sys::{active_bc1_model, init, versions, Bc1Model, InitError};

#[test]
fn init_is_once_and_rejects_a_second_model() {
    assert_eq!(init(Bc1Model::Ideal), Ok(()));
    assert_eq!(init(Bc1Model::Ideal), Ok(()));
    assert_eq!(active_bc1_model(), Some(Bc1Model::Ideal));
    assert_eq!(
        init(Bc1Model::Nvidia),
        Err(InitError::ModelMismatch {
            active: Bc1Model::Ideal,
            requested: Bc1Model::Nvidia
        })
    );
    let v = versions();
    assert_eq!(v.bc7enc_rdo, "b943862");
    assert_eq!(v.ispctc, "79ddbc9");
    assert_eq!(v.ispc, "1.31.0");
}
