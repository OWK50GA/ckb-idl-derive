// Each witness struct lives in its own module. The macro emits
// `const _CKB_WITNESS_IDL_PATH: &str` in the same scope as the struct,
// so module isolation prevents name collisions when multiple structs are
// defined in the same crate.
//
// In a real project each of these would typically be its own crate
// (one lock script = one crate = one idl.json). Here we use modules
// to demonstrate multiple IDL shapes in a single example crate.

// ── 1. Standard single-signature lock (secp256k1) ────────────────────────────
//
// The most common CKB lock script pattern. The witness carries a 65-byte
// ECDSA signature. The public key is stored in the lock script args, not
// the witness, so it does not appear here.
pub mod secp256k1_lock {
    use ckb_idl_derive::CkbWitness;

    #[derive(CkbWitness)]
    pub struct Witness {
        #[witness(description = "65-byte ECDSA signature (r || s || v) over the transaction hash")]
        pub signature: [u8; 65],
    }
}

// ── 2. 2-of-3 multisig lock ──────────────────────────────────────────────────
//
// A threshold multisig lock where any 2 of 3 participants must sign.
// Each signer provides their own 65-byte signature.
pub mod multisig_lock {
    use ckb_idl_derive::CkbWitness;

    #[derive(CkbWitness)]
    pub struct Witness {
        #[witness(description = "First signer's ECDSA signature")]
        pub sig_0: [u8; 65],

        #[witness(required = false, description = "Second signer's ECDSA signature")]
        pub sig_1: [u8; 65],

        #[witness(required = false, description = "Third signer's ECDSA signature")]
        pub sig_2: [u8; 65],
    }
}

// ── 3. Schnorr / RGB++ style lock ────────────────────────────────────────────
//
// A lock script using Schnorr signatures, as used in RGB++ and some
// Bitcoin-compatible CKB lock designs.
pub mod schnorr_lock {
    use ckb_idl_derive::CkbWitness;

    #[derive(CkbWitness)]
    pub struct Witness {
        #[witness(description = "64-byte Schnorr signature (R || s)")]
        pub signature: [u8; 64],

        #[witness(required = false, description = "Compressed public key (33 bytes), if not in args")]
        pub pubkey: [u8; 33],
    }
}

// ── 4. Owner-lock with extra payload ─────────────────────────────────────────
//
// A lock that requires a signature plus an arbitrary extra payload —
// common in scripts that need to pass additional context to the VM
// (e.g. a Merkle proof, a nonce, or a session token).
pub mod owner_lock {
    use ckb_idl_derive::CkbWitness;

    #[derive(CkbWitness)]
    pub struct Witness {
        #[witness(description = "ECDSA signature authorising the spend")]
        pub signature: [u8; 65],

        #[witness(required = false, description = "Optional extra payload passed to the script VM")]
        pub extra: Vec<u8>,
    }
}

// ── 5. HTLC (Hash Time Lock Contract) ────────────────────────────────────────
//
// Can be unlocked by revealing a preimage (hash-lock path) or by providing
// a signature after a timeout (time-lock path).
pub mod htlc_lock {
    use ckb_idl_derive::CkbWitness;

    #[derive(CkbWitness)]
    pub struct Witness {
        #[witness(description = "Signature of the recipient (hash-lock) or sender (timeout)")]
        pub signature: [u8; 65],

        #[witness(required = false, description = "Preimage of the hash lock; absent on the timeout path")]
        pub preimage: Vec<u8>,
    }
}

// ── Print all IDL paths at runtime ───────────────────────────────────────────

pub fn idl_paths() -> [(&'static str, &'static str); 5] {
    [
        ("secp256k1_lock", secp256k1_lock::_CKB_WITNESS_IDL_PATH),
        ("multisig_lock",  multisig_lock::_CKB_WITNESS_IDL_PATH),
        ("schnorr_lock",   schnorr_lock::_CKB_WITNESS_IDL_PATH),
        ("owner_lock",     owner_lock::_CKB_WITNESS_IDL_PATH),
        ("htlc_lock",      htlc_lock::_CKB_WITNESS_IDL_PATH),
    ]
}
