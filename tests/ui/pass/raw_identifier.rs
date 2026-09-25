extern crate alloc;

use ckb_idl_derive::CkbWitness;

#[derive(CkbWitness)]
struct Witness {
    r#type: u8,
}

fn main() {}
