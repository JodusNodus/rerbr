//! Files that cannot be opened, with the reason. An installer's reader would replace the installer line.

use std::path::Path;

use super::{Fail, Source};

pub const INSTALLER: &str = "installers can't be read: extract it with 7-Zip, then add the folder";

pub fn open(path: &Path) -> Option<Result<Box<dyn Source>, Fail>> {
    let name = path.to_string_lossy().to_ascii_lowercase();
    let refused = !path.is_dir() && [".exe", ".msi"].iter().any(|e| name.ends_with(e));
    refused.then(|| Err(INSTALLER.into()))
}
