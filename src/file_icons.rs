//! Bundled Catppuccin Mocha icons. Lookup never accesses the filesystem.
use serde::Deserialize;
use std::{collections::HashMap, sync::OnceLock};

#[path = "file_icon_assets.rs"]
mod assets;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Mapping {
    file: String,
    file_names: HashMap<String, String>,
    file_extensions: HashMap<String, String>,
}
fn mapping() -> &'static Mapping {
    static MAPPING: OnceLock<Mapping> = OnceLock::new();
    MAPPING.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/catppuccin/mapping.json"))
            .expect("validated bundled icon mapping")
    })
}

pub fn icon_id(name: &str) -> &'static str {
    let map = mapping();
    let name = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .to_lowercase();
    if let Some(id) = map.file_names.get(&name) {
        return id;
    }
    // Longest compound extension first (e.g. css.map before map).
    for (offset, _) in name.match_indices('.') {
        if let Some(id) = map.file_extensions.get(&name[offset + 1..]) {
            return id;
        }
    }
    &map.file
}

pub fn icon(name: &str) -> (&'static str, &'static [u8]) {
    assets::asset(icon_id(name)).unwrap_or_else(|| assets::asset("_file").unwrap())
}

pub fn jira_icon() -> (&'static str, &'static [u8]) {
    ("bytes://sources/jira.svg", include_bytes!("../assets/sources/jira.svg"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filename_precedence_compound_extensions_and_fallback() {
        assert_eq!(icon_id("C:\\notes\\CARGO.TOML"), "cargo");
        assert_eq!(icon_id("package.json"), "package-json");
        assert_eq!(icon_id(".gitignore"), "git");
        assert_eq!(icon_id("photo.PNG"), "image");
        assert_eq!(icon_id("note.md"), "markdown");
        assert_eq!(icon_id("styles.css.map"), "css-map");
        assert_eq!(icon_id("unknown.mystery"), "_file");
        assert_eq!(icon_id("no-extension"), "_file");
    }
    #[test]
    fn all_mapped_icons_render() {
        egui_extras::image::load_svg_bytes(jira_icon().1, &Default::default())
            .expect("Invalid bundled Jira icon");
        let map = mapping();
        let ids: std::collections::HashSet<_> = map
            .file_names
            .values()
            .chain(map.file_extensions.values())
            .chain(std::iter::once(&map.file))
            .collect();
        for id in ids {
            let (_, svg) = assets::asset(id).unwrap_or_else(|| panic!("Missing icon: {id}"));
            egui_extras::image::load_svg_bytes(svg, &Default::default())
                .unwrap_or_else(|e| panic!("Invalid icon {id}: {e}"));
        }
    }
}
pub fn slack_icon() -> (&'static str, &'static [u8]) {
    ("bytes://slack.svg", br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M8 3v14" stroke="#36c5f0" stroke-width="4" stroke-linecap="round"/><path d="M7 8h14" stroke="#2eb67d" stroke-width="4" stroke-linecap="round"/><path d="M16 7v14" stroke="#ecb22e" stroke-width="4" stroke-linecap="round"/><path d="M3 16h14" stroke="#e01e5a" stroke-width="4" stroke-linecap="round"/></svg>"##)
}

pub fn gmail_icon() -> (&'static str, &'static [u8]) {
    ("bytes://gmail.svg", br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M3 19V6l9 7 9-7v13" fill="none" stroke="#ea4335" stroke-width="3" stroke-linejoin="round"/><path d="M3 19V7" stroke="#4285f4" stroke-width="3"/><path d="M21 19V7" stroke="#34a853" stroke-width="3"/></svg>"##)
}
