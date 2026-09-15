use ckb_idl_derive::CkbWitness;

#[derive(CkbWitness)]
struct MyWitness {
    #[witness(required = true)]
    extra: Option<Vec<u8>>,
}

fn main() {}
