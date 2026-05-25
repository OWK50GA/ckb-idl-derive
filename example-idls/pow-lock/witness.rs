use ckb_idl_derive::CkbWitness;

/// Witness for a proof-of-work lock script.
///
/// The cell can be spent by anyone who provides a valid proof of work:
/// a nonce such that `blake2b(nonce || tx_hash)` has at least `difficulty`
/// leading zero bits. The `difficulty` byte is set in the witness and must
/// match the value stored in the script args.
///
/// This is a contrived example designed to exercise the `u8` and `Vec<u8>`
/// type registry mappings, which are not covered by the other lock scripts
/// in this collection.
///
/// Args: one byte — the required difficulty (number of leading zero bits).
#[derive(CkbWitness)]
pub struct Witness {
    #[witness(description = "Required difficulty level (leading zero bits); must match args[0]")]
    pub difficulty: u8,

    #[witness(description = "Variable-length proof-of-work nonce bytes")]
    pub proof: Vec<u8>,
}
