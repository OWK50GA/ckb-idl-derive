use ckb_idl_derive::CkbWitnessUnion;

#[derive(CkbWitnessUnion)]
enum MyUnion {
    Variant { sig: [u8; 65] },   // named fields — should error
}

fn main() {}
