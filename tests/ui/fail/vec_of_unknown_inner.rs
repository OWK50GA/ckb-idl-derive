use ckb_idl_derive::CkbWitness;
extern crate alloc;

// Vec<String> — String is not a supported inner element type for VecOf.
// It will produce WitnessFields not implemented for String.
#[derive(CkbWitness)]
struct MyWitness {
    items: Vec<String>,
}

fn main() {}
