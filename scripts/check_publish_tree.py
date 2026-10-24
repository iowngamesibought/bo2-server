"""Audit Git publication candidates; optionally export only the audited sources."""
import argparse
import json
import re
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ALLOWED_ROOT = {'AGENTS.md', 'CLAUDE.md', 'README.md', 'Cargo.toml', 'Cargo.lock',
    'rust-toolchain.toml', '.gitignore', '.gitattributes', 'bo2-server.example.toml'}
ALLOWED_SUFFIX = {'.rs', '.md', '.toml', '.json', '.py', '.yml', '.yaml'}
ALLOWED_DIRS = {'src', 'tests', 'docs', 'scripts', 'config', '.github'}
SKILLS = {f'{folder}/skills/{skill}/SKILL.md' for folder in ('.claude', '.agents')
    for skill in ('pick-game-files', 'dns-override')}
REVIEWED_FIXTURES = {'tests/fixtures/synthetic.json', 'tests/fixtures/league06-results.json',
    'tests/fixtures/stats-vectors.json', 'tests/fixtures/archive-vectors.json', 'tests/fixtures/profile.json'}
MAGIC = (b'\x7fELF', b'7z\xbc\xaf\x27\x1c', b'PK\x03\x04', b'MZ',
    b'\x0a\x0d\x0d\x0a', b'\xd4\xc3\xb2\xa1', b'\xa1\xb2\xc3\xd4')


def audit(root, paths, terms=()):
    failures = []
    for name in paths:
        p = root / name
        if not p.is_file() or p.is_symlink() or not p.resolve().is_relative_to(root.resolve()):
            failures.append((name, 'not a regular contained source file')); continue
        parts = Path(name).parts
        if name not in ALLOWED_ROOT and name not in SKILLS and (parts[0] not in ALLOWED_DIRS or p.suffix not in ALLOWED_SUFFIX):
            failures.append((name, 'not on source allowlist')); continue
        if p.stat().st_size > 1_000_000:
            failures.append((name, 'unexpected large source file')); continue
        data = p.read_bytes()
        if data.startswith(MAGIC) or b'\x00' in data:
            failures.append((name, 'binary signature or NUL byte')); continue
        try: text = data.decode('utf-8-sig')
        except UnicodeDecodeError:
            failures.append((name, 'not UTF-8 source')); continue
        if re.search(r'\b[A-Z]{4}[-_]?[0-9]{5}\b', text):
            failures.append((name, 'possible product identifier'))
        # Public examples may contain loopback only, never workstation addresses.
        for address in re.findall(r'(?<![\w.])(?:[0-9]{1,3}\.){3}[0-9]{1,3}(?![\w.])', text):
            if address not in ('127.0.0.1', '127.0.0.2', '0.0.0.0'):
                failures.append((name, 'literal non-loopback address'))
        if re.search(r'(?:[A-Za-z]:[/\\](?:Users|projects)|/home/[A-Za-z0-9_.-]+/)', text):
            failures.append((name, 'workstation-specific path'))
        haystack = text.casefold()
        for term in terms:
            if term.casefold() in haystack:
                failures.append((name, 'private identifying term')); break
        if name.startswith('tests/fixtures/'):
            if name not in REVIEWED_FIXTURES:
                failures.append((name, 'fixture requires explicit provenance review'))
            else:
                try: fixture = json.loads(text)
                except json.JSONDecodeError:
                    failures.append((name, 'invalid fixture JSON')); continue
                if 'Public synthetic' not in fixture.get('notice', ''):
                    failures.append((name, 'missing synthetic notice'))
    return failures


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--private-terms', type=Path, help='Optional ignored JSON array of identifying terms to reject')
    parser.add_argument('--export', type=Path, help='New directory outside the repository; sources only, no Git metadata')
    args = parser.parse_args()
    result = subprocess.run(['git', 'ls-files', '-z', '--cached', '--others', '--exclude-standard'],
        cwd=ROOT, check=True, capture_output=True)
    paths = sorted(set(p for p in result.stdout.decode('utf-8').split('\0') if p))
    terms = json.loads(args.private_terms.read_text(encoding='utf-8')) if args.private_terms else []
    if not isinstance(terms, list) or any(not isinstance(t, str) or not t for t in terms):
        parser.error('private terms must be a list of nonempty strings')
    failures = audit(ROOT, paths, terms)
    # Ignore rules are essential, but already tracked private files must still fail above.
    ignored = ['game/audit-probe', 'bo2-server.toml', 'runtime/audit-probe']
    for path in ignored:
        check = subprocess.run(['git', 'check-ignore', '-q', '--no-index', path], cwd=ROOT)
        if check.returncode != 0: failures.append((path, 'required ignore rule missing'))
    print(json.dumps({'checked_files': len(paths), 'failures': failures}, indent=2))
    if failures: raise SystemExit(1)
    if args.export:
        destination = args.export.resolve()
        if destination == ROOT or destination.is_relative_to(ROOT) or ROOT.is_relative_to(destination):
            parser.error('export must be separate from repository')
        destination.mkdir(parents=True, exist_ok=False)
        for name in paths:
            target = destination / name; target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / name, target)
        print(f'Exported {len(paths)} audited source files; private inputs and Git metadata excluded.')

if __name__ == '__main__': main()
