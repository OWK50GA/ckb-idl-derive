use ckb_idl_derive::CkbWitness;

#[derive(CkbWitness)]
struct MyWitness {
    #[witness(required = false)]
    extra: Option<Option<Vec<u8>>>,
}

fn main() {}
