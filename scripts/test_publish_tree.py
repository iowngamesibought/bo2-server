"""Synthetic negative checks for the source publication gate."""
import tempfile
import unittest
from pathlib import Path
from check_publish_tree import audit

class PublicationGateTests(unittest.TestCase):
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
