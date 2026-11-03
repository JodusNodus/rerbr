use rbr_btb::{scene, track, xfile};
use rbr_formats::rbz;
fn main() {
    let folder = std::env::args().nth(1).unwrap();
    let want: usize = std::env::args().nth(2).unwrap().parse().unwrap();
    let t = track::Track::open(std::path::Path::new(&folder)).unwrap();
    let objs = t.objects().unwrap();
    let maps = t.surface_maps().unwrap();
    let mats = t.materials().unwrap();
    println!("MAP{want} = {:?}", maps.textures[want]);
    for (i, m) in maps.textures.iter().enumerate() { if m == &maps.textures[want] { println!("  same texture at MAP{i}"); } }
    let path = rbr_formats::stage_files::in_dir(&t.folder, "Objects.zip").unwrap();
    let mut zip = rbz::open(&path).unwrap();
    let mut seen = std::collections::HashSet::new();
    for o in objs.iter().filter(|o| o.collision_model == Some(0)) {
        if !seen.insert(o.file.clone()) { continue; }
        let bytes = scene::read_entry(&mut zip, &format!("Objects/{}.x", o.file)).unwrap();
        let file = xfile::parse(&bytes).unwrap();
        let root = *file.objects_of_type("Mesh").first().unwrap();
        let m = xfile::Mesh::decode(root, &file).unwrap();
        let tex = m.materials.as_ref().and_then(|m| m.materials.first()).and_then(|m| m.texture.clone()).unwrap_or_default();
        let sec = mats.sections.iter().find(|s| s.name.eq_ignore_ascii_case(&tex));
        let d = sec.and_then(|s| s.texture("diffuseTexture")).unwrap_or("-");
        let b = d.rsplit('\\').next().unwrap();
        let idx = maps.textures.iter().position(|t| t.as_deref().is_some_and(|t| t.rsplit('\\').next().unwrap().eq_ignore_ascii_case(b)));
        if idx == Some(want) { println!("{} obj{} di={:?} cm={:?} tech={:?} tex={tex:?} diffuse={d:?} nmats={}", o.file, o.index, o.draw_instanced, o.col_box, sec.map(|s| s.technique.clone()), m.materials.as_ref().map_or(0, |m| m.materials.len())); }
    }
}
