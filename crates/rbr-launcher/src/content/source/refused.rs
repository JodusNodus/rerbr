//! Files that cannot be opened, with the reason. An installer's reader would replace the installer line.

use std::path::Path;

use super::{Fail, Source};

pub const RAR: &str = "RAR archives can't be read: extract it, then add the folder";
pub const INSTALLER: &str = "installers can't be read: extract it with 7-Zip, then add the folder";

pub fn open(path: &Path) -> Option<Result<Box<dyn Source>, Fail>> {
    let name = path.to_string_lossy().to_ascii_lowercase();
    let ends = |ends: &[&str]| !path.is_dir() && ends.iter().any(|e| name.ends_with(e));
    if ends(&[".rar"]) {
        Some(Err(RAR.into()))
    } else if ends(&[".exe", ".msi"]) {
        Some(Err(INSTALLER.into()))
    } else {
        None
    }
}
