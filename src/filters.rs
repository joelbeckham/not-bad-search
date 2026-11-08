use crate::sources::Source;
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq)]
pub struct SearchFilters {
    pub sort: crate::sorting::SortMode,
    pub text: bool,
    pub images: bool,
    pub semantic: bool,
    pub text_matches: bool,
    /// None means all roots, including newly configured sources. Some([]) means none.
    pub roots: Option<Vec<String>>,
}
impl Default for SearchFilters {
    fn default() -> Self {
        Self {
            sort: Default::default(),
            text: true,
            images: true,
            semantic: true,
            text_matches: true,
            roots: None,
        }
    }
}

/// Source IDs survive renames and path edits. All is deliberately not a snapshot.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct SourceSelection(pub Option<BTreeSet<String>>);
impl SourceSelection {
    pub fn contains(&self, id: &str) -> bool {
        self.0.as_ref().is_none_or(|ids| ids.contains(id))
    }
    pub fn set(&mut self, ids: &[String], enabled: bool, sources: &[Source]) {
        let selected = self
            .0
            .get_or_insert_with(|| sources.iter().map(|s| s.id.clone()).collect());
        for id in ids {
            if enabled {
                selected.insert(id.clone());
            } else {
                selected.remove(id);
            }
        }
    }
    pub fn roots(&self, sources: &[Source]) -> Option<Vec<String>> {
        self.0.as_ref().map(|_| {
            sources
                .iter()
                .filter(|s| self.contains(&s.id))
                .map(|s| {
                    if s.is_folder() {
                        s.path.to_string_lossy().into_owned()
                    } else {
                        s.root()
                    }
                })
                .collect()
        })
    }
}
