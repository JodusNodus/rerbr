//! intel_tex_2's prebuilt library carries a C++ object (its unused ASTC helper) that needs the C++ runtime's
//! exception personality; MSVC links its own, Linux and macOS need it named.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("linux") => println!("cargo::rustc-link-lib=stdc++"),
        Ok("macos") => println!("cargo::rustc-link-lib=c++"),
        _ => {}
    }
}
