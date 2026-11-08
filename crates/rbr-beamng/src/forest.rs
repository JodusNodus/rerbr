//! A level's forest: `levels/<level>/forest/*.forest4.json` (one JSON object per line: `type`, `pos`,
//! `rotationMatrix`, `scale`), the types' shapes from `art/forest/managedItemData.json` (`shapeFile`). Each item comes
//! back as a [`Static`] of its type's shape.

use crate::json::{self, Json};
use crate::level::Static;
use crate::zipfs::Archive;
use std::collections::HashMap;

/// Every forest item of `level` whose type names a shape.
pub fn items(archive: &mut Archive, level: &str) -> Result<Vec<Static>, String> {
    let lower = format!("levels/{level}/").to_lowercase();
    let names = archive.names();
    let mut shapes: HashMap<String, String> = HashMap::new();
    let data: Vec<&String> = names
        .iter()
        .filter(|n| n.to_lowercase().starts_with(&lower) && n.to_lowercase().ends_with("manageditemdata.json"))
        .collect();
    for file in data {
        let doc = json::parse(&archive.read_text(file)?).map_err(|e| format!("{file}: {e}"))?;
        for (name, item) in doc.members() {
            if let Some(shape) = item.get("shapeFile").and_then(Json::str) {
                let key = item.get("name").and_then(Json::str).unwrap_or(name);
                shapes.insert(key.to_lowercase(), shape.to_owned());
            }
        }
    }
    let mut files: Vec<&String> = names
        .iter()
        .filter(|n| n.to_lowercase().starts_with(&lower) && n.to_lowercase().ends_with(".forest4.json"))
        .collect();
    files.sort();
    let mut out = Vec::new();
    for file in files {
        for o in json::lines(&archive.read_text(file)?).map_err(|e| format!("{file}: {e}"))? {
            let Some(shape) = o.get("type").and_then(Json::str).and_then(|t| shapes.get(&t.to_lowercase())) else {
                continue;
            };
            out.push(Static::placed(shape, o.get("pos"), o.get("rotationMatrix"), o.get("scale")));
        }
    }
    Ok(out)
}
