fn main() {
    // This build script exists solely to ensure Cargo sets OUT_DIR for the
    // crate under test, which is required by the CkbWitness derive macro when
    // it writes idl.json.
}
