fn main() {
    println!("cargo:rustc-link-arg-cdylib=/DELAYLOAD:mesia-probe-delayed.dll");
    println!("cargo:rustc-link-lib=delayimp");
}
