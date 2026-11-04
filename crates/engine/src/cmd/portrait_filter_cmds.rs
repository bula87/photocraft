//! Filter › Portrait retouch: external packages (`manifest.json` + `model.onnx`).
//!
//! The host lives in `photocraft-portrait-filters`. This module only turns a result into new
//! layers. Trained weights are not part of PhotoCraft; see `docs/PORTRAIT_FILTERS.md`.

use photocraft_color::ColorMode;
use photocraft_doc::{Document, Layer, LayerId};
use photocraft_portrait_filters::{
    Catalog, LayerPlacement, MenuWhich, RgbImage, STACK_ORDER, Task, disable_reason, infer, placement, resolve_directory, runtime_available, scan,
};
use photocraft_raster::Surface;
use serde_json::{Value, json};

use crate::commands::CommandSpec;
use crate::{EngineError, Result, Session};

const MENU: &[&str] = &["Filter", "Portrait retouch"];

/// Tasks the All command places, in stack order. Re-exported for tests in this module.
fn catalog_tasks(catalog: &Catalog) -> Vec<Task> {
    STACK_ORDER.into_iter().filter(|task| catalog.get(*task).is_some()).collect()
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        spec(Task::Heal),
        spec(Task::DodgeBurn),
        spec(Task::SkinTone),
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

fn spec(task: Task) -> CommandSpec {
    CommandSpec {
        id: task.command_id(),
        label: task.menu_label(),
        menu: MENU,
        shortcut: None,
        params: "{}",
        enabled: match task {
            Task::Heal => enabled_heal,
            Task::DodgeBurn => enabled_dodge,
            Task::SkinTone => enabled_skin,
        },
        run: match task {
            Task::Heal => |s, _| run_one(s, Task::Heal),
            Task::DodgeBurn => |s, _| run_one(s, Task::DodgeBurn),
            Task::SkinTone => |s, _| run_one(s, Task::SkinTone),
        },
        journal: true,
    }
}

fn enabled_heal(s: &Session) -> std::result::Result<(), String> {
    enabled_task(s, Task::Heal)
}
fn enabled_dodge(s: &Session) -> std::result::Result<(), String> {
    enabled_task(s, Task::DodgeBurn)
}
fn enabled_skin(s: &Session) -> std::result::Result<(), String> {
    enabled_task(s, Task::SkinTone)
}

fn enabled_task(s: &Session, task: Task) -> std::result::Result<(), String> {
    require_rgb(s)?;
    let catalog = catalog_of(s);
    match disable_reason(&catalog, runtime_available(), MenuWhich::Task(task)) {
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
        "packages": catalog.packages.iter().map(|package| json!({
            "task": package.task.id(),
            "name": package.solo.name,
            "version": package.version,
            "model": package.model.display().to_string(),
            "blend": package.solo.blend.label(),
            "opacity": package.solo.opacity,
            "stackedBlend": package.stacked.blend.label(),
            "stackedOpacity": package.stacked.opacity,
        })).collect::<Vec<_>>(),
        "skipped": catalog.skipped.iter().map(|skip| json!({"folder": skip.folder, "reason": skip.reason})).collect::<Vec<_>>(),
    })
}

fn run_one(s: &mut Session, task: Task) -> Result<Value> {
    run_tasks(s, &[task], false, task.menu_label())
}

fn run_all(s: &mut Session) -> Result<Value> {
    let tasks: Vec<Task> = {
        let catalog = catalog_of(s);
        if let Some(reason) = disable_reason(&catalog, runtime_available(), MenuWhich::All) {
            return Err(EngineError::Other(reason));
        }
        catalog_tasks(&catalog)
    };
    run_tasks(s, &tasks, true, "Portrait retouch")
}

fn run_tasks(s: &mut Session, tasks: &[Task], stacked: bool, label: &str) -> Result<Value> {
    require_rgb(s).map_err(|msg| EngineError::Other(msg))?;
    let catalog = catalog_of(s);
    let (image, alpha) = read_active(s)?;
    let mut planned = Vec::with_capacity(tasks.len());
    for task in tasks {
        let which = MenuWhich::Task(*task);
        if let Some(reason) = disable_reason(&catalog, runtime_available(), which) {
            return Err(EngineError::Other(reason));
        }
        let package = catalog.get(*task).ok_or_else(|| EngineError::Other(format!("portrait filter package `{}` is not installed", task.id())))?;
        let image = infer(package, &image).map_err(|e| EngineError::Other(e.to_string()))?;
        planned.push((placement(&package.solo, &package.stacked, stacked), image));
    }
    let ids = s.edit(label, |doc, active| install(doc, active, &alpha, &planned))?;
    if let Some(top) = ids.last().copied() {
        select_only(s, top);
    }
    Ok(json!({
        "layers": ids.iter().map(|id| id.0).collect::<Vec<_>>(),
        "tasks": tasks.iter().map(|task| task.id()).collect::<Vec<_>>(),
    }))
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
    use photocraft_portrait_filters::{MenuWhich, RgbImage, Task, disable_reason, runtime_available, scan};
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
        for (id, label) in [
            ("filter.portraitRetouch.heal", "Heal"),
            ("filter.portraitRetouch.dodgeBurn", "Dodge & Burn"),
            ("filter.portraitRetouch.skinTone", "Skin Tone"),
            ("filter.portraitRetouch.all", "All"),
        ] {
            let spec = crate::commands::find(id).unwrap_or_else(|| panic!("missing {id}"));
            assert_eq!(spec.menu, ["Filter", "Portrait retouch"]);
            assert_eq!(spec.label, label);
        }
        assert!(crate::commands::find("filter.portraitRetouch.packages").unwrap().menu.is_empty());
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
        assert!(catalog.get(Task::Heal).is_some());
        let err = session.execute("filter.portraitRetouch.heal", json!({})).unwrap_err().to_string();
        assert!(err.contains("ONNX Runtime"), "{err}");
        assert!(disable_reason(&scan(&root), false, MenuWhich::Task(Task::Heal)).unwrap().contains("ONNX Runtime"));
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
                (super::LayerPlacement { blend: BlendMode::Color, opacity: 1.0, name: "Skin Tone".into() }, skin),
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
            assert_eq!(doc.layers[3].blend, BlendMode::Color);
            let healed = doc.layers[1].surface().unwrap().pixel(1, 0);
            assert!((healed[0] - 0.8).abs() < 1.0 / 200.0, "depth {depth}: {healed:?}");
            let original = doc.layers[0].surface().unwrap().pixel(0, 0);
            assert!((original[0] - before[0]).abs() < 1e-5, "the source layer stays put");
            session.execute("edit.undo", json!({})).unwrap();
            assert_eq!(session.active().unwrap().doc.layers.len(), 1, "one undo drops the whole stack");
        }
    }
}
