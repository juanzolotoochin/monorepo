//! The effect catalog: every file in `kestrel/app/vfx/` by name, with `base`
//! merged in (`docs/effect-definition.md`, "Variants reuse an effect"). The
//! file name is the effect's ID.
//!
//! A file with a `base` is merged as a RON tree (`ron_tree.rs`) over its
//! base's merged tree; the result is printed and parsed by `vfx::parse` like
//! any file, so loading stays strict.
use super::ron_tree::{self, Fields, Value};
use super::{parse, EffectFile};
use bevy::color::{ColorToComponents, LinearRgba, Srgba};
use bevy_sprinkles::asset::{DrawPassMaterial, Gradient, SolidOrGradientColor};
use bevy_sprinkles::prelude::ParticlesAsset;
use std::path::Path;

/// The emitters list, merged by emitter `name` instead of replaced.
const EMITTERS: &str = "particles.emitters";

/// Loads every effect in `sources` (file name without `.ron`, RON text), in
/// order. A file that fails doesn't stop the others: its result is the error,
/// which names the file. A base resolves to another file in `sources`.
pub fn load<'a>(sources: impl IntoIterator<Item = (&'a str, &'a str)>) -> Vec<(String, Result<EffectFile, String>)> {
    let sources: Vec<(&str, &str)> = sources.into_iter().collect();
    let trees: Vec<(&str, Result<Value, String>)> =
        sources.iter().map(|&(name, text)| (name, ron_tree::parse(text).map_err(|e| format!("vfx/{name}.ron: {e}")))).collect();
    sources
        .iter()
        .map(|&(name, text)| {
            let file = match resolve(&trees, name, &mut Vec::new()) {
                // Parsed from the file itself, so errors point into it.
                Ok((_, false)) => parse(name, text),
                Ok((merged, true)) => parse(name, &merged.print()).map_err(|e| format!("{e} (in the file merged with its base)")),
                Err(e) => Err(e),
            };
            (name.to_string(), file)
        })
        .collect()
}

/// The `.ron` files in `dir` as (name, text) pairs, sorted by name: the
/// catalog's sources when reading `kestrel/app/vfx/` from disk.
pub fn read_dir(dir: &Path) -> std::io::Result<Vec<(String, String)>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "ron") {
            let name = path.file_stem().expect("a file name").to_string_lossy().into_owned();
            files.push((name, std::fs::read_to_string(&path)?));
        }
    }
    files.sort();
    Ok(files)
}

/// File `name`'s tree with its base chain merged in and `base` removed, and
/// whether it had a base. `chain` holds the files being resolved, to catch
/// cycles.
fn resolve(trees: &[(&str, Result<Value, String>)], name: &str, chain: &mut Vec<String>) -> Result<(Value, bool), String> {
    if chain.iter().any(|n| n == name) {
        chain.push(name.to_string());
        return Err(format!("vfx/{}.ron: base cycle: {}", chain[0], chain.join(" -> ")));
    }
    let tree = trees.iter().find(|(n, _)| *n == name).map(|(_, tree)| tree).expect("resolving a listed file");
    let mut own = tree.clone()?;
    let fail = |why: String| Err(format!("vfx/{name}.ron: {why}"));
    let base = match own.take_field("base") {
        None => return Ok((own, false)),
        Some(base) => match base.as_str() {
            Some(base) => base,
            None => return fail("base: expected a string, the base effect's file name".into()),
        },
    };
    if !trees.iter().any(|(n, _)| *n == base) {
        return fail(format!("unknown base {base:?}"));
    }
    chain.push(name.to_string());
    let (base_tree, _) = resolve(trees, &base, chain)?;
    chain.pop();
    match merge(base_tree, own, "") {
        Ok(merged) => Ok((merged, true)),
        Err(e) => fail(e),
    }
}

