import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import prepare_editor


class BundleTests(unittest.TestCase):
    def write(self, root, bundle):
        (root / prepare_editor.BUNDLE).write_text(json.dumps(bundle), encoding='utf-8')

    def test_accepts_the_pinned_schema(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write(root, {'schema': 1, 'files': ['index.html', 'src/ui/app.mjs']})
            self.assertEqual(prepare_editor.load_bundle(root), ['index.html', 'src/ui/app.mjs'])

    def test_rejects_missing_manifest(self):
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaises(ValueError):
                prepare_editor.load_bundle(Path(temporary))

    def test_rejects_unknown_schema_and_empty_list(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for bundle in ({'schema': 2, 'files': ['index.html']}, {'schema': 1, 'files': []}, {'schema': 1}):
                with self.subTest(bundle=bundle):
                    self.write(root, bundle)
                    with self.assertRaises(ValueError):
                        prepare_editor.load_bundle(root)

    def test_rejects_paths_outside_the_checkout(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for name in ('/etc/passwd', '../outside', 'src/../../outside', 'src\\ui\\app.mjs', '', 'src//app.mjs'):
                with self.subTest(name=name):
                    self.write(root, {'schema': 1, 'files': [name]})
                    with self.assertRaises(ValueError):
                        prepare_editor.load_bundle(root)

    def test_rejects_duplicates(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write(root, {'schema': 1, 'files': ['index.html', 'index.html']})
            with self.assertRaises(ValueError):
                prepare_editor.load_bundle(root)


if __name__ == '__main__':
    unittest.main()
