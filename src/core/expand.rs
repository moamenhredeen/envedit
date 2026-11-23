//! `%VAR%` expansion against the loaded and staged state.

/// Result of expanding a text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expanded {
    pub text: String,
    /// Referenced names that are not defined, in order of appearance.
    pub unresolved: Vec<String>,
}

/// Nesting depth for references whose values contain references.
const MAX_DEPTH: usize = 8;

/// Expands `%NAME%` references like `ExpandEnvironmentStrings` does:
/// undefined references stay as written, a lone `%` is kept.
pub fn expand(text: &str, lookup: &dyn Fn(&str) -> Option<String>) -> Expanded {
    let mut unresolved = Vec::new();
    let text = expand_depth(text, lookup, &mut unresolved, 0);
    Expanded { text, unresolved }
}

fn expand_depth(text: &str, lookup: &dyn Fn(&str) -> Option<String>, unresolved: &mut Vec<String>, depth: usize) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('%') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let name = &after[..end];
        match (!name.is_empty()).then(|| lookup(name)).flatten() {
            Some(value) => {
                let value = if depth < MAX_DEPTH && value.contains('%') {
                    expand_depth(&value, lookup, unresolved, depth + 1)
                } else {
                    value
                };
                out.push_str(&value);
                rest = &after[end + 1..];
            }
            None => {
                if !name.is_empty() && !unresolved.iter().any(|u| u.eq_ignore_ascii_case(name)) {
                    unresolved.push(name.to_string());
                }
                // Keep the opening `%` and the name; the closing `%` may open the next reference.
                out.push('%');
                out.push_str(name);
                rest = &after[end..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn lookup_from(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_uppercase(), v.to_string())).collect();
        move |name| map.get(&name.to_uppercase()).cloned()
    }

    #[test]
    fn expands_defined_reference() {
        let lookup = lookup_from(&[("USERPROFILE", r"C:\Users\me")]);
        let e = expand(r"%USERPROFILE%\bin", &lookup);
        assert_eq!(e.text, r"C:\Users\me\bin");
        assert!(e.unresolved.is_empty());
    }

    #[test]
    fn keeps_undefined_reference() {
        let lookup = lookup_from(&[]);
        let e = expand(r"%NOPE%\bin", &lookup);
        assert_eq!(e.text, r"%NOPE%\bin");
        assert_eq!(e.unresolved, vec!["NOPE"]);
    }

    #[test]
    fn closing_percent_of_undefined_can_open_next() {
        let lookup = lookup_from(&[("B", "x")]);
        assert_eq!(expand("%A%B%", &lookup).text, "%Ax");
    }

    #[test]
    fn lone_percent_kept() {
        let lookup = lookup_from(&[]);
        assert_eq!(expand("100%", &lookup).text, "100%");
        assert_eq!(expand("%%", &lookup).text, "%%");
    }

    #[test]
    fn nested_reference_to_staged_variable() {
        let lookup = lookup_from(&[("TOOLS", r"%USERPROFILE%\tools"), ("USERPROFILE", r"C:\Users\me")]);
        assert_eq!(expand(r"%TOOLS%\bin", &lookup).text, r"C:\Users\me\tools\bin");
    }

    #[test]
    fn self_reference_terminates() {
        let lookup = lookup_from(&[("LOOP", "%LOOP%")]);
        assert_eq!(expand("%LOOP%", &lookup).text, "%LOOP%");
    }
}
