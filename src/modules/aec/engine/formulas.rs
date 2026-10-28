//! Persistent parametric formulas for AEC entities (Wall, Slab, Opening, Room).

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// Key-value mapping of property field names to mathematical/parametric formula strings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AecFormulas {
    #[serde(default)]
    pub fields: HashMap<String, String>,
}

impl AecFormulas {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, field: &str) -> Option<&str> {
        self.fields.get(field).map(|s| s.as_str())
    }

    pub fn set(&mut self, field: &str, formula: String) {
        let trimmed = formula.trim();
        if trimmed.is_empty() {
            self.fields.remove(field);
        } else {
            self.fields.insert(field.to_string(), trimmed.to_string());
        }
    }

    pub fn remove(&mut self, field: &str) -> Option<String> {
        self.fields.remove(field)
    }

    /// Finds the first matching formula amongst the provided keys (in priority order).
    pub fn get_any<'a, 'b>(&'a self, keys: &'b [&'b str]) -> Option<(&'b str, &'a str)> {
        for &k in keys {
            if let Some(v) = self.get(k) {
                return Some((k, v));
            }
        }
        None
    }

    /// Removes all occurrences of the specified keys.
    pub fn remove_all(&mut self, keys: &[&str]) {
        for &k in keys {
            self.remove(k);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    pub fn len(&self) -> usize {
        self.fields.len()
    }
}

/// Helper to test if a string represents a formula rather than a plain literal number.
pub fn is_formula_expression(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    // If it parses as a standard float, it's not a formula
    if t.parse::<f64>().is_ok() {
        return false;
    }
    // Check length or angle notations
    if crate::entities::common::parse_length(t).is_some()
        || crate::entities::common::parse_angle_deg(t).is_some()
    {
        return false;
    }
    true
}
