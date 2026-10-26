use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::Path,
};
use voxel_assets::{
    generate_boulder, BoulderRecipe, Geometry, Palette, Prefab, Scene, ScenePack, VERSION,
};

type Result<T> = std::result::Result<T, String>;
fn read_ron<T: serde::de::DeserializeOwned>(path: &str) -> Result<T> {
    let text = fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    ron::from_str(&text).map_err(|e| format!("{path}: {e}"))
}
fn write_json(path: &str, value: &impl serde::Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    fs::write(path, bytes).map_err(|e| format!("{path}: {e}"))
}
fn assemble(scene: Scene, mut prefabs: Vec<Prefab>) -> Result<ScenePack> {
    let required: BTreeSet<_> = scene.instances.iter().map(|i| i.prefab.as_str()).collect();
    prefabs.retain(|p| required.contains(p.id.as_str()));
    prefabs.sort_by(|a, b| a.id.cmp(&b.id));
    let shapes = voxel_assets::expand_scene_shapes(
        scene.style.as_ref().unwrap_or(&Default::default()),
        &scene.architecture,
        &scene.shape_instances,
    )?;
    let environment = if shapes.is_empty() {
        None
    } else {
        Some(voxel_assets::compile_environment(
            scene.style.as_ref().unwrap_or(&Default::default()),
            &shapes,
        )?)
    };
    let destructibles = voxel_assets::compile_destructibles(
        scene.style.as_ref().unwrap_or(&Default::default()), &scene.destructibles,
    )?;
    let actor = voxel_assets::compile_actor(scene.style.as_ref().unwrap_or(&Default::default()))?;
    let skins = voxel_assets::compile_skins(scene.style.as_ref().unwrap_or(&Default::default()))?;
    let pack = ScenePack {
        skins,
        actor,
        environment,
        destructibles,
        version: VERSION,
        scene,
        prefabs,
    };
    pack.validate()?;
    Ok(pack)
}
fn run(args: Vec<String>) -> Result<()> {
    let command = args.first().ok_or("expected prefab or scene command")?;
    let mut flags: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut items = args[1..].chunks_exact(2);
    for pair in &mut items {
        if !pair[0].starts_with("--") {
            return Err(format!("invalid argument {}", pair[0]));
        }
        flags
            .entry(pair[0].clone())
            .or_default()
            .push(pair[1].clone());
    }
    if !items.remainder().is_empty() {
        return Err("missing flag value".into());
    }
    let one = |name: &str| -> Result<&str> {
        let values = flags.get(name).ok_or_else(|| format!("missing {name}"))?;
        if values.len() != 1 {
            return Err(format!("expected one {name}"));
        }
        Ok(values[0].as_str())
    };
    match command.as_str() {
        "prefab" => {
            let mut recipe: BoulderRecipe = read_ron(one("--recipe")?)?;
            let palette: Palette = if flags.contains_key("--style") {
                let style: voxel_assets::StyleProfile = read_ron(one("--style")?)?;
                style.validate()?;
                recipe.voxel_size = style.voxel_size;
                let materials = ["stone", "shadow", "trim", "magic"]
                    .iter()
                    .map(|name| {
                        style
                            .materials
                            .iter()
                            .find(|m| m.name == *name)
                            .cloned()
                            .ok_or_else(|| format!("style is missing prefab material {name}"))
                    })
                    .collect::<Result<Vec<_>>>()?;
                Palette { materials }
            } else {
                read_ron(one("--palette")?)?
            };
            recipe.validate()?;
            palette.validate()?;
            let geometry = match one("--mode")? {
                "baked" => Geometry::Baked(generate_boulder(&recipe)?),
                "runtime" => Geometry::Runtime(recipe),
                _ => return Err("mode must be baked or runtime".into()),
            };
            let prefab = Prefab::new(one("--id")?.into(), palette, geometry)?;
            prefab.validate()?;
            write_json(one("--out")?, &prefab)
        }
        "scene" => {
            let mut scene: Scene = read_ron(one("--layout")?)?;
            if let Some(paths) = flags.get("--style") {
                if paths.len() != 1 {
                    return Err("expected one style".into());
                }
                scene.style = Some(read_ron(&paths[0])?);
            }
            if let Some(paths) = flags.get("--scenario") {
                if paths.len() != 1 {
                    return Err("expected one scenario".into());
                }
                let text = fs::read_to_string(&paths[0]).map_err(|e| format!("{}: {e}", paths[0]))?;
                scene.scenario = Some(voxel_assets::Scenario::from_ron(&text).map_err(|e| format!("{}: {e}", paths[0]))?);
            }
            let mut prefabs = vec![];
            for path in flags.get("--prefab").into_iter().flatten() {
                let bytes = fs::read(path).map_err(|e| format!("{path}: {e}"))?;
                prefabs.push(serde_json::from_slice(&bytes).map_err(|e| format!("{path}: {e}"))?);
            }
            let pack = assemble(scene, prefabs)?;
            let output = one("--out")?;
            write_json(output, &pack)?;
            let file = Path::new(output)
                .file_name()
                .unwrap()
                .to_str()
                .ok_or("non-UTF8 pack path")?;
            let rust = format!("// Generated by assetc. The pack is embedded at compile time.\npub const PACK_BYTES: &[u8] = include_bytes!({file:?});\n");
            fs::write(one("--rust-out")?, rust).map_err(|e| e.to_string())
        }
        _ => Err(format!("unknown command {command}")),
    }
}
fn main() {
    if let Err(error) = run(env::args().skip(1).collect()) {
        eprintln!("assetc: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_prefab_reference_is_a_build_error() {
        let scene:Scene = ron::from_str("Scene(id:\"bad\",name:\"bad\",player:\"exosuit\",spawn:(0.0,0.0,0.0),instances:[(prefab:\"missing\",position:(0.0,0.0,0.0))])").unwrap();
        assert!(assemble(scene, vec![])
            .unwrap_err()
            .contains("unknown prefab missing"));
    }
    #[test]
    fn ron_recipes_reject_unknown_fields() {
        assert!(ron::from_str::<BoulderRecipe>("Boulder(raduis:1.0)").is_err());
        let recipe: BoulderRecipe = ron::from_str("Boulder(seed:42,radius:1.2)").unwrap();
        assert_eq!(recipe.seed, 42);
        assert_eq!(recipe.radius, 1.2);
    }
}
