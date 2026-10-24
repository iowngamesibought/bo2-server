"""Copy a user-supplied input into ignored local storage and record its digest."""
import argparse
import hashlib
import json
import shutil
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', required=True, type=Path)
    parser.add_argument('--name', required=True, help='Single local filename, without directories')
    parser.add_argument('--sha256', help='Expected digest from a previously verified input')
    args = parser.parse_args()
    if not args.name or args.name in ('.', '..') or any(c in args.name for c in '/\\:'):
        parser.error('name must be a single filename')
    source = args.source.resolve(strict=True)
    if not source.is_file(): parser.error('source must be a regular file')
    with source.open('rb') as f: digest = hashlib.file_digest(f, 'sha256').hexdigest()
    if args.sha256 and digest != args.sha256.lower(): parser.error('source digest mismatch')
    target_root = ROOT / 'game' / 'inputs'
    target_root.mkdir(parents=True, exist_ok=True)
    target = target_root / args.name
    if target.is_symlink() or not target.resolve().is_relative_to((ROOT / 'game').resolve()):
        parser.error('target must stay inside local game storage')
    if target.exists():
        with target.open('rb') as f: existing = hashlib.file_digest(f, 'sha256').hexdigest()
        if existing != digest: parser.error('refusing to overwrite a different input')
    else:
        with source.open('rb') as src, target.open('xb') as dst: shutil.copyfileobj(src, dst)
    with target.open('rb') as f:
        if hashlib.file_digest(f, 'sha256').hexdigest() != digest: raise RuntimeError('copy mismatch')
    manifest = ROOT / 'game' / 'provisioned-inputs.json'
    data = json.loads(manifest.read_text(encoding='utf-8')) if manifest.exists() else []
    row = {'source': str(source), 'prepared_path': str(target), 'sha256': digest, 'bytes': target.stat().st_size}
    data = [r for r in data if r['prepared_path'] != str(target)] + [row]
    manifest.write_text(json.dumps(data, indent=2) + '\n', encoding='utf-8')
    print('Input verified and copied to ignored local storage; client compatibility is not inferred.')

if __name__ == '__main__': main()
