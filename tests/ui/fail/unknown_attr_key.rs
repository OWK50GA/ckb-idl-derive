use ckb_idl_derive::CkbWitness;

#[derive(CkbWitness)]
struct MyWitness {
    #[witness(foo = "bar")]
    sig: [u8; 65],
}

fn main() {}
