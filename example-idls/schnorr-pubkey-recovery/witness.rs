use ckb_idl_derive::CkbWitness;

/// Witness for a Schnorr signature lock with optional pubkey recovery.
///
/// Used in RGB++ style lock scripts and Bitcoin-compatible CKB locks.
/// The public key can be stored in args (compressed, 33 bytes) or
/// provided inline in the witness for scripts that support key rotation
/// or multi-key derivation without redeploying.
///
/// Args: optionally carry the compressed public key (33 bytes).
/// If args are empty, `pubkey` in the witness is required.
#[derive(CkbWitness)]
pub struct Witness {
    #[witness(description = "64-byte Schnorr signature (R || s)")]
    pub signature: [u8; 64],

    #[witness(
        required = false,
        description = "Compressed secp256k1 public key (33 bytes); required only when not stored in args"
    )]
    pub pubkey: [u8; 33],
}
