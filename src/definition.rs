//! Definition diagnostics deliberately omit parser messages, source excerpts and values.
use anyhow::anyhow;

pub fn toml_error(error: &toml::de::Error, text: &str, kind: &str) -> anyhow::Error {
    let location = error
        .span()
        .and_then(|span| text.get(..span.start))
        .map(|prefix| {
            let prefix = prefix.strip_prefix('\u{feff}').unwrap_or(prefix);
            let line = prefix.bytes().filter(|b| *b == b'\n').count() + 1;
            let column = prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1;
            format!(" at line {line}, column {column}")
        })
        .unwrap_or_default();
    anyhow!("invalid {kind} TOML{location}; check version, syntax and supported fields")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations_count_unicode_characters_crlf_and_ignore_initial_bom() {
        for text in ["\u{feff}PRIVATE = \"中\" @", "# 中\r\nPRIVATE = \"中\" @"] {
            let error = toml::from_str::<toml::Value>(text).unwrap_err();
            let offset = text.find('@').unwrap();
            let prefix = text[..offset].trim_start_matches('\u{feff}');
            let expected = format!(
                "line {}, column {}",
                prefix.matches('\n').count() + 1,
                prefix.rsplit('\n').next().unwrap().chars().count() + 1
            );
            let rendered = toml_error(&error, text, "recipe").to_string();
            assert!(rendered.contains(&expected), "{rendered}");
            assert!(
                !rendered.contains("PRIVATE")
                    && !rendered.contains('中')
                    && !rendered.contains('@')
            );
        }
    }

    #[test]
    fn missing_spans_keep_a_safe_diagnostic_without_inventing_a_location() {
        let error = <toml::de::Error as serde::de::Error>::custom("PRIVATE_PARSER_VALUE");
        let rendered = toml_error(&error, "PRIVATE_SOURCE", "presets").to_string();
        assert_eq!(
            rendered,
            "invalid presets TOML; check version, syntax and supported fields"
        );
    }
}
