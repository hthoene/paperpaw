fn main() {
    // C startup stubs add initializers that host plugin preflight rejects.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-arg-cdylib=-nostartfiles");
    }
}
