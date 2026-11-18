import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import hub_missing_files


class MissingTest(unittest.TestCase):
    def test_only_files_under_the_given_folders_are_listed(self):
        remote = ["meta/info.json", "data/chunk-000/file-000.parquet", "videos/cam/file-000.mp4", "README.md"]
        with tempfile.TemporaryDirectory() as tmp:
            os.makedirs(os.path.join(tmp, "meta"))
            open(os.path.join(tmp, "meta", "info.json"), "w").close()
            self.assertEqual(hub_missing_files.missing(remote, tmp, ("meta/", "data/")), ["data/chunk-000/file-000.parquet"])
            self.assertEqual(len(hub_missing_files.missing(remote, tmp)), 3)


if __name__ == "__main__":
    unittest.main()
