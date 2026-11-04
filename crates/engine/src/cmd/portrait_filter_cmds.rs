//! Filter › Portrait retouch: external packages (`manifest.json` + `model.onnx`).
//!
//! The host lives in `photocraft-portrait-filters`. This module only turns a result into new
//! layers. Trained weights are not part of PhotoCraft; see `docs/PORTRAIT_FILTERS.md`.

use photocraft_color::ColorMode;
use photocraft_doc::{Document, Layer, LayerId};
use photocraft_portrait_filters::{
    Catalog, LayerPlacement, MenuWhich, RgbImage, disable_reason, infer, layer_pixels, placement, resolve_directory, runtime_available, scan,
};
use photocraft_raster::Surface;
use serde_json::{Value, json};

use crate::commands::CommandSpec;
use crate::{EngineError, Result, Session};

const MENU: &[&str] = &["Filter", "Portrait retouch"];

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "filter.portraitRetouch.apply",
            label: "Portrait Retouch",
            menu: &[],
            shortcut: None,
            params: r#"{"id":str}"#,
            enabled: enabled_apply,
            run: run_apply,
            journal: true,
        },
        // Aliases for actions recorded against the first three packages. The menu rows themselves
        // come from the scan (`filter.portraitRetouch.pkg.<id>`), so a new package needs no command.
        alias("filter.portraitRetouch.heal", "Heal", enabled_heal, run_heal),
        alias("filter.portraitRetouch.dodgeBurn", "Dodge & Burn", enabled_dodge, run_dodge),
        alias("filter.portraitRetouch.skinTone", "Skin Tone", enabled_skin, run_skin),
        CommandSpec {
            id: "filter.portraitRetouch.all",
            label: "All",
            menu: MENU,
            shortcut: None,
            params: "{}",
            enabled: enabled_all,
            run: |s, _| run_all(s),
            journal: true,
        },
        CommandSpec {
            id: "filter.portraitRetouch.packages",
            label: "Portrait Filter Packages",
            menu: &[],
            shortcut: None,
            params: "{}",
            enabled: |_| Ok(()),
            run: |s, _| Ok(list_json(&catalog_of(s))),
            journal: false,
        },
    ]
}

fn alias(
    id: &'static str,
    label: &'static str,
    enabled: fn(&Session) -> std::result::Result<(), String>,
    run: fn(&mut Session, &Value) -> Result<Value>,
) -> CommandSpec {
    CommandSpec { id, label, menu: &[], shortcut: None, params: "{}", enabled, run, journal: true }
}

fn enabled_apply(s: &Session) -> std::result::Result<(), String> {
    require_rgb(s)?;
    if !runtime_available() {
        return Err("this build does not include ONNX Runtime; rebuild with --features portrait-onnx".into());
    }
    Ok(())
}

fn enabled_heal(s: &Session) -> std::result::Result<(), String> {
    enabled_package(s, "heal")
}
fn enabled_dodge(s: &Session) -> std::result::Result<(), String> {
    enabled_package(s, "dodge_burn")
}
fn enabled_skin(s: &Session) -> std::result::Result<(), String> {
    enabled_package(s, "skin_tone")
}

fn enabled_package(s: &Session, id: &str) -> std::result::Result<(), String> {
    require_rgb(s)?;
    let catalog = catalog_of(s);
    match disable_reason(&catalog, runtime_available(), MenuWhich::Package(id)) {
        Some(reason) => Err(reason),
        None => Ok(()),
    }
}

fn enabled_all(s: &Session) -> std::result::Result<(), String> {
    require_rgb(s)?;
    let catalog = catalog_of(s);
    match disable_reason(&catalog, runtime_available(), MenuWhich::All) {
        Some(reason) => Err(reason),
        None => Ok(()),
    }
}

fn require_rgb(s: &Session) -> std::result::Result<(), String> {
    let doc = s.active().ok_or("no document open")?;
    let layer = doc.active_layer.and_then(|id| doc.doc.layer(id)).ok_or("no active layer")?;
    let surface =
        layer.surface().ok_or_else(|| format!("active layer is {} {} layer, not a pixel layer", layer.content.article(), layer.content.kind_name()))?;
    if surface.format().mode != ColorMode::Rgb {
        return Err("portrait filters need an RGB document".into());
    }
    Ok(())
}

fn catalog_of(s: &Session) -> Catalog {
    match resolve_directory(&s.prefs().plug_ins.portrait_filters_folder) {
        Some(dir) => scan(&dir),
        None => Catalog::unavailable(std::path::PathBuf::new(), "no portrait filters directory"),
    }
}

