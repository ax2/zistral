"""Extract one version from CHANGELOG.md for a GitHub Release."""
import argparse
from pathlib import Path
import re
import sys


def extract_notes(changelog: str, tag: str) -> str:
    version = tag.removeprefix('v')
    if not re.fullmatch(r'\d+\.\d+\.\d+', version):
        raise ValueError('Expected a version such as v0.4.0')
    sections = re.split(r'(?m)^##[ \t]+', changelog)
    matches = []
    for section in sections[1:]:
        heading, _, body = section.partition('\n')
        if re.match(re.escape(version) + r'(?:\s|$)', heading):
            matches.append(body.strip())
    if len(matches) != 1:
        raise ValueError(f'Expected exactly one changelog section for {version}')
    if not matches[0]:
        raise ValueError(f'Changelog section for {version} is empty')
    return matches[0] + '\n'


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('tag')
    args = parser.parse_args()
    changelog = Path(__file__).resolve().parents[1] / 'CHANGELOG.md'
    try:
        notes = extract_notes(changelog.read_text(encoding='utf-8'), args.tag)
    except ValueError as error:
        parser.error(str(error))
    sys.stdout.buffer.write(notes.encode('utf-8'))


if __name__ == '__main__':
    main()
