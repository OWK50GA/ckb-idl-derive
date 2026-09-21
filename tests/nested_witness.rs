use ckb_idl_derive::CkbInnerWitness;
use ckb_idl_types::WitnessFields;

#[derive(CkbInnerWitness, Debug, PartialEq)]
struct Authorization {
    signature: [u8; 3],
    unlock_after_ms: u64,
}

#[derive(CkbInnerWitness, Debug, PartialEq)]
struct Envelope {
    authorization: Authorization,
    nonce: u16,
}

#[test]
fn nested_inner_witness_decodes_a_complete_buffer() {
    let mut wire = Vec::new();
    wire.extend_from_slice(&[0xaa, 0xbb, 0xcc]);
    wire.extend_from_slice(&42_u64.to_le_bytes());
    wire.extend_from_slice(&7_u16.to_le_bytes());

    let mut cursor = 0;
    let decoded = Envelope::decode_fields(&wire, &mut cursor)
        .expect("a complete nested witness should decode");

    assert_eq!(
        decoded,
        Envelope {
            authorization: Authorization {
                signature: [0xaa, 0xbb, 0xcc],
                unlock_after_ms: 42,
            },
            nonce: 7,
        }
    );
    assert_eq!(cursor, wire.len());

    let inner_fields = Authorization::idl_fields();
    assert_eq!(inner_fields.len(), 2);
    assert_eq!(inner_fields[0].name, "signature");
    assert_eq!(inner_fields[0].idl_type, "bytes_fixed_3");
    assert_eq!(inner_fields[1].name, "unlock_after_ms");
    assert_eq!(inner_fields[1].idl_type, "uint64");
}