fn list_json(catalog: &Catalog) -> Value {
    json!({
        "directory": catalog.directory.display().to_string(),
        "runtime": runtime_available(),
        "note": catalog.note,
        "packages": catalog.in_apply_order().into_iter().map(|package| json!({
            "id": package.id,
            "task": package.id,
            "label": package.label,
            "name": package.solo.name,
            "description": package.description,
            "priority": package.priority,
            "version": package.version,
            "model": package.model.display().to_string(),
            "blend": package.solo.blend.label(),
            "opacity": package.solo.opacity,
            "result": package.result.label(),
            "stackedBlend": package.stacked.blend.label(),
            "stackedOpacity": package.stacked.opacity,
            "tile": package.tile.map(|tile| json!({"size": tile.size, "overlap": tile.overlap})),
            "menuId": photocraft_portrait_filters::menu_command_id(&package.id),
        })).collect::<Vec<_>>(),
        "skipped": catalog.skipped.iter().map(|skip| json!({"folder": skip.folder, "reason": skip.reason})).collect::<Vec<_>>(),
    })
}

fn run_apply(s: &mut Session, params: &Value) -> Result<Value> {
    let id =
        params.get("id").and_then(Value::as_str).filter(|id| !id.is_empty()).ok_or_else(|| bad("filter.portraitRetouch.apply", "`id` must be a package id"))?;
    run_ids(s, &[id.to_string()], "Portrait retouch")
}

fn run_heal(s: &mut Session, _: &Value) -> Result<Value> {
    run_ids(s, &["heal".to_string()], "Heal")
}
fn run_dodge(s: &mut Session, _: &Value) -> Result<Value> {
    run_ids(s, &["dodge_burn".to_string()], "Dodge & Burn")
}
fn run_skin(s: &mut Session, _: &Value) -> Result<Value> {
    run_ids(s, &["skin_tone".to_string()], "Skin Tone")
}

fn run_all(s: &mut Session) -> Result<Value> {
    let ids: Vec<String> = {
        let catalog = catalog_of(s);
        if let Some(reason) = disable_reason(&catalog, runtime_available(), MenuWhich::All) {
            return Err(EngineError::Other(reason));
        }
        catalog.in_apply_order().into_iter().map(|package| package.id.clone()).collect()
    };
    run_ids(s, &ids, "Portrait retouch")
}

fn run_ids(s: &mut Session, ids: &[String], label: &str) -> Result<Value> {
    require_rgb(s).map_err(EngineError::Other)?;
    let catalog = catalog_of(s);
    let stacked = ids.len() > 1;
    let (image, alpha) = read_active(s)?;
    let mut planned = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(reason) = disable_reason(&catalog, runtime_available(), MenuWhich::Package(id)) {
            return Err(EngineError::Other(reason));
        }
        let package = catalog.get(id).ok_or_else(|| EngineError::Other(format!("portrait filter package `{id}` is not installed")))?;
        let filtered = infer(package, &image).map_err(|e| EngineError::Other(e.to_string()))?;
        // Difference packages store a mid-gray residual. Linear Light over `image` reconstructs the model RGB.
        let pixels = layer_pixels(package.result, &image, filtered).map_err(|e| EngineError::Other(e.to_string()))?;
        planned.push((placement(&package.solo, &package.stacked, stacked), pixels));
    }
    let layer_ids = s.edit(label, |doc, active| install(doc, active, &alpha, &planned))?;
    if let Some(top) = layer_ids.last().copied() {
        select_only(s, top);
    }
    Ok(json!({
        "layers": layer_ids.iter().map(|id| id.0).collect::<Vec<_>>(),
        "tasks": ids,
    }))
}

fn bad(cmd: &str, msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: cmd.into(), msg: msg.into() }
}

fn read_active(s: &Session) -> Result<(RgbImage, Vec<f32>)> {
    let state = s.active().ok_or(EngineError::NoDocument)?;
    let id = state.active_layer.ok_or_else(|| EngineError::Other("no active layer".into()))?;
    let surface = state.doc.layer(id).and_then(Layer::surface).ok_or_else(|| EngineError::Other("active layer has no pixels".into()))?;
    read_canvas(surface, state.doc.size.width, state.doc.size.height)
}

