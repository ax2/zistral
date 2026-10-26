"""Validate an explicit release entry point before any artifacts are built."""
import argparse
import os
from pathlib import Path
import re
import tomllib


def resolve_release(version: str, event: str, ref_type: str, ref_name: str,
                    requested: str = '') -> str:
    if not re.fullmatch(r'(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)', version):
        raise ValueError('Package version must be a stable major.minor.patch release')
    tag = 'v' + version
    if event == 'push':
        expected = tag if ref_type == 'tag' else 'release/' + tag
        if ref_type not in ('branch', 'tag') or ref_name != expected:
            raise ValueError('Push releases require the exact package tag or release/vVERSION branch')
    elif event == 'workflow_dispatch':
        if requested != tag:
            raise ValueError('Manual release tag must match the checked-out package version')
        if not ref_name or ref_type not in ('branch', 'tag') or (ref_type == 'tag' and ref_name != tag):
            raise ValueError('Manual release source ref does not match the package version')
    else:
        raise ValueError('Only explicit release pushes and manual dispatch may publish')
    return tag


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--event', default=os.environ.get('GITHUB_EVENT_NAME', ''))
    parser.add_argument('--ref-type', default=os.environ.get('GITHUB_REF_TYPE', ''))
    parser.add_argument('--ref-name', default=os.environ.get('GITHUB_REF_NAME', ''))
    parser.add_argument('--tag', default=os.environ.get('REQUESTED_TAG', ''))
    args = parser.parse_args()
    version = tomllib.loads((Path(__file__).resolve().parents[1] / 'Cargo.toml').read_text(encoding='utf-8'))['package']['version']
    try:
        tag = resolve_release(version, args.event, args.ref_type, args.ref_name, args.tag)
    except ValueError as error:
        parser.error(str(error))
    # Do not write workflow outputs for rejected events/refs.
    if output := os.environ.get('GITHUB_OUTPUT'):
        with open(output, 'a', encoding='utf-8', newline='\n') as stream:
            stream.write('tag=' + tag + '\n')
    print(tag)


if __name__ == '__main__':
    main()
