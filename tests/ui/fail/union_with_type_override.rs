use ckb_idl_derive::CkbWitness;

struct Authorization;

#[derive(CkbWitness)]
struct Witness {
    #[witness(union, type = "my-project:authorization")]
    authorization: Authorization,
}

fn main() {}
