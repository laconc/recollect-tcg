//! The UniFFI binding generator for this crate. Reads the built `cdylib`'s
//! embedded metadata and emits Swift/Kotlin/Python bindings for the native
//! shells. Run: `cargo run -p recollect-ffi --bin uniffi-bindgen -- generate
//! --library target/<profile>/librecollect_ffi.<ext> --language <lang> --out-dir <dir>`.
fn main() {
    uniffi::uniffi_bindgen_main()
}
