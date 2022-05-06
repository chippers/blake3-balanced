use std::env::var;

fn main() {
    let opt_level = advanced_rustflags_detection()
        .or_else(|| var("OPT_LEVEL").ok())
        .unwrap_or_default();

    if matches!(opt_level.as_str(), "2" | "3") {
        println!("cargo:rustc-cfg=complex");
    }
}

#[cfg(not(feature = "rustflags"))]
fn advanced_rustflags_detection() -> Option<String> {
    None
}

#[cfg(feature = "rustflags")]
fn advanced_rustflags_detection() -> Option<String> {
    ::rustflags::from_env().find_map(|flag| {
        if let ::rustflags::Flag::Codegen { opt, value } = flag {
            if opt == "opt-level" {
                return value;
            }
        }

        None
    })
}
