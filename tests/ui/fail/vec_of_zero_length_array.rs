use ckb_idl_derive::CkbWitness;

#[derive(CkbWitness)]
struct Witness {
    empty_items: Vec<[u8; 0]>,
}

fn main() {}