fn read_canvas(surface: &Surface, width: u32, height: u32) -> Result<(RgbImage, Vec<f32>)> {
    if surface.format().mode != ColorMode::Rgb {
        return Err(EngineError::Other("portrait filters need an RGB document".into()));
    }
    let image = RgbImage::new(width, height).map_err(|e| EngineError::Other(e.to_string()))?;
    let mut image = image;
    let mut alpha = vec![1.0; image.pixel_count()];
    let channels = surface.channels();
    if channels > 5 {
        return Err(EngineError::Other("portrait filters need an RGB document".into()));
    }
    let mut sample = [0.0; 5];
    for y in 0..height {
        for x in 0..width {
            surface.read_pixel(x as i32, y as i32, &mut sample[..channels]);
            let rgba = photocraft_raster::to_rgba(&surface.format(), &sample[..channels]);
            image.set(x, y, [rgba[0], rgba[1], rgba[2]]);
            let i = (y as usize).saturating_mul(width as usize).saturating_add(x as usize);
            if let Some(slot) = alpha.get_mut(i) {
                *slot = rgba[3];
            }
        }
    }
    Ok((image, alpha))
}

fn install(doc: &mut Document, active: &mut Option<LayerId>, alpha: &[f32], planned: &[(LayerPlacement, RgbImage)]) -> Result<Vec<LayerId>> {
    if planned.is_empty() {
        return Err(EngineError::Other("no portrait filter to apply".into()));
    }
    let mut anchor = *active;
    let mut ids = Vec::with_capacity(planned.len());
    for (place, image) in planned {
        let mut layer = Layer::raster(unique_layer_name(doc, &place.name), doc.pixel_format());
        layer.blend = place.blend;
        layer.opacity = place.opacity;
        write_canvas(crate::pixels_mut(&mut layer)?, image, alpha)?;
        let id = crate::layer_place::insert_new(doc, anchor, layer, crate::layer_place::Place::Above);
        anchor = Some(id);
        ids.push(id);
    }
    *active = anchor;
    Ok(ids)
}

fn write_canvas(surface: &mut Surface, image: &RgbImage, alpha: &[f32]) -> Result<()> {
    let format = surface.format();
    let mut px = [0.0; 5];
    for y in 0..image.height {
        for x in 0..image.width {
            let rgb = image.get(x, y);
            let i = (y as usize).saturating_mul(image.width as usize).saturating_add(x as usize);
            let a = alpha.get(i).copied().unwrap_or(1.0);
            let n = photocraft_raster::from_rgba_into(&format, [rgb[0], rgb[1], rgb[2], a], &mut px);
            if n > px.len() {
                return Err(EngineError::Other("pixel format is wider than an RGB image".into()));
            }
            surface.write_pixel(x as i32, y as i32, &px[..n]);
        }
    }
    surface.prune();
    Ok(())
}

fn unique_layer_name(doc: &Document, base: &str) -> String {
    let mut n = 0u32;
    loop {
        let candidate = if n == 0 { base.to_string() } else { format!("{base} {}", n.saturating_add(1)) };
        let taken = doc.walk().iter().any(|(_, _, layer)| layer.name == candidate);
        if !taken || n == u32::MAX {
            return candidate;
        }
        n = n.saturating_add(1);
    }
}

fn select_only(s: &mut Session, id: LayerId) {
    if let Some(state) = s.active_mut() {
        state.selected_layers = vec![id];
        state.active_layer = Some(id);
        state.layer_anchor = Some(id);
        crate::fix_selection(state);
    }
}

#[cfg(test)]
mod tests {
    use photocraft_color::BlendMode;
    use photocraft_portrait_filters::{MenuWhich, ResultKind, RgbImage, disable_reason, layer_pixels, menu_command_id, runtime_available, scan};
    use serde_json::json;

    use super::catalog_of;
    use crate::Session;

    fn rgb(width: u32, height: u32, depth: u32) -> Session {
        let mut session = Session::new();
        session.execute("file.new", json!({"width": width, "height": height, "mode": "rgb", "depth": depth})).unwrap();
        session
    }

    #[test]
    fn commands_sit_in_filter_portrait_retouch() {
        let all = crate::commands::find("filter.portraitRetouch.all").unwrap();
        assert_eq!(all.menu, ["Filter", "Portrait retouch"]);
        assert_eq!(all.label, "All");
        for id in [
            "filter.portraitRetouch.apply",
            "filter.portraitRetouch.heal",
            "filter.portraitRetouch.dodgeBurn",
            "filter.portraitRetouch.skinTone",
            "filter.portraitRetouch.packages",
        ] {
            let spec = crate::commands::find(id).unwrap_or_else(|| panic!("missing {id}"));
            assert!(spec.menu.is_empty(), "{id} is not a static menu row");
        }
    }

