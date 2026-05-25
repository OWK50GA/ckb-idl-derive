use ckb_idl_derive::CkbWitness;

/// Witness for a 2-of-2 multisig lock script with replay protection.
///
/// Both parties must sign. The `nonce` is a monotonically increasing counter
/// stored in the cell data and incremented on each spend, preventing replay
/// of a previously valid witness against a new cell with the same lock.
///
/// Args carry the two public key hashes (32 bytes each, concatenated = 64 bytes).
#[derive(CkbWitness)]
pub struct Witness {
    #[witness(description = "Signature from the first co-signer")]
    pub sig_a: [u8; 65],

    #[witness(description = "Signature from the second co-signer")]
    pub sig_b: [u8; 65],

    #[witness(description = "Replay-protection nonce; must match the value stored in cell data")]
    pub nonce: u32,
}
