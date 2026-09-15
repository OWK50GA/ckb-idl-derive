use ckb_idl_derive::CkbWitness;

#[derive(CkbWitness)]
struct MyWitness {
    #[witness(required = false)]
    extra: Option<Vec<u8>>,
    sig: [u8; 65],
}

fn main() {}
