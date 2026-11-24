//! Placed props: `objects.ens` (the instances of a route) resolved through `route_objecttypes.pssg` (the entities)
//! to the nodes of `objects.pssg`, `route_objects.pssg` or `trees.pssg` that draw them. Both files are plain XML.
//!
//! An instance (`TEMPLATEBASICENTITYINSTANCE` or `TEMPLATEENTITYINSTANCE`, `uri="#name"`) names a
//! `TEMPLATEENTITYREFERENCE` of the same file (`route_objecttypes.pssg#name.max`), which names a `TEMPLATEENTITY` of
//! the types file; each of its `TEMPLATESUBENTITY`s holds a `TEMPLATETRANSFORMABLE` naming a `TEMPLATERENDERABLE`
//! (`objects.pssg#default!4`). The `default` subentity is the intact prop, the `damage` ones the pieces it breaks into.
//! The instance's `TEMPLATETRANSFORM` is 16 floats, row major with row vectors (the last row the translation), in the
//! world's frame. An entity without a `default` renderable (cloth, physics-only) draws nothing.
use crate::install::Route;
use std::collections::{BTreeMap, HashMap};

pub type Mat = [[f32; 4]; 4];

/// One placeable type: the node ids of its `default` renderables, each in the file named.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Entity {
    pub name: String,
    /// `(file, node id)`, `("objects.pssg", "default!4")`.
    pub parts: Vec<(String, String)>,
    /// The physics hulls of the intact prop ([`Shape`]); the `damage` pieces' are left out.
    pub shapes: Vec<Shape>,
}

/// A physics hull's form, in its own axes (y up, centred): `TEMPLATESHAPEBOX` width (x), height (y), length (z);
/// `CYLINDER` and `CAPSULE` along y; `CONE` base radius and full height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hull {
    Box([f32; 3]),
    Cylinder { height: f32, radius: f32 },
    Capsule { height: f32, radius: f32 },
    Sphere { radius: f32 },
    Cone { height: f32, radius: f32 },
}

/// One hull of an entity's intact rigid body (`infMass="1"`, drawn by a `default` renderable).
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    pub hull: Hull,
    /// Row-vector matrix of the hull in the entity's space (`TEMPLATERIGIDBODYSHAPE`'s `TEMPLATETRANSFORM`).
    pub matrix: Mat,
    /// The shape's `props="physparams.xml#sSolidTreeWood"`.
    pub material: String,
    /// The hull's own behaviour impulse class (`bPhantom`, `bLight`, `bHeavy`, `bArmco`...): the prop gives way or
    /// breaks into its `damage` pieces on a hit. None: nothing in the data lets it give way.
    pub impulse: Option<String>,
}

impl Shape {
    /// What DiRT Rally's data keeps solid: a hull with no impulse class (trees, rocks, walls, buildings, stumps) or the
    /// armco's, not vegetation (bushes, `*Vegetation`: the car drives through them).
    pub fn solid(&self) -> bool {
        matches!(self.impulse.as_deref(), None | Some("bArmco")) && !self.material.contains("Vegetation")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    /// Index into [`Layout::entities`].
    pub entity: usize,
    pub matrix: Mat,
}

#[derive(Debug, Default)]
pub struct Layout {
    pub entities: Vec<Entity>,
    pub instances: Vec<Instance>,
    /// Instances that draw nothing and have no hull, by the entity or reference they name: no `default` renderable, or
    /// not an entity of the types file (cloth, links).
    pub skipped: BTreeMap<String, usize>,
}

/// A start tag of an XML text: its name and attributes.
struct Tag<'a> {
    name: &'a str,
    attrs: Vec<(&'a str, &'a str)>,
}
impl<'a> Tag<'a> {
    fn attr(&self, name: &str) -> Option<&'a str> {
        self.attrs.iter().find(|(k, _)| *k == name).map(|(_, v)| *v)
    }
}

/// The start tags of a text in order, each with the text up to the next tag (a `TEMPLATETRANSFORM`'s floats). End tags,
/// comments and the prolog are left out; attribute values hold no `"` or `>` in these files.
fn tags(text: &str) -> impl Iterator<Item = (Tag<'_>, &str)> {
    text.split('<').skip(1).filter_map(|piece| {
        let (inside, after) = piece.split_once('>')?;
        if matches!(inside.as_bytes().first(), Some(b'/' | b'?' | b'!') | None) {
            return None;
        }
        let inside = inside.trim_end_matches('/');
        let (name, mut rest) = inside.split_once(char::is_whitespace).unwrap_or((inside, ""));
        let mut attrs = Vec::new();
        while let Some((key, value)) = rest.split_once("=\"") {
            let (value, next) = value.split_once('"')?;
            attrs.push((key.trim(), value));
            rest = next;
        }
        Some((Tag { name, attrs }, after))
    })
}