/// `own` over `base`: sections merge field by field, lists and values
/// replace, except the emitters list, which merges by name. A value with
/// another struct or variant name replaces the base's. `path` names the field
/// for errors.
fn merge(base: Value, own: Value, path: &str) -> Result<Value, String> {
    match (base, own) {
        (Value::List(base), Value::List(own)) if path == EMITTERS => merge_emitters(base, own).map(Value::List),
        (Value::Struct { name: base_name, fields: base }, Value::Struct { name, fields }) if base_name == name => {
            Ok(Value::Struct { name, fields: merge_fields(base, fields, path)? })
        }
        (Value::Map(mut base), Value::Map(own)) => {
            for (key, value) in own {
                let label = format!("{path}{{{}}}", key.print().trim_end());
                match base.iter().position(|(k, _)| *k == key) {
                    Some(i) => {
                        let old = std::mem::replace(&mut base[i].1, Value::List(Vec::new()));
                        base[i].1 = merge(old, value, &label)?;
                    }
                    None => base.push((key, value)),
                }
            }
            Ok(Value::Map(base))
        }
        (Value::Struct { .. }, own @ Value::Struct { .. })
        | (Value::Scalar(_), own @ Value::Scalar(_))
        | (Value::List(_), own @ Value::List(_)) => Ok(own),
        (base, own) => Err(format!("{}: {} where the base has {}", shown(path), kind(&own), kind(&base))),
    }
}

fn merge_fields(base: Fields, own: Fields, path: &str) -> Result<Fields, String> {
    match (base, own) {
        (Fields::Named(mut base), Fields::Named(own)) => {
            for (field, value) in own {
                let label = if path.is_empty() { field.clone() } else { format!("{path}.{field}") };
                match base.iter().position(|(f, _)| *f == field) {
                    Some(i) => {
                        let old = std::mem::replace(&mut base[i].1, Value::List(Vec::new()));
                        base[i].1 = merge(old, value, &label)?;
                    }
                    None => base.push((field, value)),
                }
            }
            Ok(Fields::Named(base))
        }
        // `Some(x)` over `Some(y)`, `Standard((...))` over `Standard((...))`.
        (Fields::Tuple(base), Fields::Tuple(own)) if base.len() == own.len() => base
            .into_iter()
            .zip(own)
            .enumerate()
            .map(|(i, (base, own))| merge(base, own, &format!("{path}.{i}")))
            .collect::<Result<_, _>>()
            .map(Fields::Tuple),
        (Fields::Tuple(_), own @ Fields::Tuple(_)) | (Fields::Unit, own @ Fields::Unit) => Ok(own),
        // `()` is an empty section too.
        (Fields::Named(base), Fields::Tuple(own)) if own.is_empty() => Ok(Fields::Named(base)),
        (Fields::Tuple(base), own @ Fields::Named(_)) if base.is_empty() => Ok(own),
        (base, own) => Err(format!("{}: {} where the base has {}", shown(path), fields_kind(&own), fields_kind(&base))),
    }
}

/// Merges emitters by `name`: a matching one merges into the base's, a new
/// name appends.
fn merge_emitters(mut base: Vec<Value>, own: Vec<Value>) -> Result<Vec<Value>, String> {
    let name = |emitter: &Value| emitter.field("name").and_then(Value::as_str);
    for emitter in own {
        let Some(own_name) = name(&emitter) else { return Err(format!("{EMITTERS}: an emitter without a name can't merge")) };
        match base.iter().position(|e| name(e).as_ref() == Some(&own_name)) {
            Some(i) => {
                let old = std::mem::replace(&mut base[i], Value::List(Vec::new()));
                base[i] = merge(old, emitter, &format!("{EMITTERS}[{own_name}]"))?;
            }
            None => base.push(emitter),
        }
    }
    Ok(base)
}

fn shown(path: &str) -> &str {
    if path.is_empty() { "the file" } else { path }
}

fn kind(value: &Value) -> &'static str {
    match value {
        Value::Scalar(_) => "a value",
        Value::List(_) => "a list",
        Value::Map(_) => "a map",
        Value::Struct { fields, .. } => fields_kind(fields),
    }
}

fn fields_kind(fields: &Fields) -> &'static str {
    match fields {
        Fields::Unit => "a name",
        Fields::Tuple(_) => "a tuple",
        Fields::Named(_) => "a section",
    }
}

