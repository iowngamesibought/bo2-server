"""Generate private deployment inputs from the user's supported decrypted executable.

Python 3.11+. No downloads, decryption, target execution, sockets or database access.
"""
import argparse
import hashlib
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LAYOUT = ROOT / 'config' / 'client-layout.json'
MAX_INPUT = 32 * 1024 * 1024


def digest(data):
    return hashlib.sha256(data).hexdigest()


def json_bytes(value):
    return (json.dumps(value, indent=2, ensure_ascii=False) + '\n').encode('utf-8')


def materialize(value, data):
    if isinstance(value, dict) and 'slices' in value:
        if set(value) != {'slices'} or not isinstance(value['slices'], list) or not value['slices']:
            raise ValueError('invalid layout slice reference')
        result = b''
        for span in value['slices']:
            if (not isinstance(span, list) or len(span) != 2
                    or any(type(n) is not int for n in span)):
                raise ValueError('invalid layout slice bounds')
            offset, length = span
            if offset < 0 or length <= 0 or length > 127 or offset + length > len(data):
                raise ValueError('layout slice exceeds input bounds')
            result += data[offset:offset + length]
        if not re.fullmatch(rb'[A-Za-z0-9_.-]{1,127}', result):
            raise ValueError('invalid extracted identifier')
        return result.decode('ascii')
    if isinstance(value, dict):
        return {key: materialize(item, data) for key, item in value.items()}
    if isinstance(value, list):
        return [materialize(item, data) for item in value]
    return value


def prepare(source, output, layout):
    source = Path(source).resolve(strict=True)
    if not source.is_file() or source.stat().st_size > MAX_INPUT:
        raise ValueError('input must be a regular file no larger than 32 MiB')
    with source.open('rb') as handle:
        data = handle.read(MAX_INPUT + 1)
    if (layout.get('schema_version') != 1
            or len(data) != layout['input_bytes']
            or digest(data) != layout['input_sha256']):
        raise ValueError('unsupported executable: size or SHA-256 differs from the reviewed layout; no files generated')
    profile = materialize(layout['profile'], data)
    endpoints = materialize(layout['endpoints'], data)
    if set(endpoints) != {'authentication', 'lobby'}:
        raise ValueError('layout must define exactly two application endpoints')
    files = {
        'inputs/client.elf': data,
        'profile.json': json_bytes(profile),
        'endpoints.json': json_bytes({
            'notice': 'Private application DNS names; never publish. No DNS changes were made.',
            'transport': 'tcp', 'port': 3074, 'hosts': endpoints,
        }),
    }
    files['setup-manifest.json'] = json_bytes({
        'schema_version': 1,
        'client_label': layout['client_label'],
        'source_sha256': digest(data),
        'layout_sha256': digest(json_bytes(layout)),
        'files': [{'path': name, 'bytes': len(content), 'sha256': digest(content)}
                  for name, content in files.items()],
        'validation': 'Input identity and extraction only; run the server configuration check next. No console acceptance implied.',
    })
    output = Path(output).absolute()
    # Refuse linked roots/parents and linked destinations before making changes.
    for path in [output, *output.parents, *(output / name for name in files), output / 'inputs']:
        if path.is_symlink() or getattr(path, 'is_junction', lambda: False)():
            raise ValueError('output must not traverse a symlink or junction')
    for name, content in files.items():
        target = output / name
        if target.exists() and (not target.is_file() or target.read_bytes() != content):
            raise ValueError(f'refusing to replace existing {name}; preserve the deployment or choose a new output directory')
    for name, content in files.items():
        target = output / name
        if not target.exists():
            target.parent.mkdir(parents=True, exist_ok=True)
            with target.open('xb') as handle:
                handle.write(content)
        if digest(target.read_bytes()) != digest(content):
            raise ValueError(f'output verification failed: {name}')
    return files


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--executable', required=True, type=Path,
                        help="User's already decrypted multiplayer executable")
    parser.add_argument('--output', type=Path, default=ROOT / 'game',
                        help='Private output directory (default: project game/)')
    args = parser.parse_args()
    try:
        files = prepare(args.executable, args.output, json.loads(LAYOUT.read_text(encoding='utf-8')))
    except (OSError, ValueError, KeyError) as error:
        parser.exit(1, f'prepare.failed: {error}\n')
    print(f'prepare.complete files={len(files)}; next run --check-config, then --write-bundle')


if __name__ == '__main__':
    main()
