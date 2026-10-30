//! Compare JSON integer literals exactly, borrowing digits rather than using floats or bigints.
use anyhow::{Result, bail};
use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Key<'a> {
    negative: bool,
    digits: &'a str,
}

impl<'a> Key<'a> {
    pub fn parse(raw: &'a str) -> Result<Self> {
        let (negative, digits) = raw
            .strip_prefix('-')
            .map_or((false, raw), |digits| (true, digits));
        if digits.is_empty()
            || !digits.bytes().all(|b| b.is_ascii_digit())
            || (digits.len() > 1 && digits.starts_with('0'))
        {
            bail!(
                "selected key must be a JSON integer literal; decimal/exponent syntax and type coercion are not supported"
            );
        }
        Ok(Self {
            negative: negative && digits != "0",
            digits,
        })
    }
}

impl Ord for Key<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.negative, other.negative) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (negative, _) => {
                let magnitude = self
                    .digits
                    .len()
                    .cmp(&other.digits.len())
                    .then_with(|| self.digits.cmp(other.digits));
                if negative {
                    magnitude.reverse()
                } else {
                    magnitude
                }
            }
        }
    }
}

impl PartialOrd for Key<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_order_matches_bounded_numeric_oracle_and_handles_unbounded_magnitudes() {
        let values = [
            i128::MIN,
            -9007199254740993,
            -100,
            -10,
            -2,
            -1,
            0,
            1,
            2,
            10,
            100,
            9007199254740992,
            9007199254740993,
            i128::MAX,
        ];
        for a in values {
            for b in values {
                assert_eq!(
                    Key::parse(&a.to_string())
                        .unwrap()
                        .cmp(&Key::parse(&b.to_string()).unwrap()),
                    a.cmp(&b)
                );
            }
        }
        let huge = format!("1{}", "0".repeat(4096));
        let smaller = "9".repeat(4096);
        assert!(Key::parse(&huge).unwrap() > Key::parse(&smaller).unwrap());
        assert!(
            Key::parse(&format!("-{huge}")).unwrap() < Key::parse(&format!("-{smaller}")).unwrap()
        );
        assert_eq!(Key::parse("-0").unwrap(), Key::parse("0").unwrap());
    }

    #[test]
    fn unsupported_literal_forms_fail_without_echoing_values() {
        for raw in [
            "",
            "-",
            "00",
            "-01",
            "+1",
            "1.0",
            "1e0",
            "1E+2",
            "\"2\"",
            "true",
            "null",
            "NaN",
            "１２",
            "PRIVATE_VALUE",
        ] {
            let error = Key::parse(raw).unwrap_err().to_string();
            assert!(error.contains("JSON integer literal") && !error.contains("PRIVATE"));
        }
    }
}
