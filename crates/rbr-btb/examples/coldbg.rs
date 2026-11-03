use rbr_btb::{scene, track, xfile};
use rbr_formats::rbz;
fn main() {
    let folder = std::env::args().nth(1).unwrap();
    let t = track::Track::open(std::path::Path::new(&folder)).unwrap();
    let objs = t.objects().unwrap();
    let maps = t.surface_maps().unwrap();
    let path = rbr_formats::stage_files::in_dir(&t.folder, "Objects.zip").unwrap();
    let mut zip = rbz::open(&path).unwrap();
    let mut seen = std::collections::HashSet::new();
    for o in objs.iter().filter(|o| o.index == 322 || o.index==0) {
        if !seen.insert(o.file.clone()) { continue; }
        let bytes = scene::read_entry(&mut zip, &format!("Objects/{}.x", o.file)).unwrap();
        let file = xfile::parse(&bytes).unwrap();
        let root = *file.objects_of_type("Mesh").first().unwrap();
        let m = xfile::Mesh::decode(root, &file).unwrap();
        let tex = m.materials.as_ref().and_then(|m| m.materials.first()).and_then(|m| m.texture.clone());
        let mut minuv = [1e9f32; 2]; let mut maxuv = [-1e9f32; 2];
        for u in m.uv_sets.first().into_iter().flatten() { for i in 0..2 { minuv[i]=minuv[i].min(u[i]); maxuv[i]=maxuv[i].max(u[i]); } }
        println!("{:?} {:?} nuv={}", m.decl.iter().map(|d| d.elements.clone()).collect::<Vec<_>>(), m.fvf.iter().map(|f| f.code).collect::<Vec<_>>(), m.uv_sets.len());
        for d in &m.decl { println!("decl {:?} {}", d.elements, d.data.len()); }
        for f in &m.fvf { println!("fvf {} {:?}", f.code, &f.data[..8.min(f.data.len())]); }
        println!("pos0 {:?} uv0 {:?}", &m.positions[..2], &m.uv_sets[0][..3]);
        println!("{} tex={:?} nmat={} uv {:?}..{:?}", o.file, tex, m.materials.as_ref().map_or(0,|m|m.materials.len()), minuv, maxuv);
    }
    for (i, t) in maps.textures.iter().enumerate().take(5) { println!("MAP{i} {t:?}"); }
    println!("{:?}", maps.textures.iter().position(|t| t.as_deref().is_some_and(|t| t.contains("wb_real"))));
}
