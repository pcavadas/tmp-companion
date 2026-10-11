//! TCT v1 line format: how one trace event becomes one line of JSON, and how a trace
//! file is read back.
//!
//! A trace is JSON Lines, header first. Every line starts with the same five fields in
//! the same order (`seq`, `sid`, `t_us`, `t_nom_ms`, `ev`) and each event type then
//! writes its own fields in a fixed order, so a plain `grep` finds a field without a
//! JSON tool (jq is not a dependency of the lane). `serde_json` is not used to build the
//! object because its map ordering depends on a crate feature another dependency turns
//! on; it is only used to escape strings and to read lines back.
//!
//! A cut last line is legal: the e2e server is killed rather than stopped, so its
//! writer can die mid-line. [`read_trace`] drops that line.

use std::fmt::Write as _;

/// The format version written in every header (`tct`).
pub(crate) const TCT_VERSION: u32 = 1;

/// One trace line under construction. Fields land in the order they are added.
pub(crate) struct Line {
    buf: String,
}

impl Line {
    /// Start a line with the five fields every event carries.
    pub(crate) fn event(seq: u64, sid: Option<u64>, t_us: u64, t_nom_ms: u64, ev: &str) -> Line {
        let mut l = Line {
            buf: String::with_capacity(128),
        };
        l.buf.push('{');
        l.push_key("seq");
        let _ = write!(l.buf, "{seq}");
        l = l
            .opt_u64("sid", sid)
            .u64("t_us", t_us)
            .u64("t_nom_ms", t_nom_ms);
        l.str("ev", ev)
    }

    /// An empty object, for a nested value closed with [`Line::close`].
    pub(crate) fn object() -> Line {
        Line {
            buf: String::from("{"),
        }
    }

    fn push_key(&mut self, key: &str) {
        if self.buf.len() > 1 {
            self.buf.push(',');
        }
        self.buf.push('"');
        self.buf.push_str(key);
        self.buf.push_str("\":");
    }

    /// A JSON string, escaped.
    pub(crate) fn str(mut self, key: &str, v: &str) -> Line {
        self.push_key(key);
        self.buf
            .push_str(&serde_json::to_string(v).unwrap_or_else(|_| "null".into()));
        self
    }

    /// A JSON string, or `null`.
    pub(crate) fn opt_str(self, key: &str, v: Option<&str>) -> Line {
        match v {
            Some(v) => self.str(key, v),
            None => self.raw(key, "null"),
        }
    }

    pub(crate) fn u64(mut self, key: &str, v: u64) -> Line {
        self.push_key(key);
        let _ = write!(self.buf, "{v}");
        self
    }

    pub(crate) fn opt_u64(self, key: &str, v: Option<u64>) -> Line {
        match v {
            Some(v) => self.u64(key, v),
            None => self.raw(key, "null"),
        }
    }

    /// A finite float, or `null` (JSON has no NaN or infinity).
    pub(crate) fn f64(self, key: &str, v: f64) -> Line {
        let json = num_json(v);
        self.raw(key, &json)
    }

    pub(crate) fn bool(self, key: &str, v: bool) -> Line {
        self.raw(key, if v { "true" } else { "false" })
    }

    /// An already-encoded JSON value.
    pub(crate) fn raw(mut self, key: &str, json: &str) -> Line {
        self.push_key(key);
        self.buf.push_str(json);
        self
    }

    /// Close a nested object (no newline).
    pub(crate) fn close(mut self) -> String {
        self.buf.push('}');
        self.buf
    }

    /// Close the object and end the line.
    pub(crate) fn finish(mut self) -> String {
        self.buf.push_str("}\n");
        self.buf
    }
}

/// A float as JSON: `null` when not finite.
pub(crate) fn num_json(v: f64) -> String {
    if v.is_finite() {
        format!("{v}")
    } else {
        "null".into()
    }
}

/// A JSON array of floats.
pub(crate) fn f64_array(vals: &[f64]) -> String {
    let parts: Vec<String> = vals.iter().map(|v| num_json(*v)).collect();
    format!("[{}]", parts.join(","))
}

/// A JSON array of strings.
pub(crate) fn str_array<S: AsRef<str>>(vals: &[S]) -> String {
    let parts: Vec<String> = vals
        .iter()
        .map(|v| serde_json::to_string(v.as_ref()).unwrap_or_else(|_| "null".into()))
        .collect();
    format!("[{}]", parts.join(","))
}

/// Lowercase hex, no separators.
pub(crate) fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn read_varint(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    let mut shift = 0;
    while *pos < buf.len() {
        let b = buf[*pos];
        *pos += 1;
        value |= u64::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
        if shift >= 64 {
            return None;
        }
    }
    None
}

/// The first tag of a protobuf buffer: `(field number, nested bytes when it is
/// length-delimited)`. The nested slice is cut at the buffer's end, so a stream-start
/// report's partial body still yields its kind.
fn first_tag(buf: &[u8]) -> Option<(u64, Option<&[u8]>)> {
    let mut pos = 0;
    let tag = read_varint(buf, &mut pos)?;
    let field = tag >> 3;
    if field == 0 {
        return None;
    }
    if tag & 7 != 2 {
        return Some((field, None));
    }
    let len = read_varint(buf, &mut pos)? as usize;
    let end = pos.saturating_add(len).min(buf.len());
    Some((field, Some(&buf[pos..end])))
}

