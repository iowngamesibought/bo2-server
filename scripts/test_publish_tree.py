"""Synthetic negative checks for the source publication gate."""
import tempfile
import subprocess
import unittest
from pathlib import Path
from check_publish_tree import audit, publication_candidates

class PublicationGateTests(unittest.TestCase):
    def test_working_tree_deletions_are_excluded_but_tracked_private_files_are_audited(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            def git(*args):
                subprocess.run(['git', *args], cwd=root, check=True, capture_output=True)
            git('init')
            (root / 'old.md').write_text('removed source', encoding='utf-8')
            (root / '.gitignore').write_text('/game/\n', encoding='utf-8')
            (root / 'game').mkdir()
            (root / 'game/private.json').write_text('{}', encoding='utf-8')
            git('add', 'old.md', '.gitignore')
            git('add', '-f', 'game/private.json')
            (root / 'old.md').unlink()
            (root / 'README.md').write_text('new source', encoding='utf-8')
            paths = publication_candidates(root)
            self.assertEqual(paths, ['.gitignore', 'README.md', 'game/private.json'])
            self.assertEqual([name for name, _ in audit(root, paths)], ['game/private.json'])

    def check_case(self, name, data, terms=()):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            path = root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
            return audit(root, [name], terms)

    def test_private_files_fail_even_if_force_added(self):
        self.assertTrue(self.check_case('game/profile.json', b'{}'))
        self.assertTrue(self.check_case('bo2-server.toml', b'{}'))
        self.assertTrue(self.check_case('runtime/state.sqlite', b'{}'))

    def test_disguised_binary_is_not_source(self):
        self.assertTrue(self.check_case('src/example.rs', b'\x7fELFbinary'))

    def test_private_terms_are_not_embedded_in_gate(self):
        self.assertTrue(self.check_case('docs/example.md', b'sample_brand', ['sample_brand']))
        self.assertFalse(self.check_case('docs/example.md', b'generic deployment', ['sample_brand']))

    def test_fixture_requires_provenance(self):
        self.assertTrue(self.check_case('tests/fixtures/profile.json', b'{}'))
        self.assertTrue(self.check_case('tests/fixtures/unreviewed.json', b'{"notice":"Public synthetic"}'))

if __name__ == '__main__': unittest.main()
