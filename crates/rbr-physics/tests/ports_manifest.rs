//! Checks `ports.csv`, the manifest of Rust ports: each row's address is unique and sorted, and its module and Rust
//! item exist in `src/`.

use std::fs;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Whether `text` has `fn name(` or `fn name<` (not a longer name that starts with `name`).
fn defines_fn(text: &str, name: &str) -> bool {
    text.match_indices(&format!("fn {name}")).any(|(i, _)| text[i + 3 + name.len()..].starts_with(['(', '<']))
}

/// The source file for a module path such as `car_body/accessors.rs` (or its `mod.rs` form).
fn module_text(dir: &str, module: &str) -> Option<String> {
    let path = root().join(dir).join(module);
    fs::read_to_string(&path).ok().or_else(|| fs::read_to_string(path.with_extension("").join("mod.rs")).ok())
}

#[test]
fn ports_manifest_matches_source() {
    let text = fs::read_to_string(root().join("ports.csv")).expect("ports.csv");
    assert!(!text.contains('\r'), "ports.csv must use LF line endings");
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some("addr,module,rust_item"));

    let mut last = None;
    for (n, line) in lines.enumerate() {
        let row = n + 2;
        let fields: Vec<&str> = line.split(',').collect();
        let [addr, module, item] = fields[..] else {
            panic!("ports.csv line {row}: expected 3 fields: {line}");
        };

        let value = u32::from_str_radix(addr.trim_start_matches("0x"), 16)
            .unwrap_or_else(|_| panic!("ports.csv line {row}: bad address {addr}"));
        assert!(last.is_none_or(|prev| prev < value), "ports.csv line {row}: {addr} is not after the row before");
        last = Some(value);

        assert!(module_text("src", module).is_some(), "ports.csv line {row}: no module src/{module}");

        let (path, name) = item.rsplit_once("::").unwrap_or_else(|| panic!("ports.csv line {row}: item {item}"));
        let module_path = path.replace("::", "/");
        let found = module_text("src", &format!("{module_path}.rs"))
            .or_else(|| module_text("src", &format!("{module_path}/mod.rs")));
        assert!(found.is_some_and(|src| defines_fn(&src, name)), "ports.csv line {row}: {item} is not a fn in src/");
    }
}
