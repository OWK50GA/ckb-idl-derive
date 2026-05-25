use ckb_idl_derive::CkbWitness;

/// Witness for a time-locked secp256k1 lock script.
///
/// The lock unlocks a cell only if:
///   1. The signature is valid for the owner's public key (stored in args).
///   2. The current block timestamp is >= `unlock_time`.
///
/// `unlock_time` is a Unix timestamp encoded as a little-endian u64.
/// It is placed in the witness rather than args so that the same script
/// bytecode can be reused for cells with different unlock times.
#[derive(CkbWitness)]
pub struct Witness {
    #[witness(description = "65-byte secp256k1 ECDSA signature (r || s || v)")]
    pub signature: [u8; 65],

    #[witness(description = "Unix timestamp (u64 LE) before which the cell cannot be spent")]
    pub unlock_time: u64,
}
