use ckb_idl_derive::CkbWitnessUnion;
use ckb_idl_derive::CkbInnerWitness;

#[derive(CkbInnerWitness)]
struct Secp256k1Witness { sig: [u8; 65] }

#[derive(CkbWitnessUnion)]
enum MyUnion {
    Secp256k1(Secp256k1Witness, u64),  // two fields — should error
}

fn main() {}
