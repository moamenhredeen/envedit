//! Line-based record format used for snapshots and helper files.
//!
//! ```text
//! envedit-records 1
//! <kind>\t<field>\t<field>...
//! ```
//!
//! Fields escape `\`, tab, newline and carriage return. A bad header or a
//! malformed line makes the whole file invalid.

pub const HEADER: &str = "envedit-records 1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub kind: String,
    pub fields: Vec<String>,
}

impl Record {
    pub fn new<S: Into<String>>(kind: &str, fields: impl IntoIterator<Item = S>) -> Record {
        Record {
            kind: kind.to_string(),
            fields: fields.into_iter().map(Into::into).collect(),
        }
    }
}

pub fn encode(records: &[Record]) -> String {
    let mut out = String::from(HEADER);
    out.push('\n');
    for record in records {
        out.push_str(&escape(&record.kind));
        for field in &record.fields {
            out.push('\t');
            out.push_str(&escape(field));
        }
        out.push('\n');
    }
    out
}

pub fn decode(text: &str) -> Result<Vec<Record>, String> {
    let mut lines = text.split('\n');
    match lines.next() {
        Some(HEADER) => {}
        _ => return Err("missing or unknown header".to_string()),
    }
    let mut records = Vec::new();
    for (index, line) in lines.enumerate() {
        if line.is_empty() {
            continue;
        }
        let line_no = index + 2;
        let mut parts = line
            .split('\t')
            .map(unescape)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("line {line_no}: {e}"))?;
        let kind = parts.remove(0);
        if kind.is_empty() {
            return Err(format!("line {line_no}: empty record kind"));
        }
        records.push(Record { kind, fields: parts });
    }
    Ok(records)
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out
}

fn unescape(s: &str) -> Result<String, String> {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('\\') => out.push('\\'),
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some(other) => return Err(format!("unknown escape '\\{other}'")),
                None => return Err("dangling escape at end of field".to_string()),
            },
            '\r' => return Err("unescaped carriage return".to_string()),
            c => out.push(c),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(records: &[Record]) -> Vec<Record> {
        decode(&encode(records)).expect("decode")
    }

    #[test]
    fn round_trips_special_characters() {
        let records = vec![
            Record::new("var", ["user", "Path", "REG_SZ", "a\tb\nc\rd\\e"]),
            Record::new("var", ["machine", "Ünïcødé", "REG_EXPAND_SZ", "日本語;%X%"]),
            Record::new("ok", ["machine", "Path"]),
            Record::new("empty", [""]),
        ];
        assert_eq!(round_trip(&records), records);
    }

    #[test]
    fn round_trips_max_length_value() {
        let value = "x".repeat(32_767);
        let records = vec![Record::new("var", ["user", "BIG", "REG_SZ", value.as_str()])];
        assert_eq!(round_trip(&records), records);
    }

    #[test]
    fn rejects_bad_header() {
        assert!(decode("something else\nvar\tuser\n").is_err());
        assert!(decode("").is_err());
    }

    #[test]
    fn rejects_malformed_lines() {
        assert!(decode(&format!("{HEADER}\nvar\tbad\\q\n")).is_err());
        assert!(decode(&format!("{HEADER}\nvar\tbad\\\n")).is_err());
        assert!(decode(&format!("{HEADER}\n\tno-kind\n")).is_err());
        assert!(decode(&format!("{HEADER}\nvar\tcr\rinside\n")).is_err());
    }

    #[test]
    fn accepts_empty_file_body() {
        assert_eq!(decode(&format!("{HEADER}\n")).unwrap(), vec![]);
    }
}