fn matrix(text: &str) -> Option<Mat> {
    let f: Vec<f32> = text.split_whitespace().map(str::parse).collect::<Result<_, _>>().ok()?;
    let f: [f32; 16] = f.try_into().ok()?;
    Some(std::array::from_fn(|i| std::array::from_fn(|j| f[4 * i + j])))
}

/// The entities of a `route_objecttypes.pssg` text by id: the `(file, node id)` of each `default` renderable.
fn entities(text: &str) -> HashMap<String, Vec<(String, String)>> {
    let renderables: HashMap<&str, &str> = tags(text)
        .filter(|(t, _)| t.name == "TEMPLATERENDERABLE")
        .filter_map(|(t, _)| Some((t.attr("id")?, t.attr("uri")?)))
        .collect();
    let mut out = HashMap::<String, Vec<(String, String)>>::new();
    let mut current = None;
    for (tag, _) in tags(text) {
        match tag.name {
            "TEMPLATEENTITY" => {
                current = tag.attr("id").map(str::to_owned);
                out.entry(current.clone().unwrap_or_default()).or_default();
            }
            "TEMPLATETRANSFORMABLE" => {
                let uri = tag.attr("uri").and_then(|u| u.strip_prefix('#')).and_then(|r| renderables.get(r));
                let part = uri.and_then(|u| u.split_once('#')).filter(|(_, node)| !node.contains("damage"));
                if let (Some(entity), Some((file, node))) = (&current, part) {
                    let parts = out.entry(entity.clone()).or_default();
                    let part = (file.to_owned(), node.to_owned());
                    if !parts.contains(&part) {
                        parts.push(part);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// The intact hulls of each entity of a `route_objecttypes.pssg` text by id. A `TEMPLATESUBENTITY` holds one
/// `TEMPLATERIGIDBODY` (`infMass="1"`: fixed; the 0 ones are the pieces that fly off), its `TEMPLATERIGIDBODYSHAPE`s (a
/// shape id, a transform and maybe a `TEMPLATEBEHAVIOUR` with an impulse class) and the `TEMPLATETRANSFORMABLE` that
/// names its renderable: a `damage` one is the broken state, left out. The shapes (`TEMPLATESHAPE*`) come by id.
fn shapes(text: &str) -> HashMap<String, Vec<Shape>> {
    let renderables: HashMap<&str, &str> = tags(text)
        .filter(|(t, _)| t.name == "TEMPLATERENDERABLE")
        .filter_map(|(t, _)| Some((t.attr("id")?, t.attr("uri")?)))
        .collect();
    let num = |t: &Tag, k: &str| t.attr(k).and_then(|v| v.parse::<f32>().ok());
    let mut forms = HashMap::<&str, (Hull, String)>::new();
    for (tag, _) in tags(text) {
        let (Some(id), Some(material)) = (tag.attr("id"), tag.attr("props")) else { continue };
        let hull = match tag.name {
            "TEMPLATESHAPEBOX" => num(&tag, "width")
                .zip(num(&tag, "height"))
                .zip(num(&tag, "length"))
                .map(|((w, h), l)| Hull::Box([w, h, l])),
            "TEMPLATESHAPECYLINDER" => {
                num(&tag, "height").zip(num(&tag, "radius")).map(|(height, radius)| Hull::Cylinder { height, radius })
            }
            "TEMPLATESHAPECAPSULE" => {
                num(&tag, "height").zip(num(&tag, "radius")).map(|(height, radius)| Hull::Capsule { height, radius })
            }
            "TEMPLATESHAPESPHERE" => num(&tag, "radius").map(|radius| Hull::Sphere { radius }),
            "TEMPLATESHAPECONE" => num(&tag, "fullHeight")
                .zip(num(&tag, "baseRadius"))
                .map(|(height, radius)| Hull::Cone { height, radius }),
            _ => None,
        };
        if let Some(hull) = hull {
            forms.insert(id, (hull, material.rsplit_once('#').map_or(material, |(_, m)| m).to_owned()));
        }
    }
    let mut out = HashMap::<String, Vec<Shape>>::new();
    let (mut entity, mut fixed) = (None::<String>, false);
    // The open subentity's shapes, and the shape whose transform is awaited.
    let mut open: Option<Vec<(&str, Mat, Option<String>)>> = None;
    let mut awaiting: Option<&str> = None;
    for (tag, text) in tags(text) {
        match tag.name {
            "TEMPLATEENTITY" => {
                entity = tag.attr("id").map(str::to_owned);
                open = None;
            }
            "TEMPLATESUBENTITY" => (open, fixed) = (Some(Vec::new()), false),
            "TEMPLATERIGIDBODY" => fixed = tag.attr("infMass") == Some("1"),
            "TEMPLATERIGIDBODYSHAPE" => awaiting = tag.attr("uri").and_then(|u| u.strip_prefix('#')),
            "TEMPLATETRANSFORM" => {
                if let (Some(shape), Some(m), Some(open)) = (awaiting.take(), matrix(text), open.as_mut()) {
                    open.push((shape, m, None));
                }
            }
            "TEMPLATEBEHAVIOUR" => {
                let class = tag.attr("impulse").and_then(|i| i.rsplit_once('#')).map(|(_, c)| c.to_owned());
                if let Some(last) = open.as_mut().and_then(|o| o.last_mut()) {
                    last.2 = class;
                }
            }
            "TEMPLATETRANSFORMABLE" => {
                let renderable = tag.attr("uri").and_then(|u| u.strip_prefix('#')).and_then(|r| renderables.get(r));
                let intact = renderable.is_some_and(|r| !r.contains("damage"));
                if let (Some(open), Some(entity), true) = (open.take(), &entity, fixed && intact) {
                    for (id, matrix, impulse) in open {
                        if let Some((hull, material)) = forms.get(id) {
                            out.entry(entity.clone()).or_default().push(Shape {
                                hull: *hull,
                                matrix,
                                material: material.clone(),
                                impulse,
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// The instances of an `objects.ens` text resolved against a `route_objecttypes.pssg` text.
pub fn resolve(ens: &str, types: &str) -> Layout {
    let known = entities(types);
    let mut hulls = shapes(types);
    let mut layout = Layout::default();
    // `TEMPLATEENTITYREFERENCE id` -> the entity id it names (`route_objecttypes.pssg#x.max`).
    let mut references = HashMap::<&str, &str>::new();
    let mut slots = HashMap::<&str, usize>::new();
    let mut pending: Option<&str> = None;
    let mut instances: Vec<(&str, Mat)> = Vec::new();
    for (tag, text) in tags(ens) {
        match tag.name {
            "TEMPLATEENTITYREFERENCE" => {
                if let (Some(id), Some(uri)) = (tag.attr("id"), tag.attr("uri")) {
                    references.insert(id, uri.split_once('#').map_or(uri, |(_, e)| e));
                }
            }
            "TEMPLATEBASICENTITYINSTANCE" | "TEMPLATEENTITYINSTANCE" => {
                pending = tag.attr("uri").and_then(|u| u.strip_prefix('#'));
            }
            "TEMPLATETRANSFORM" => {
                if let (Some(uri), Some(m)) = (pending.take(), matrix(text)) {
                    instances.push((uri, m));
                }
            }
            _ => {}
        }
    }
    for (uri, matrix) in instances {
        let entity = references.get(uri).copied().unwrap_or(uri);
        let parts = known.get(entity).filter(|p| !p.is_empty());
        if parts.is_none() && !hulls.contains_key(entity) {
            *layout.skipped.entry(entity.to_owned()).or_default() += 1;
            continue;
        }
        let slot = *slots.entry(entity).or_insert_with(|| {
            let (parts, shapes) = (parts.cloned().unwrap_or_default(), hulls.remove(entity).unwrap_or_default());
            layout.entities.push(Entity { name: entity.to_owned(), parts, shapes });
            layout.entities.len() - 1
        });
        layout.instances.push(Instance { entity: slot, matrix });
    }
    layout
}

/// The props a route places; `Err` when the route has no `objects.ens` or types file.
pub fn read(route: &Route) -> Result<Layout, String> {
    let read =
        |name: &str| std::fs::read_to_string(route.dir.join(name)).map_err(|e| format!("{}/{name}: {e}", route.id()));
    Ok(resolve(&read("objects.ens")?, &read("route_objecttypes.pssg")?))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const TYPES: &str = r##"<?xml version="1.0"?>
<PSSGFILE><PSSGDATABASE>
<TEMPLATEENTITY id="rock.max"><TEMPLATESUBENTITY id="a"><TEMPLATETRANSFORMABLE uri="#rock_default_renderable" sid="x" /></TEMPLATESUBENTITY>
<TEMPLATESUBENTITY id="b"><TEMPLATETRANSFORMABLE uri="#rock_damage_renderable" sid="y" /></TEMPLATESUBENTITY></TEMPLATEENTITY>
<TEMPLATERENDERABLE id="rock_default_renderable" uri="objects.pssg#rock_default_001" />
<TEMPLATERENDERABLE id="rock_damage_renderable" uri="objects.pssg#rock_damage_001" />
<TEMPLATEENTITY id="flag.max"><TEMPLATESUBENTITY id="c"><TEMPLATETRANSFORMABLE uri="#none" sid="z" /></TEMPLATESUBENTITY></TEMPLATEENTITY>
</PSSGDATABASE></PSSGFILE>"##;

    const ENS: &str = r##"<PSSGFILE><PSSGDATABASE>
<TEMPLATEBASICENTITYINSTANCE id="rock1" uri="#rock!0" instance_tag="1">
<TEMPLATETRANSFORM>1 0 0 0 0 1 0 0 0 0 1 0 5 6 7 1 </TEMPLATETRANSFORM>
</TEMPLATEBASICENTITYINSTANCE>
<TEMPLATEENTITYREFERENCE id="rock!0" uri="route_objecttypes.pssg#rock.max" allocAlt="5" />
<TEMPLATEENTITYINSTANCE id="flag1" uri="#flag" instance_tag="2">
<TEMPLATETRANSFORM>1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1 </TEMPLATETRANSFORM>
</TEMPLATEENTITYINSTANCE>
<TEMPLATEENTITYREFERENCE id="flag" uri="route_objecttypes.pssg#flag.max" />
</PSSGDATABASE></PSSGFILE>"##;

    #[test]
    fn default_renderables_resolve_and_the_rest_is_skipped() {
        let layout = resolve(ENS, TYPES);
        assert_eq!(
            layout.entities,
            [Entity {
                name: "rock.max".into(),
                parts: vec![("objects.pssg".into(), "rock_default_001".into())],
                shapes: Vec::new()
            }]
        );
        assert_eq!(layout.instances.len(), 1);
        assert_eq!(layout.instances[0].matrix[3], [5., 6., 7., 1.]);
        assert_eq!(layout.skipped.get("flag.max"), Some(&1));
    }

    /// The sample routes of `RBR_DIRT_RALLY`, none without it.
    pub(crate) fn sample_routes() -> Vec<crate::install::Route> {
        let Some(game) = std::env::var_os("RBR_DIRT_RALLY") else { return Vec::new() };
        let install = crate::install::Install::at(std::path::Path::new(&game)).unwrap();
        ["greece/ghymno_rally_01/route_0", "finland/ouninpohja_rally_01/route_0"]
            .iter()
            .map(|id| install.route(id).unwrap())
            .collect()
    }

    #[test]
    fn every_instance_of_the_sample_routes_resolves_to_a_model_or_a_known_skip() {
        for route in sample_routes() {
            let started = std::time::Instant::now();
            let layout = read(&route).unwrap();
            let mut files = HashMap::new();
            let mut missing = Vec::new();
            for entity in &layout.entities {
                for (file, node) in &entity.parts {
                    let types = files.entry(file.clone()).or_insert_with(|| {
                        let dir =
                            if file.starts_with("route_") { route.dir.clone() } else { route.area_dir().to_owned() };
                        crate::mesh::types(&crate::pssg::Pssg::read(&dir.join(file)).unwrap()).unwrap()
                    });
                    match types.index.get(node) {
                        Some(&at) if !types.meshes[at].is_empty() => {}
                        _ => missing.push(format!("{}: {file}#{node}", entity.name)),
                    }
                }
            }
            let skipped: usize = layout.skipped.values().sum();
            eprintln!(
                "{}: {} instances of {} entities, {skipped} skipped {:?}, {} without a model {missing:?}, {} ms",
                route.id(),
                layout.instances.len(),
                layout.entities.len(),
                layout.skipped,
                missing.len(),
                started.elapsed().as_millis()
            );
            assert!(layout.instances.len() > 10_000 && missing.is_empty(), "{missing:?}");
            // What is skipped is not an entity with a model: cloth, physics-only and links.
            assert!(skipped * 20 < layout.instances.len() + skipped);
            for i in &layout.instances {
                let m = i.matrix;
                assert!(m.iter().flatten().all(|v| v.is_finite()) && m[3][3] == 1. && m[0][3] == 0.);
            }
        }
    }
}
