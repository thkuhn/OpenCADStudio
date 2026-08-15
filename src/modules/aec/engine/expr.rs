//! Thin arithmetic-only wrapper around `evalexpr` for wall-style formulas.
//!
//! Variables are supplied by the caller (e.g. `"BB"` = wall base width).
//! Builtin and user-defined functions are disabled so formulas stay pure
//! arithmetic expressions over numeric variables.

use evalexpr::{
    eval_with_context, Context, ContextWithMutableVariables, DefaultNumericTypes, HashMapContext,
    Value,
};
use std::collections::HashMap;

/// Evaluate a formula string against a map of numeric wall variables.
///
/// Returns `Ok(f64)` on success. Failures (unknown variable, division by
/// zero, malformed syntax, non-numeric result) are returned as `Err(String)`
/// and never panic.
///
/// Only arithmetic operators and the provided variables are available —
/// function calls are rejected.
pub fn eval_formula(formula: &str, vars: &HashMap<String, f64>) -> Result<f64, String> {
    let trimmed = formula.trim();
    if trimmed.is_empty() {
        return Err("empty formula".to_string());
    }

    let mut context = HashMapContext::<DefaultNumericTypes>::new();
    // Restrict to arithmetic: no builtin functions (min/max/floor/…) and no
    // user-registered functions.
    context
        .set_builtin_functions_disabled(true)
        .map_err(|e| e.to_string())?;

    for (name, value) in vars {
        context
            .set_value(name.clone(), Value::from_float(*value))
            .map_err(|e| e.to_string())?;
    }

    let value = eval_with_context(trimmed, &context).map_err(|e| e.to_string())?;
    // `as_float` accepts both Int and Float results.
    let f: f64 = value.as_float().map_err(|e| e.to_string())?;
    if f.is_finite() {
        Ok(f)
    } else {
        Err(format!("non-finite result: {f}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bb(v: f64) -> HashMap<String, f64> {
        let mut m = HashMap::new();
        m.insert("BB".to_string(), v);
        m
    }

    #[test]
    fn eval_fixed_arithmetic() {
        let v = eval_formula("1.5 + 2.5 * 2", &HashMap::new()).unwrap();
        assert!((v - 6.5).abs() < 1e-9);
    }

    #[test]
    fn eval_bb_formula() {
        let v = eval_formula("BB * 0.5", &bb(0.4)).unwrap();
        assert!((v - 0.2).abs() < 1e-9);
        let v2 = eval_formula("BB * 0.5", &bb(0.8)).unwrap();
        assert!((v2 - 0.4).abs() < 1e-9);
    }

    #[test]
    fn unknown_variable_errors() {
        let err = eval_formula("UNKNOWN * 2", &bb(1.0)).unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn syntax_error() {
        assert!(eval_formula("BB *", &bb(1.0)).is_err());
    }

    #[test]
    fn division_by_zero_errors() {
        assert!(eval_formula("BB / 0", &bb(1.0)).is_err());
    }

    #[test]
    fn functions_are_disabled() {
        assert!(eval_formula("max(1, 2)", &HashMap::new()).is_err());
    }

    #[test]
    fn empty_formula_errors() {
        assert!(eval_formula("   ", &HashMap::new()).is_err());
    }
}
