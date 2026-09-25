#![allow(dead_code)]

extern crate alloc;

use ckb_idl_derive::{CkbInnerWitness, CkbWitness, CkbWitnessUnion};
use ckb_idl_export::document_for;

#[derive(CkbInnerWitness)]
struct SecpAuthorization {
    signature: [u8; 65],
}

#[derive(CkbInnerWitness)]
struct MultisigAuthorization {
    threshold: u16,
}

#[derive(CkbWitnessUnion)]
enum Authorization {
    #[witness(tag = 7)]
    Secp(SecpAuthorization),
    #[witness(tag = 42)]
    Multisig(MultisigAuthorization),
}

#[derive(CkbWitness)]
struct Witness {
    #[witness(type = "blake2b_hash")]
    digest: [u8; 32],
    #[witness(union, description = "Authorization method")]
    authorization: Authorization,
}

#[test]
fn exports_nested_union_variants() {
    let document = document_for::<Witness>();
    let fields = &document["interfaces"][0]["fields"];
    let digest = &fields[0];
    let auth = &fields[1];
    assert_eq!(document["idl_version"], "0.1.0");
    assert_eq!(digest["type"], "blake2b_hash");
    assert_eq!(digest["wire_type"], "bytes_fixed_32");
    assert_eq!(auth["type"], "union");
    assert_eq!(auth["variants"][0]["tag"], 7);
    assert_eq!(auth["variants"][0]["fields"][0]["name"], "signature");
    assert_eq!(auth["variants"][1]["tag"], 42);
    assert_eq!(auth["variants"][1]["fields"][0]["type"], "uint16");
}

#[test]
fn export_creates_parent_directories() {
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("nested/artifacts/idl.json");
    ckb_idl_export::export_to_path::<Witness>(&output).unwrap();
    assert!(output.is_file());
}
