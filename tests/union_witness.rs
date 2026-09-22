use ckb_idl_derive::{CkbInnerWitness, CkbWitnessUnion};
use ckb_idl_types::WitnessUnion;

#[derive(CkbInnerWitness, Debug, PartialEq)]
struct SecpAuthorization {
    recovery_id: u8,
}

#[derive(CkbInnerWitness, Debug, PartialEq)]
struct MultisigAuthorization {
    threshold: u16,
}

#[derive(CkbWitnessUnion, Debug, PartialEq)]
enum Authorization {
    #[witness(tag = 7)]
    Secp(SecpAuthorization),
    #[witness(tag = 42)]
    Multisig(MultisigAuthorization),
}

#[test]
fn union_uses_explicit_tags_for_metadata_and_decoding() {
    let variants = Authorization::idl_variants();
    assert_eq!(variants[0].type_id, 7);
    assert_eq!(variants[1].type_id, 42);
    assert_eq!((variants[0].fields)()[0].name, "recovery_id");
    assert_eq!((variants[1].fields)()[0].name, "threshold");

    let mut secp_wire = 7_u32.to_le_bytes().to_vec();
    secp_wire.push(3);
    let mut cursor = 0;
    assert_eq!(
        Authorization::decode_union(&secp_wire, &mut cursor).unwrap(),
        Authorization::Secp(SecpAuthorization { recovery_id: 3 })
    );
    assert_eq!(cursor, secp_wire.len());

    let mut multisig_wire = 42_u32.to_le_bytes().to_vec();
    multisig_wire.extend_from_slice(&2_u16.to_le_bytes());
    let mut cursor = 0;
    assert_eq!(
        Authorization::decode_union(&multisig_wire, &mut cursor).unwrap(),
        Authorization::Multisig(MultisigAuthorization { threshold: 2 })
    );
    assert_eq!(cursor, multisig_wire.len());
}
