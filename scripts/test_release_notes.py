import unittest

from release_notes import extract_notes


class ReleaseNotesTests(unittest.TestCase):
    def test_only_selected_version_and_nested_headings(self):
        changelog = '# Changelog\n\n## 0.5.0 — today\n\n- New feature\n\n### Fixes\n- Fix\n\n## 0.4.0 — yesterday\n\n- Historical feature\n'
        self.assertEqual(extract_notes(changelog, 'v0.5.0'), '- New feature\n\n### Fixes\n- Fix\n')
        self.assertEqual(extract_notes(changelog, '0.4.0'), '- Historical feature\n')

    def test_exact_version_match(self):
        changelog = '## 0.4.01\n- Other\n## 0.4.0\n- Selected\n'
        self.assertEqual(extract_notes(changelog, 'v0.4.0'), '- Selected\n')

    def test_missing_duplicate_or_empty_version_fails(self):
        for changelog in ['', '## 0.4.0\n', '## 0.4.0\n- A\n## 0.4.0\n- B\n']:
            with self.subTest(changelog=changelog), self.assertRaises(ValueError):
                extract_notes(changelog, 'v0.4.0')

    def test_invalid_version_fails(self):
        for tag in ['latest', 'v0.4', 'v0.4.0\n', '../0.4.0']:
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                extract_notes('## 0.4.0\n- A\n', tag)


if __name__ == '__main__':
    unittest.main()
