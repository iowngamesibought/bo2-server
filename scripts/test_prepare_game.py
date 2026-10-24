"""Synthetic tests; no client files required."""
import copy
import json
import tempfile
import unittest
from pathlib import Path
from prepare_game import digest, materialize, prepare


class PreparationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source = self.root / 'input'
        self.data = b'example_token\0auth.example\0lobby.example\0'
        self.source.write_bytes(self.data)
        self.layout = {
            'schema_version': 1, 'client_label': 'synthetic',
            'input_bytes': len(self.data), 'input_sha256': digest(self.data),
            'profile': {'token': {'slices': [[0, 7], [7, 6]]}},
            'endpoints': {'authentication': {'slices': [[14, 12]]},
                          'lobby': {'slices': [[27, 13]]}},
        }
        # Each synthetic endpoint is sliced without its terminating NUL.
        for role, token in [('authentication', b'auth.example'), ('lobby', b'lobby.example')]:
            self.layout['endpoints'][role] = {'slices': [[self.data.index(token), len(token)]]}
        self.output = self.root / 'game'

    def test_generates_all_files_and_verified_manifest_idempotently(self):
        files = prepare(self.source, self.output, self.layout)
        self.assertEqual(set(files), {'inputs/client.elf', 'profile.json', 'endpoints.json', 'setup-manifest.json'})
        manifest = json.loads(files['setup-manifest.json'])
        for row in manifest['files']:
            data = (self.output / row['path']).read_bytes()
            self.assertEqual(row['sha256'], digest(data))
            self.assertEqual(row['bytes'], len(data))
        self.assertEqual(json.loads(files['profile.json'])['token'], 'example_token')
        before = {p: p.stat().st_mtime_ns for p in self.output.rglob('*') if p.is_file()}
        self.assertEqual(files, prepare(self.source, self.output, self.layout))
        self.assertEqual(before, {p: p.stat().st_mtime_ns for p in before})

    def test_wrong_executable_writes_nothing(self):
        self.source.write_bytes(b'X' + self.data[1:])
        with self.assertRaisesRegex(ValueError, 'unsupported executable'):
            prepare(self.source, self.output, self.layout)
        self.assertFalse(self.output.exists())

    def test_conflicting_profile_preserved_before_any_copy(self):
        self.output.mkdir()
        profile = self.output / 'profile.json'
        profile.write_bytes(b'existing private configuration')
        with self.assertRaisesRegex(ValueError, 'refusing to replace'):
            prepare(self.source, self.output, self.layout)
        self.assertEqual(profile.read_bytes(), b'existing private configuration')
        self.assertFalse((self.output / 'inputs').exists())

    def test_bad_layout_bounds_and_tokens_rejected(self):
        for spans in [[], [[-1, 2]], [[0, 128]], [[0, 14]], [[999, 2]], [[True, 1]]]:
            with self.subTest(spans=spans), self.assertRaises(ValueError):
                materialize({'slices': spans}, self.data)
        layout = copy.deepcopy(self.layout)
        del layout['endpoints']['lobby']
        with self.assertRaises(ValueError):
            prepare(self.source, self.output, layout)
        self.assertFalse(self.output.exists())


if __name__ == '__main__':
    unittest.main()
