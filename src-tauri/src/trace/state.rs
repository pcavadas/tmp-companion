//! Field-8 state records: the canonical form of a stored preset body.
//!
//! The device does not store the text it was sent. It drops parameters a model lacks,
//! adds its own defaults and re-rounds floats (fw 1.8.58: an imported `0.49` reads back
//! as `0.489999920129776`). Two readers that compare bodies, this recorder and the
//! replay normalizer, must therefore agree on one canonical form, and both call
//! [`canonical_json`] for it:
//!
//! - object keys sorted, at every depth;
//! - arrays kept in order;
//! - every non-integer number taken through `f32` (the device's storage width) and then
//!   to six significant digits, which absorbs the few `f32` steps the device's
//!   re-rounding moves a value by; integers stay exact;
//! - strings, node ids included, verbatim.
//!
//! A body that does not parse as JSON (field 8 is a partial read that can stop short) is
//! never canonicalized: the caller records `complete: false` and its length instead.

use serde_json::Value;

/// The canonical JSON text of a field-8 body, or `None` when it does not parse.
/// Trailing NUL padding and whitespace are ignored.
pub(crate) fn canonical_json(body: &[u8]) -> Option<String> {
    let end = body
        .iter()
        .rposition(|b| *b != 0 && !b.is_ascii_whitespace())
        .map_or(0, |i| i + 1);
    let doc: Value = serde_json::from_slice(&body[..end]).ok()?;
    let mut out = String::with_capacity(end);
    write_canonical(&doc, &mut out);
    Some(out)
}

/// A float as the canonical form writes it: through `f32`, then six significant
/// digits, then the shortest text that reads back as that value.
pub(crate) fn canonical_float(v: f64) -> f64 {
    let narrowed = f64::from(v as f32);
    format!("{narrowed:.5e}").parse().unwrap_or(narrowed)
}

fn write_canonical(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                out.push_str(&n.to_string());
            } else {
                let f = canonical_float(n.as_f64().unwrap_or(0.0));
                out.push_str(&super::format::num_json(f));
            }
        }
        Value::String(s) => {
            out.push_str(&serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into()));
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(k).unwrap_or_else(|_| "\"\"".into()));
                out.push(':');
                write_canonical(&map[k], out);
            }
            out.push('}');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_device_rerounding_canonicalizes_to_the_imported_value() {
        let sent = br#"{"nodeId":"ACD_UniVibe","dspUnitParameters":{"volume":0.49}}"#;
        let stored =
            br#"{"dspUnitParameters":{"volume":0.489999920129776},"nodeId":"ACD_UniVibe"}"#;
        assert_eq!(canonical_json(sent), canonical_json(stored));
        assert_eq!(
            canonical_json(sent).unwrap(),
            r#"{"dspUnitParameters":{"volume":0.49},"nodeId":"ACD_UniVibe"}"#
        );
    }

    #[test]
    fn a_real_edit_still_differs() {
        let a = canonical_json(br#"{"v":0.49}"#);
        let b = canonical_json(br#"{"v":0.4901}"#);
        assert_ne!(a, b);
    }

    #[test]
    fn node_ids_and_integers_stay_verbatim() {
        let c = canonical_json(br#"{"b":{"nodeId":"cab1_1"},"a":[3,1.0,"x"],"n":12345678901}"#)
            .unwrap();
        assert_eq!(
            c,
            r#"{"a":[3,1,"x"],"b":{"nodeId":"cab1_1"},"n":12345678901}"#
        );
    }

    #[test]
    fn a_cut_body_does_not_canonicalize() {
        assert_eq!(canonical_json(br#"{"audioGraph":{"presetLevel":0.5"#), None);
        assert_eq!(canonical_json(b""), None);
        // NUL padding after a complete body is not a cut.
        assert!(canonical_json(b"{\"a\":1}\0\0").is_some());
    }
}
