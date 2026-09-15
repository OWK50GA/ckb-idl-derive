use ckb_idl_derive::CkbWitness;

#[derive(CkbWitness)]
struct MyWitness {
    #[witness(required = false)]
    sig: [u8; 65],
}

fn main() {}
