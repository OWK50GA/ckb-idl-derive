extern crate alloc;

use ckb_idl_derive::{CkbInnerWitness, CkbWitness};

#[derive(CkbInnerWitness)]
struct Authorization {
    signature: [u8; 65],
    unlock_after_ms: u64,
}

#[derive(CkbWitness)]
struct Witness {
    authorization: Authorization,
    nonce: u16,
}

fn main() {}
