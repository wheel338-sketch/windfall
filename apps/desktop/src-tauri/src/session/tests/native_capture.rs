//! Comparing a fresh native capture with the checked one the UI's own tests
//! read. The checked captures were recorded on Windows. On Windows they must
//! match exactly. Elsewhere the platform's math library rounds `sin`, `exp`,
//! `log10` and friends differently in the last bits, so every number may
//! differ by float rounding and nothing else: the structure, every string
//! and every flag must still be identical.

use serde_json::Value;

/// Rounding the platform's math library may introduce: sixteen f32 epsilons
/// relative, plus 1e-8 absolute for values cancelled down near zero. Linux
/// (glibc) measured at most about 2 epsilons on ordinary values and 4e-9
/// absolute near zero.
fn rounding_only(expected: f64, actual: f64) -> bool {
    let bound = 16.0 * f64::from(f32::EPSILON) * expected.abs().max(actual.abs()) + 1e-8;
    (expected - actual).abs() <= bound
}

fn differences(expected: &Value, actual: &Value, path: &str, out: &mut Vec<String>) {
    if out.len() >= 20 {
        return;
    }
    match (expected, actual) {
        (Value::Number(a), Value::Number(b)) => {
            let (Some(x), Some(y)) = (a.as_f64(), b.as_f64()) else {
                if a != b {
                    out.push(format!("{path}: {a} != {b}"));
                }
                return;
            };
            if !rounding_only(x, y) {
                out.push(format!("{path}: {x} != {y}"));
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (index, (x, y)) in a.iter().zip(b).enumerate() {
                differences(x, y, &format!("{path}[{index}]"), out);
            }
        }
        (Value::Object(a), Value::Object(b))
            if a.len() == b.len() && a.keys().all(|key| b.contains_key(key)) =>
        {
            for (key, x) in a {
                differences(x, &b[key], &format!("{path}.{key}"), out);
            }
        }
        _ if expected == actual => {}
        _ => out.push(format!(
            "{path}: {} != {}",
            truncated(expected),
            truncated(actual)
        )),
    }
}

fn truncated(value: &Value) -> String {
    let text = value.to_string();
    text.chars().take(200).collect()
}

/// Panics with `message` and the first differing paths unless `actual`
/// matches `expected` as described in the module documentation.
pub(super) fn assert_matches(expected: Value, actual: Value, message: &str) {
    if cfg!(windows) {
        assert_eq!(expected, actual, "{message}");
        return;
    }
    let mut found = Vec::new();
    differences(&expected, &actual, "", &mut found);
    assert!(found.is_empty(), "{message}: {found:#?}");
}

#[test]
fn only_float_rounding_is_allowed_off_windows() {
    let expected =
        serde_json::json!({"level": 10.061794, "tiny": -3.258884e-5, "name": "a", "on": true});
    let rounded =
        serde_json::json!({"level": 10.061796, "tiny": -3.2592565e-5, "name": "a", "on": true});
    let mut found = Vec::new();
    differences(&expected, &rounded, "", &mut found);
    assert!(found.is_empty(), "{found:?}");
    for changed in [
        serde_json::json!({"level": 10.07, "tiny": -3.258884e-5, "name": "a", "on": true}),
        serde_json::json!({"level": 10.061794, "tiny": -3.3e-5, "name": "a", "on": true}),
        serde_json::json!({"level": 10.061794, "tiny": -3.258884e-5, "name": "b", "on": true}),
        serde_json::json!({"level": 10.061794, "tiny": -3.258884e-5, "name": "a", "on": false}),
        serde_json::json!({"level": 10.061794, "tiny": -3.258884e-5, "name": "a"}),
        serde_json::json!([10.061794]),
    ] {
        let mut found = Vec::new();
        differences(&expected, &changed, "", &mut found);
        assert!(!found.is_empty(), "{changed} passed as rounding");
    }
}