/// The color fields `particles_to_linear` converts, as `Type.field` (enum
/// variants as `Type::Variant.field`). A test checks this against every color
/// field reachable from `ParticlesAsset`.
#[cfg(test)]
const COLOR_FIELDS: &[&str] = &[
    "GradientStop.color",
    "SolidOrGradientColor::Solid.color",
    "StandardParticleMaterial.attenuation_color",
    "StandardParticleMaterial.base_color",
    "StandardParticleMaterial.emissive",
    "StandardParticleMaterial.specular_tint",
];

/// Effect files write particle colors in sRGB, like every authored color in
/// Kestrel; sprinkles takes linear. Converts every color in `asset` (alpha
/// stays as is). Values above 1 (HDR emissive) convert with the same curve.
pub fn particles_to_linear(asset: &mut ParticlesAsset) {
    fn convert(color: &mut [f32; 4]) {
        let [r, g, b, a] = *color;
        *color = LinearRgba::from(Srgba::new(r, g, b, a)).to_f32_array();
    }
    fn gradient(gradient: &mut Gradient) {
        gradient.stops.iter_mut().for_each(|stop| convert(&mut stop.color));
    }
    for emitter in &mut asset.emitters {
        match &mut emitter.colors.initial_color {
            SolidOrGradientColor::Solid { color } => convert(color),
            SolidOrGradientColor::Gradient { gradient: g } => gradient(g),
        }
        gradient(&mut emitter.colors.color_over_lifetime);
        if let DrawPassMaterial::Standard(material) = &mut emitter.draw_pass.material {
            for color in [&mut material.base_color, &mut material.emissive, &mut material.specular_tint, &mut material.attenuation_color] {
                convert(color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vfx::FILES;
    use bevy::color::Color;
    use bevy::reflect::enums::VariantInfo;
    use bevy::reflect::{TypeInfo, Typed};
    use std::any::TypeId;
    use std::collections::{BTreeSet, HashSet};

    fn one(sources: &[(&str, &str)], name: &str) -> Result<EffectFile, String> {
        load(sources.iter().copied()).into_iter().find(|(n, _)| n == name).unwrap().1
    }

    fn with_stone<'a>(variant: &'a str) -> Vec<(&'a str, &'a str)> {
        let stone = FILES.iter().find(|(n, _)| *n == "impact_stone").unwrap().1;
        vec![("impact_stone", stone), ("variant", variant)]
    }

    #[test]
    fn every_file_round_trips_through_the_tree() {
        for &(name, source) in FILES {
            let tree = ron_tree::parse(source).unwrap_or_else(|e| panic!("{name}: {e}"));
            let printed = tree.print();
            assert_eq!(ron_tree::parse(&printed).unwrap().print(), printed, "{name}");
            // A variant only parses merged with its base (`load`).
            if tree.field("base").is_some() {
                continue;
            }
            let (own, again) = (parse(name, source).unwrap(), parse(name, &printed).unwrap_or_else(|e| panic!("{e}\n{printed}")));
            assert_eq!(ron::to_string(&own.particles).unwrap(), ron::to_string(&again.particles).unwrap(), "{name}");
            assert_eq!(format!("{:?}", own.lights), format!("{:?}", again.lights), "{name}");
        }
    }

    #[test]
    fn the_list_matches_the_folder() {
        // The files are the test's data (BUILD.bazel); the working directory is the workspace.
        let dir = Path::new("kestrel/app/vfx");
        let on_disk: Vec<String> = read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())).into_iter().map(|(n, _)| n).collect();
        let mut listed: Vec<String> = FILES.iter().map(|(n, _)| n.to_string()).collect();
        listed.sort();
        assert_eq!(listed, on_disk, "vfx::FILES must list every file in kestrel/app/vfx/");
    }

    #[test]
    fn a_variant_recolors_one_emitter_and_disables_another() {
        let sources = with_stone(
            r#"(base: "impact_stone", particles: (emitters: [
                (name: "sparks", draw_pass: (material: Standard((base_color: (0.0, 0.0, 1.0, 1.0))))),
                (name: "debris", enabled: false),
            ]))"#,
        );
        let (base, variant) = (one(&sources, "impact_stone").unwrap(), one(&sources, "variant").unwrap());
        let emitters = &variant.particles.emitters;
        assert_eq!(emitters.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["sparks", "debris", "flash"]);
        let DrawPassMaterial::Standard(sparks) = &emitters[0].draw_pass.material else { panic!() };
        let DrawPassMaterial::Standard(base_sparks) = &base.particles.emitters[0].draw_pass.material else { panic!() };
        assert_eq!(sparks.base_color, [0.0, 0.0, 1.0, 1.0]);
        // The rest of the material, and of the emitter, comes from the base.
        assert_eq!(sparks.emissive, base_sparks.emissive);
        assert_eq!(emitters[0].emission.particles_amount, 12);
        assert!(!emitters[1].enabled && emitters[1].emission.particles_amount == 18);
        assert_eq!(variant.lights.len(), 1);
    }

    #[test]
    fn a_new_emitter_appends() {
        let sources = with_stone(
            r#"(base: "impact_stone", particles: (emitters: [
                (name: "smoke", time: (lifetime: 1.0, one_shot: true), emission: (particles_amount: 3)),
            ]))"#,
        );
        let variant = one(&sources, "variant").unwrap();
        let names: Vec<_> = variant.particles.emitters.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["sparks", "debris", "flash", "smoke"]);
    }

    #[test]
    fn sections_merge_lists_and_variants_replace() {
        let sources = with_stone(
            r#"(base: "impact_stone",
                particles: (emitters: [(name: "flash", time: (lifetime: 0.2), emission: (shape: Sphere(radius: 0.5)))]),
                lights: [(color: (0.0, 1.0, 0.0), intensity: 10.0, range: 1.0, radius: 0.1, fade_seconds: 0.5)])"#,
        );
        let variant = one(&sources, "variant").unwrap();
        let flash = &variant.particles.emitters[2];
        // A nested section: lifetime replaced, one_shot from the base.
        assert_eq!(flash.time.lifetime, 0.2);
        assert!(flash.time.one_shot);
        assert!(matches!(flash.emission.shape, bevy_sprinkles::asset::EmissionShape::Sphere { radius } if radius == 0.5));
        // The light list replaces the base's.
        assert_eq!(variant.lights.len(), 1);
        assert_eq!(variant.lights[0].color, [0.0, 1.0, 0.0]);

        // A variant with another variant name replaces the base's whole value.
        let base = "(a: Box(size: 1, depth: 2), b: Some(Box(size: 1)))";
        let merged = merge(ron_tree::parse(base).unwrap(), ron_tree::parse("(a: Sphere(radius: 3), b: Some(Box(depth: 4)))").unwrap(), "");
        assert_eq!(merged.unwrap(), ron_tree::parse("(a: Sphere(radius: 3), b: Some(Box(size: 1, depth: 4)))").unwrap());
    }

    #[test]
    fn bad_bases_and_shapes_are_errors_naming_the_file_and_field() {
        let unknown = one(&with_stone(r#"(base: "impact_metal")"#), "variant").unwrap_err();
        assert_eq!(unknown, r#"vfx/variant.ron: unknown base "impact_metal""#);
        let cycle = load([("a", r#"(base: "b")"#), ("b", r#"(base: "a")"#)]);
        assert_eq!(cycle[0].1.as_ref().unwrap_err(), "vfx/a.ron: base cycle: a -> b -> a");
        let own = load([("a", r#"(base: "a")"#)]);
        assert!(own[0].1.as_ref().unwrap_err().contains("cycle"));
        let shape = one(&with_stone(r#"(base: "impact_stone", particles: (emitters: [(name: "sparks", draw_pass: [1])]))"#), "variant");
        assert_eq!(
            shape.unwrap_err(),
            "vfx/variant.ron: particles.emitters[sparks].draw_pass: a list where the base has a section"
        );
        // The base still loads.
        assert!(one(&with_stone(r#"(base: "nope")"#), "impact_stone").is_ok());
    }

    /// Every `[f32; 4]` (or Bevy color) field reachable from `ParticlesAsset`,
    /// found from the types' reflection info, so a color field added to the
    /// fork (in any enum variant, not just the ones a test file uses) fails
    /// here until `particles_to_linear` converts it.
    #[test]
    fn every_particle_color_field_is_converted() {
        fn walk(info: &'static TypeInfo, label: &str, seen: &mut HashSet<TypeId>, out: &mut BTreeSet<String>) {
            let is_color = |info: &TypeInfo| {
                info.is::<[f32; 4]>() || info.is::<Color>() || info.is::<LinearRgba>() || info.is::<Srgba>()
            };
            let mut visit = |label: String, field: Option<&'static TypeInfo>, seen: &mut HashSet<TypeId>| {
                let field = field.unwrap_or_else(|| panic!("{label}: no type info"));
                if is_color(field) {
                    out.insert(label);
                } else {
                    walk(field, &label, seen, out);
                }
            };
            if !seen.insert(info.type_id()) {
                return;
            }
            let owner = info.type_path_table().short_path();
            match info {
                TypeInfo::Struct(s) => s.iter().for_each(|f| visit(format!("{owner}.{}", f.name()), f.type_info(), seen)),
                TypeInfo::TupleStruct(s) => s.iter().for_each(|f| visit(format!("{owner}.{}", f.index()), f.type_info(), seen)),
                TypeInfo::Tuple(t) => t.iter().for_each(|f| visit(format!("{label}.{}", f.index()), f.type_info(), seen)),
                TypeInfo::List(l) => visit(label.to_string(), l.item_info(), seen),
                TypeInfo::Array(a) => visit(label.to_string(), a.item_info(), seen),
                TypeInfo::Map(m) => {
                    visit(label.to_string(), m.key_info(), seen);
                    visit(label.to_string(), m.value_info(), seen);
                }
                TypeInfo::Set(_) | TypeInfo::Opaque(_) => {}
                TypeInfo::Enum(e) => {
                    for variant in e.iter() {
                        match variant {
                            VariantInfo::Struct(v) => {
                                v.iter().for_each(|f| visit(format!("{owner}::{}.{}", v.name(), f.name()), f.type_info(), seen))
                            }
                            VariantInfo::Tuple(v) => {
                                v.iter().for_each(|f| visit(format!("{owner}::{}.{}", v.name(), f.index()), f.type_info(), seen))
                            }
                            VariantInfo::Unit(_) => {}
                        }
                    }
                }
            }
        }
        let mut found = BTreeSet::new();
        walk(ParticlesAsset::type_info(), "ParticlesAsset", &mut HashSet::new(), &mut found);
        let listed: BTreeSet<String> = COLOR_FIELDS.iter().map(|s| s.to_string()).collect();
        assert_eq!(found, listed, "a particle color field changed: convert it in particles_to_linear and list it in COLOR_FIELDS");

        // And each listed field converts: sRGB 0.5 everywhere reads linear 0.214.
        let (half, linear) = ([0.5; 4], LinearRgba::from(Srgba::new(0.5, 0.5, 0.5, 0.5)).to_f32_array());
        let mut asset = parse("impact_stone", FILES.iter().find(|(n, _)| *n == "impact_stone").unwrap().1).unwrap().particles;
        let emitter = &mut asset.emitters[0];
        let mut gradient = emitter.colors.color_over_lifetime.clone();
        gradient.stops.iter_mut().for_each(|s| s.color = half);
        emitter.colors.color_over_lifetime = gradient.clone();
        emitter.colors.initial_color = SolidOrGradientColor::Solid { color: half };
        let DrawPassMaterial::Standard(m) = &mut emitter.draw_pass.material else { panic!() };
        (m.base_color, m.emissive, m.specular_tint, m.attenuation_color) = (half, half, half, half);
        asset.emitters[1].colors.initial_color = SolidOrGradientColor::Gradient { gradient };
        particles_to_linear(&mut asset);
        let [first, second] = [&asset.emitters[0], &asset.emitters[1]];
        let DrawPassMaterial::Standard(m) = &first.draw_pass.material else { panic!() };
        let SolidOrGradientColor::Gradient { gradient } = &second.colors.initial_color else { panic!() };
        let mut colors = vec![m.base_color, m.emissive, m.specular_tint, m.attenuation_color, first.colors.initial_color.as_solid_color().unwrap()];
        colors.extend(first.colors.color_over_lifetime.stops.iter().chain(&gradient.stops).map(|s| s.color));
        assert!(colors.iter().all(|&c| c == [linear[0], linear[1], linear[2], 0.5]), "{colors:?}");
    }
}
