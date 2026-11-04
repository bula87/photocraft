//! Filter › Portrait retouch: one menu row per package found on disk.
//!
//! Rows are not engine command specs. A new package id shows up here with no PhotoCraft change.
//! Choosing a row runs `filter.portraitRetouch.apply` with that id.

use serde_json::{Value, json};

use crate::PhotocraftApp;
use crate::menus::MenuItem;

/// Menu ids of installed packages: `filter.portraitRetouch.pkg.<package id>`.
const PREFIX: &str = "filter.portraitRetouch.pkg.";

/// Inserts discovered packages into Filter › Portrait retouch, above All.
pub fn insert_menu_items(app: &PhotocraftApp, items: &mut Vec<MenuItem>) {
    let Some(at) = items.iter().position(|i| i.id == "filter.portraitRetouch.all") else { return };
    let Some(path) = items.get(at).map(|i| i.path.clone()) else { return };
    let catalog = catalog(app);
    let enabled = app.session.is_enabled("filter.portraitRetouch.apply");
    let mut extra: Vec<MenuItem> = photocraft_portrait_filters::menu_entries(&catalog)
        .into_iter()
        .map(|entry| MenuItem { id: entry.menu_id, label: entry.label, path: path.clone(), shortcut: None, enabled, checked: None, color: None })
        .collect();
    if !extra.is_empty() {
        extra.push(MenuItem { id: "---".into(), label: "---".into(), path: path.clone(), shortcut: None, enabled: false, checked: None, color: None });
    }
    for (k, it) in extra.into_iter().enumerate() {
        items.insert(at + k, it);
    }
}

/// Enabled state of a discovered package row (`None` for other ids).
pub fn is_enabled(app: &PhotocraftApp, id: &str) -> Option<bool> {
    let pid = id.strip_prefix(PREFIX)?;
    Some(catalog(app).get(pid).is_some() && app.session.is_enabled("filter.portraitRetouch.apply"))
}

/// Runs a discovered package by forwarding its id to `filter.portraitRetouch.apply`.
pub fn menu(app: &mut PhotocraftApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    let pid = id.strip_prefix(PREFIX)?;
    let mut body = params.as_object().cloned().unwrap_or_default();
    body.insert("id".into(), json!(pid));
    Some(app.run("filter.portraitRetouch.apply", Value::Object(body)))
}

fn catalog(app: &PhotocraftApp) -> photocraft_portrait_filters::Catalog {
    match photocraft_portrait_filters::resolve_directory(&app.session.prefs().plug_ins.portrait_filters_folder) {
        Some(dir) => photocraft_portrait_filters::scan(&dir),
        None => photocraft_portrait_filters::Catalog::unavailable(std::path::PathBuf::new(), "no portrait filters directory"),
    }
}

#[cfg(test)]
mod tests {
    use super::PREFIX;

    #[test]
    fn package_menu_ids_use_the_discoverable_prefix() {
        assert!(photocraft_portrait_filters::menu_command_id("glow").starts_with(PREFIX));
        assert_eq!(photocraft_portrait_filters::menu_command_id("glow"), "filter.portraitRetouch.pkg.glow");
    }
}
