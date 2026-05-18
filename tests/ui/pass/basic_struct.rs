use ckb_idl_derive::CkbWitness;

#[derive(CkbWitness)]
struct MyWitness {
    sig: [u8; 65],
    pubkey: [u8; 33],
    #[witness(required = false, description = "optional nonce")]
    nonce: u64,
}

fn main() {}
