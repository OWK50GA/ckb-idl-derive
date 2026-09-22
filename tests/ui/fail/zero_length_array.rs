use ckb_idl_derive::CkbWitness;

#[derive(CkbWitness)]
struct Witness {
    empty: [u8; 0],
}

fn main() {}
