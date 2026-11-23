//! DiRT Rally's surface codes to RBR surface ids. A collision cell names its surfaces with four-character codes, a
//! trailing `+` marking a driven one (`DRT+`); `audio/materials.xml`'s closing comment lists them with their sound and
//! force-feedback class, `wet_materials_remap.xml` the wet code each dry one becomes. The game declares no grip of its
//! own (its car physics files are encrypted), so a code takes the RBR surface of its kind by name, as the AC
//! converter's names do (`notes/dirt-rally.md`); ids from `data/physics-lsp-fields.md`, dry, their weather from the
//! stage's `mat.ini` rows ([`weathers`]).

/// The kind of ground a code stands for, before the stage's own default is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// An RBR surface id.
    Id(u8),
    /// No surface of its own (`DEFA`, `DEFF`): the stage's road surface.
    Default,
}

/// The RBR surface of `code` (`DRT+`, `GVO`, the wet `DTW`); `None` for a code this table does not know.
pub fn rbr(code: &str) -> Option<Surface> {
    let code = code.trim_end_matches(['+', '_', ' ', '\0']);
    let id = match dry(code) {
        "DEFA" | "DEFF" => return Some(Surface::Default),
        // Gravel by its depth: light on the road, medium and heavy loose, banks and traps deeper.
        "GLD" => 17,
        "GMD" | "GVO" | "GM2" | "GLE" => 21,
        "GHD" | "GD2" => 13,
        "GBK" | "GTP" => 1,
        // Rock among gravel roads (Greece), loose and coarse: never tarmac on a gravel stage.
        "ROK" | "RBK" => 49,
        "SND" | "SIL" | "SDC" | "SDT" => 9,
        "DRT" | "DTA" => 133,
        "DBK" | "DLE" => 141,
        "MUD" | "MDL" | "MDB" | "MRW" => 149,
        "GRS" | "GSO" | "GSL" | "GWL" => 113,
        "TSD" | "TRC" | "AST" => 37,
        "TRD" | "TRO" | "COB" => 69,
        "TLE" => 45,
        "CON" | "CPO" | "CNR" | "CPR" | "CWP" | "RBL" | "RRL" | "RRM" | "MRD" | "MET" => 65,
        "WDB" | "WDS" => 85,
        // Snow: packed snow and ice is the road (snow on ice), loose snow lies on gravel by its depth.
        "SWC" | "ICS" | "ICR" => 117,
        "SWS" | "SST" | "SNO" | "DSN" | "DSC" => 101,
        "SNB" => 102,
        "SWD" | "DSD" => 103,
        "WATR" | "DWTR" => 157,
        "WALL" | "GATE" | "PLAS" | "RUBB" | "DRUB" | "CONE" | "TYRE" | "FENC" | "BBND" | "VBND" => 64,
        _ => return None,
    };
    Some(Surface::Id(id))
}

/// The dry code a wet one stands for (`wet_materials_remap.xml`, read the other way); others unchanged.
fn dry(code: &str) -> &str {
    match code {
        "DTW" => "DRT",
        "GWT" => "GRS",
        "GXO" => "GSO",
        "GWO" => "GVO",
        "GHW" => "GHD",
        "GLW" => "GLD",
        "GMW" => "GMD",
        "GBW" => "GBK",
        "GVW" => "GLE",
        "RKW" => "ROK",
        "MDW" => "MUD",
        "MBW" => "MDB",
        "TSW" => "TSD",
        "TRW" => "TRD",
        "CNW" => "CON",
        "CRW" => "COB",
        "ATW" => "AST",
        "SDW" => "SND",
        other => other,
    }
}

/// A surface id in dry, damp and wet weather: a `_Dry` gravel, tarmac, grass or dirt id's next two
/// (`rbr_ac_tracks::btb::surfaces::weathers`); snow, water and the others stay, as their ids are depths, not weather.
pub fn weathers(id: u8) -> [u8; 3] {
    match id {
        101..=103 | 117..=119 | 129..=139 | 157 => [id; 3],
        _ => rbr_ac_tracks::btb::surfaces::weathers(id),
    }
}

/// The stage's `physics=`: snow where over half of the drive lies on snow ids, else gravel where over half lies on
/// gravel or dirt, else tarmac. `along` pairs a surface id with the length of road it covers.
pub fn physics(along: impl IntoIterator<Item = (u8, f64)>) -> &'static str {
    let (mut snow, mut loose, mut all) = (0., 0., 0.);
    for (id, length) in along {
        all += length;
        if matches!(id, 101..=103 | 117..=119) {
            snow += length;
        } else if matches!(id, 1..=23 | 33..=35 | 41..=43 | 49..=51 | 133..=151) {
            loose += length;
        }
    }
    if snow * 2. > all {
        "snow"
    } else if loose * 2. > all {
        "gravel"
    } else {
        "tarmac"
    }
}

/// The stage's road surface for [`Surface::Default`] by its `physics=`.
pub fn default_for(physics: &str) -> u8 {
    match physics {
        "tarmac" => 37,
        "snow" => 117,
        _ => 17,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_map_by_kind_and_wet_codes_by_their_dry_one() {
        assert_eq!(rbr("DRT+"), Some(Surface::Id(133)));
        assert_eq!(rbr("DTW"), rbr("DRT"));
        assert_eq!(rbr("GLD+"), Some(Surface::Id(17)));
        assert_eq!(rbr("ROK+"), Some(Surface::Id(49)));
        assert_eq!(rbr("TSD+"), Some(Surface::Id(37)));
        assert_eq!(rbr("SWC+"), Some(Surface::Id(117)));
        assert_eq!(rbr("DEFA"), Some(Surface::Default));
        assert_eq!(rbr("XYZ+"), None);
    }

    #[test]
    fn depths_are_not_weather() {
        assert_eq!(weathers(37), [37, 38, 39]);
        assert_eq!(weathers(101), [101; 3]);
        assert_eq!(weathers(117), [117; 3]);
        assert_eq!(weathers(129), [129; 3]);
        assert_eq!(weathers(64), [64; 3]);
        assert_eq!(default_for("snow"), 117);
        assert_eq!(physics([(117, 6.), (17, 4.)]), "snow");
        assert_eq!(physics([(49, 6.), (37, 4.)]), "gravel");
        assert_eq!(physics([(37, 6.), (133, 4.)]), "tarmac");
    }
}