    #[test]
    fn missing_packages_and_non_rgb_documents_are_disabled() {
        let mut session = Session::new();
        let err = session.execute("filter.portraitRetouch.heal", json!({})).unwrap_err().to_string();
        assert!(err.contains("no document"), "{err}");
        session.execute("file.new", json!({"width": 4, "height": 4, "mode": "gray", "depth": 8})).unwrap();
        let err = session.execute("filter.portraitRetouch.heal", json!({})).unwrap_err().to_string();
        assert!(err.to_ascii_lowercase().contains("rgb"), "{err}");

        let mut session = rgb(4, 4, 8);
        let err = session.execute("filter.portraitRetouch.heal", json!({})).unwrap_err().to_string();
        assert!(err.contains("not installed"), "{err}");
        let listed = session.execute("filter.portraitRetouch.packages", json!({})).unwrap();
        assert_eq!(listed["runtime"], json!(runtime_available()));
        assert!(listed["packages"].as_array().unwrap().is_empty());
    }

    #[test]
    fn a_present_package_stays_disabled_until_the_runtime_is_linked() {
        let root = std::env::temp_dir().join(format!("pc-portrait-engine-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let heal = root.join("heal");
        std::fs::create_dir_all(&heal).unwrap();
        std::fs::write(heal.join("manifest.json"), r#"{"task":"heal","name":"Heal"}"#).unwrap();
        std::fs::write(heal.join("model.onnx"), b"fixture").unwrap();
        let mut session = rgb(2, 2, 8);
        session.execute("prefs.set", json!({"path": "plugIns.portraitFiltersFolder", "value": root.to_str().unwrap()})).unwrap();
        let catalog = catalog_of(&session);
        assert!(catalog.get("heal").is_some());
        let err = session.execute("filter.portraitRetouch.heal", json!({})).unwrap_err().to_string();
        assert!(err.contains("ONNX Runtime"), "{err}");
        assert!(disable_reason(&scan(&root), false, MenuWhich::Package("heal")).unwrap().contains("ONNX Runtime"));
        let glow = root.join("glow");
        std::fs::create_dir_all(&glow).unwrap();
        std::fs::write(glow.join("manifest.json"), r#"{"id":"glow","label":"Glow","blend":"screen","opacity":0.6,"description":"A fourth filter"}"#).unwrap();
        std::fs::write(glow.join("model.onnx"), b"fixture").unwrap();
        let listed = session.execute("filter.portraitRetouch.packages", json!({})).unwrap();
        let ids: Vec<&str> = listed["packages"].as_array().unwrap().iter().filter_map(|p| p["id"].as_str()).collect();
        assert_eq!(ids, ["heal", "glow"]);
        assert_eq!(listed["packages"][1]["menuId"], json!(menu_command_id("glow")));
        assert_eq!(listed["packages"][0]["result"], json!("absolute"));
        assert_eq!(listed["packages"][1]["result"], json!("absolute"));
        let err = session.execute("filter.portraitRetouch.apply", json!({"id": "glow"})).unwrap_err().to_string();
        assert!(err.contains("ONNX Runtime"), "{err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn results_are_new_layers_with_the_contract_blends() {
        for depth in [8u32, 16, 32] {
            let mut session = rgb(2, 2, depth);
            session
                .edit("fill", |doc, active| {
                    let bounds = doc.bounds();
                    let surface = doc.layer_mut(active.unwrap()).unwrap().surface_mut().unwrap();
                    let px = photocraft_raster::from_rgba(&surface.format(), [0.2, 0.4, 0.6, 1.0]);
                    surface.fill_rect(bounds, &px);
                    Ok(())
                })
                .unwrap();
            let before = session.active().unwrap().doc.layers[0].surface().unwrap().pixel(0, 0);
            let mut heal = RgbImage::new(2, 2).unwrap();
            heal.rgb.fill(0.8);
            let mut dodge = RgbImage::new(2, 2).unwrap();
            dodge.rgb.fill(0.5);
            let mut skin = RgbImage::new(2, 2).unwrap();
            skin.rgb.fill(0.2);
            let planned = vec![
                (super::LayerPlacement { blend: BlendMode::Normal, opacity: 1.0, name: "Heal".into() }, heal),
                (
                    super::LayerPlacement {
                        blend: BlendMode::SoftLight,
                        opacity: photocraft_portrait_filters::DODGE_BURN_OPACITY,
                        name: "Dodge & Burn".into(),
                    },
                    dodge,
                ),
                (super::LayerPlacement { blend: BlendMode::LinearLight, opacity: 1.0, name: "Skin Tone".into() }, skin),
            ];
            let alpha = vec![1.0; 4];
            session.edit("Portrait retouch", |doc, active| super::install(doc, active, &alpha, &planned)).unwrap();
            let doc = &session.active().unwrap().doc;
            assert_eq!(doc.layers.len(), 4, "depth {depth}");
            assert_eq!(doc.layers[1].name, "Heal");
            assert_eq!(doc.layers[1].blend, BlendMode::Normal);
            assert!((doc.layers[1].opacity - 1.0).abs() < 1e-6);
            assert_eq!(doc.layers[2].name, "Dodge & Burn");
            assert_eq!(doc.layers[2].blend, BlendMode::SoftLight);
            assert!((doc.layers[2].opacity - 0.85).abs() < 1e-5);
            assert_eq!(doc.layers[3].name, "Skin Tone");
            assert_eq!(doc.layers[3].blend, BlendMode::LinearLight);
            let healed = doc.layers[1].surface().unwrap().pixel(1, 0);
            assert!((healed[0] - 0.8).abs() < 1.0 / 200.0, "depth {depth}: {healed:?}");
            let original = doc.layers[0].surface().unwrap().pixel(0, 0);
            assert!((original[0] - before[0]).abs() < 1e-5, "the source layer stays put");
            session.execute("edit.undo", json!({})).unwrap();
            assert_eq!(session.active().unwrap().doc.layers.len(), 1, "one undo drops the whole stack");
        }
    }

    #[test]
    fn a_difference_layer_is_mid_gray_and_linear_light_matches_the_filtered_rgb() {
        for depth in [8u32, 16, 32] {
            let mut session = rgb(1, 2, depth);
            session
                .edit("fill", |doc, active| {
                    let surface = doc.layer_mut(active.unwrap()).unwrap().surface_mut().unwrap();
                    let top = photocraft_raster::from_rgba(&surface.format(), [0.2, 0.4, 0.8, 1.0]);
                    let bot = photocraft_raster::from_rgba(&surface.format(), [0.0, 1.0, 0.2, 1.0]);
                    surface.write_pixel(0, 0, &top);
                    surface.write_pixel(0, 1, &bot);
                    Ok(())
                })
                .unwrap();
            let mut source = RgbImage::new(1, 2).unwrap();
            source.set(0, 0, [0.2, 0.4, 0.8]);
            source.set(0, 1, [0.0, 1.0, 0.2]);
            let mut filtered = source.clone();
            filtered.set(0, 0, [0.6, 0.0, 0.8]);
            let layer = layer_pixels(ResultKind::Difference, &source, filtered.clone()).unwrap();
            let planned = vec![(super::LayerPlacement { blend: BlendMode::LinearLight, opacity: 1.0, name: "Skin Tone".into() }, layer)];
            let alpha = vec![1.0; 2];
            session.edit("Skin Tone", |doc, active| super::install(doc, active, &alpha, &planned)).unwrap();
            let doc = &session.active().unwrap().doc;
            assert_eq!(doc.layers.len(), 2, "depth {depth}");
            assert_eq!(doc.layers[1].blend, BlendMode::LinearLight);
            assert!((doc.layers[1].opacity - 1.0).abs() < 1e-6);
            let tol = match depth {
                8 => 2.0 / 255.0,
                16 => 2.0 / 65535.0,
                _ => 1e-5,
            };
            let stored = doc.layers[1].surface().unwrap().pixel(0, 0);
            assert!((stored[0] - 0.7).abs() < tol, "depth {depth}: residual {stored:?}");
            assert!((stored[1] - 0.3).abs() < tol, "depth {depth}: residual {stored:?}");
            assert!((stored[2] - 0.5).abs() < tol, "depth {depth}: unchanged channel stays mid-gray {stored:?}");
            assert!((stored[0] - 0.6).abs() > 0.05, "depth {depth}: the layer is the residual, not the model RGB");
            let flat = doc.layers[1].surface().unwrap().pixel(0, 1);
            assert!(flat[..3].iter().all(|s| (*s - 0.5).abs() < tol), "depth {depth}: unchanged pixel is mid-gray {flat:?}");
            let original = doc.layers[0].surface().unwrap().pixel(0, 0);
            assert!((original[0] - 0.2).abs() < tol, "the source layer stays put");
            let rendered = photocraft_compose::render(doc, photocraft_geom::Rect::new(0, 0, 1, 2));
            let top = rendered.get(0, 0);
            let bot = rendered.get(0, 1);
            for c in 0..3 {
                assert!((top[c] - filtered.get(0, 0)[c]).abs() < tol, "depth {depth} c={c}: composite {top:?}");
                assert!((bot[c] - source.get(0, 1)[c]).abs() < tol, "depth {depth} c={c}: unchanged composite {bot:?}");
            }
        }
    }
}