/// A message body's kind, `<tms>.<inner>`: the top-level `FenderMessageTMS` field
/// number, then the first field number inside it (`-` when the top-level field is not a
/// message). `?` when the body does not start with a tag.
pub(crate) fn body_kind(body: &[u8]) -> String {
    match first_tag(body) {
        None => "?".into(),
        Some((tms, None)) => format!("{tms}.-"),
        Some((tms, Some(inner))) => match first_tag(inner) {
            Some((f, _)) => format!("{tms}.{f}"),
            None => format!("{tms}.-"),
        },
    }
}

/// An input report's kind: the body kind on a single frame (`0x35`) or a stream start
/// (`0x33`), `cont` on a continuation (`0x34`), `?` otherwise. Reports carry the
/// report-id byte first (`0x00`), then the magic, then `0x00`, then the body length.
pub(crate) fn report_kind(report: &[u8]) -> String {
    if report.len() < 4 || report[0] != 0x00 {
        return "?".into();
    }
    match report[1] {
        crate::proto::MAGIC_IN_FRAME => "cont".into(),
        crate::proto::MAGIC_IN_START | 0x35 => body_kind(report_body(report)),
        _ => "?".into(),
    }
}

/// The body bytes a report carries, cut to its declared length.
pub(crate) fn report_body(report: &[u8]) -> &[u8] {
    if report.len() < 4 {
        return &[];
    }
    let end = (4 + report[3] as usize).min(report.len());
    &report[4..end]
}

/// `yyyy-mm-ddThh:mm:ss.mmmZ` for a time since the UNIX epoch.
pub(crate) fn utc_millis(since_epoch: std::time::Duration) -> String {
    let secs = since_epoch.as_secs();
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = crate::commands::support::civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60,
        since_epoch.subsec_millis()
    )
}

/// Read a trace back: one JSON value per line, in order. A last line that is cut (no
/// trailing newline, or not valid JSON) is dropped; a bad line anywhere else is an
/// error, because only the writer's last line can be cut.
pub(crate) fn read_trace(text: &str) -> Result<Vec<serde_json::Value>, String> {
    let mut lines: Vec<&str> = text.split('\n').collect();
    // `split` leaves a final "" after a trailing newline; anything else there is a line
    // the writer never finished.
    let last = lines.pop().unwrap_or("");
    let mut out = Vec::with_capacity(lines.len());
    let n = lines.len();
    for (i, line) in lines.into_iter().enumerate() {
        match serde_json::from_str(line) {
            Ok(v) => out.push(v),
            Err(_) if i + 1 == n && last.is_empty() => {}
            Err(e) => return Err(format!("trace line {}: {e}", i + 1)),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_keeps_its_field_order() {
        let l = Line::event(3, Some(1), 10, 20, "out")
            .str("call", "transact")
            .u64("pump_ms", 50)
            .finish();
        assert_eq!(
            l,
            "{\"seq\":3,\"sid\":1,\"t_us\":10,\"t_nom_ms\":20,\"ev\":\"out\",\"call\":\"transact\",\"pump_ms\":50}\n"
        );
        let v: serde_json::Value = serde_json::from_str(l.trim_end()).unwrap();
        assert_eq!(v["call"], "transact");
    }

    #[test]
    fn non_finite_floats_are_null() {
        let l = Line::event(0, None, 0, 0, "x")
            .f64("a", f64::NEG_INFINITY)
            .finish();
        assert!(l.contains("\"sid\":null"));
        assert!(l.contains("\"a\":null"));
    }

    #[test]
    fn a_cut_last_line_is_dropped() {
        let text = "{\"seq\":0}\n{\"seq\":1}\n{\"seq\":2,\"ev\":\"o";
        let lines = read_trace(text).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1]["seq"], 1);
        // A complete file reads every line.
        assert_eq!(read_trace("{\"seq\":0}\n").unwrap().len(), 1);
        // A bad line in the middle is not a cut line.
        assert!(read_trace("{\"seq\":0}\nnope\n{\"seq\":2}\n").is_err());
    }

    #[test]
    fn kinds_decode_single_frames_streams_and_continuations() {
        // An OUT body: re-amp ON is settingsMessage(3) → reampModeActive(30).
        assert_eq!(body_kind(&crate::proto::set_reamp_mode(true)), "3.30");
        assert_eq!(
            body_kind(&crate::proto::heartbeat()),
            body_kind(&crate::proto::heartbeat())
        );
        assert_eq!(body_kind(&[]), "?");

        // A single-frame IN report carrying currentFwResponse (settings 3, field 2).
        let body = [
            0x1a, 0x0a, 0x12, 0x08, 0x0a, 0x06, b'1', b'.', b'8', b'.', b'5', b'8',
        ];
        let mut single = vec![0x00, 0x35, 0x00, body.len() as u8];
        single.extend_from_slice(&body);
        assert_eq!(report_kind(&single), "3.2");

        // A stream start whose nested length runs past the report still decodes.
        let mut start = vec![0x00, 0x33, 0x00, 6];
        start.extend_from_slice(&[0x12, 0xe8, 0x07, 0x2a, 0x10, 0x7b]);
        assert_eq!(report_kind(&start), "2.5");

        let cont = vec![0x00, 0x34, 0x00, 3, b'a', b'b', b'c'];
        assert_eq!(report_kind(&cont), "cont");
        assert_eq!(report_kind(&[0x01, 0x35, 0, 0]), "?");
    }

    #[test]
    fn utc_formats_milliseconds() {
        let d = std::time::Duration::from_millis(1_791_744_515_984);
        assert_eq!(utc_millis(d), "2026-10-11T18:48:35.984Z");
    }

    #[test]
    fn hex_is_lowercase() {
        assert_eq!(hex(&[0x1a, 0x05, 0xf2]), "1a05f2");
    }
}
