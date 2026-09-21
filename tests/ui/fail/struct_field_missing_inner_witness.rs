use ckb_idl_derive::CkbWitness;

// NotAnnotated does NOT have #[derive(CkbInnerWitness)].
// Using it as a field in a CkbWitness struct should fail to compile
// because the generated decode_fields call requires WitnessFields to be
// implemented.
pub struct NotAnnotated {
    pub value: u64,
}

#[derive(CkbWitness)]
struct MyWitness {
    sig: [u8; 65],
    inner: NotAnnotated,
}

fn main() {}
