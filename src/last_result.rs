//! One completed answer for explicit interactive export. Never model context or history.
use anyhow::{Result, bail};
use std::path::Path;

const MAX_RETAINED: usize = 16 * 1024 * 1024;

#[derive(Default)]
pub struct LastResult(Option<String>);

impl LastResult {
    pub fn clear(&mut self) {
        self.0 = None;
    }

    pub fn retain(&mut self, text: String) -> Result<()> {
        self.clear();
        if text.len() > MAX_RETAINED {
            bail!("completed result exceeds 16 MiB; not retained for /save");
        }
        self.0 = Some(text);
        Ok(())
    }

    pub fn save(&self, path: &Path, force: bool) -> Result<usize> {
        let text = self
            .0
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no completed result to save; run a task first"))?;
        if path.as_os_str() == "-" {
            bail!("/save requires a file path, not stdout '-'");
        }
        crate::output::Prepared::new(path, force)?.save(text, !text.ends_with('\n'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_result_is_bounded_in_utf8_bytes_and_oversize_clears_previous() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("result.txt");
        let mut result = LastResult::default();
        let boundary = "界".repeat(MAX_RETAINED / 3) + "x";
        assert_eq!(boundary.len(), MAX_RETAINED);
        result.retain(boundary).unwrap();
        assert!(result.retain("x".repeat(MAX_RETAINED + 1)).is_err());
        assert!(result.save(&path, false).is_err());
        assert!(!path.exists());
    }
}
