extern crate alloc;

use ckb_idl_derive::{CkbInnerWitness, CkbWitness};

#[derive(CkbInnerWitness)]
struct Authorization {
    signature: [u8; 65],
    unlock_after_ms: u64,
}

#[derive(CkbWitness)]
struct Witness {
    nonce: u16,
    authorization: Authorization,
}

fn main() {}
