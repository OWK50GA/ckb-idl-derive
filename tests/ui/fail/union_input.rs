use ckb_idl_derive::CkbWitness;

#[derive(CkbWitness)]
union MyUnion {
    a: u8,
    b: u32,
}

fn main() {}
