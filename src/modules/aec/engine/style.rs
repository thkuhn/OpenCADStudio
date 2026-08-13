//! Generic style data model with single-parent inheritance.
//!
//! Provides a reusable `Style` struct and a chain resolution mechanism
//! that supports single-parent inheritance, allowing styles to override
//! or inherit properties from their ancestors.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Unique identifier for a style.
pub type StyleId = String;

/// A generic style that can inherit from a parent style.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Style {
    /// Unique identifier for the style.
    pub id: StyleId,
    /// Human-readable name of the style.
    pub name: String,
    /// The kind of object this style applies to (e.g., "Wall", "Window").
    pub object_kind: String,
    /// Optional parent style identifier for inheritance.
    pub parent_style_id: Option<StyleId>,
}

/// Errors that can occur during style inheritance resolution.
#[derive(Debug, Clone, PartialEq)]
pub enum StyleError {
    /// A cycle was detected in the inheritance chain.
    CycleDetected,
    /// A parent style identifier was not found in the style map.
    UnknownParent(StyleId),
}

/// Resolves the inheritance chain for a given style ID.
///
/// Returns a list of style IDs from the root ancestor down to the given style ID (inclusive).
/// The resulting vector is in root-to-child order: `[root, ..., id]`.
///
/// # Errors
///
/// * `StyleError::CycleDetected`: If an inheritance cycle is detected (e.g., A -> B -> A).
/// * `StyleError::UnknownParent`: If a `parent_style_id` refers to a style not present in `styles`.
pub fn resolve_chain(
    styles: &HashMap<StyleId, Style>,
    id: &StyleId,
) -> Result<Vec<StyleId>, StyleError> {
    let mut chain = Vec::new();
    let mut current_id = id;
    let mut visited = std::collections::HashSet::new();

    while !visited.contains(current_id) {
        visited.insert(current_id);

        let style = styles
            .get(current_id)
            .ok_or_else(|| StyleError::UnknownParent(current_id.clone()))?;

        chain.push(current_id.clone());

        if let Some(ref parent_id) = style.parent_style_id {
            current_id = parent_id;
        } else {
            // Reached the root
            chain.reverse();
            return Ok(chain);
        }
    }

    Err(StyleError::CycleDetected)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_style(id: &str, parent: Option<&str>) -> Style {
        Style {
            id: id.to_string(),
            name: format!("Style {}", id),
            object_kind: "Test".to_string(),
            parent_style_id: parent.map(|s| s.to_string()),
        }
    }

    #[test]
    fn resolve_single_style_no_parent() {
        let mut styles = HashMap::new();
        styles.insert("A".to_string(), create_style("A", None));

        let chain = resolve_chain(&styles, &"A".to_string()).expect("Should resolve");
        assert_eq!(chain, vec!["A".to_string()]);
    }

    #[test]
    fn resolve_simple_chain() {
        let mut styles = HashMap::new();
        styles.insert("A".to_string(), create_style("A", None));
        styles.insert("B".to_string(), create_style("B", Some("A")));
        styles.insert("C".to_string(), create_style("C", Some("B")));

        let chain = resolve_chain(&styles, &"C".to_string()).expect("Should resolve");
        assert_eq!(
            chain,
            vec!["A".to_string(), "B".to_string(), "C".to_string()]
        );
    }

    #[test]
    fn detect_cycle() {
        let mut styles = HashMap::new();
        styles.insert("A".to_string(), create_style("A", Some("B")));
        styles.insert("B".to_string(), create_style("B", Some("A")));

        let result = resolve_chain(&styles, &"A".to_string());
        assert_eq!(result, Err(StyleError::CycleDetected));
    }

    #[test]
    fn detect_unknown_parent() {
        let mut styles = HashMap::new();
        styles.insert("A".to_string(), create_style("A", Some("B")));

        let result = resolve_chain(&styles, &"A".to_string());
        assert_eq!(result, Err(StyleError::UnknownParent("B".to_string())));
    }
}
