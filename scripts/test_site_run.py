import copy
import json
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import site_run

REPORT = {
    "errors": 0,
    "warnings": 1,
    "ignored": {},
    "issues": [
        {
            "dataset": "/home/someone/datasets/pusht",
            "severity": "warning",
            "episode": 54,
            "code": "action_jump",
            "message": "1 frame-to-frame jump",
        },
    ],
}
INFO = {"codebase_version": "v2.1", "robot_type": "unknown", "total_episodes": 206, "total_frames": 25650, "fps": 10}
ENTRY = {"name": "pusht", "repo": "lerobot/pusht", "revision": "6e1d9c95aaf8abb19342e723935b6af7c54b5903"}
META = {
    "lecop_version": "0.3.1",
    "lecop_commit": "29ff292",
    "rustc": "1.99.1",
    "machine": "Apple M2 Pro",
    "os": "Darwin",
    "threads": 10,
    "date": "2026-11-17",
}


def record():
    return site_run.build_record(ENTRY, INFO, REPORT, [9.0, 11.0, 10.0], META)


class RegistryTest(unittest.TestCase):
    def test_skips_comments_and_reads_name_repo_revision(self):
        text = "# comment\npusht\tlerobot/pusht\tabc123\t0\t3\taction_jump=2\n\n# more\nx\tr/x\tdef456\t0\t0\t-\n"
        self.assertEqual(
            site_run.read_registry(text),
            [
                {"name": "pusht", "repo": "lerobot/pusht", "revision": "abc123"},
                {"name": "x", "repo": "r/x", "revision": "def456"},
            ],
        )


class ScanListTest(unittest.TestCase):
    def test_names_records_owner__name_and_reads_repo_revision(self):
        text = "# header\n# repo\trevision\tversion\n" "lerobot/libero\tabc123\tv3.0\t1.94\t1693\tav1\tno\n"
        self.assertEqual(
            site_run.read_scan_list(text),
            [{"name": "lerobot__libero", "repo": "lerobot/libero", "revision": "abc123"}],
        )

    def test_a_scan_record_is_valid_and_has_no_local_path(self):
        entry = site_run.read_scan_list("lerobot/libero\tabc123\tv3.0\t1.94\t1693\tav1\tno\n")[0]
        r = site_run.build_record(entry, INFO, REPORT, [10.0], META)
        self.assertEqual(site_run.validate(r, site_run.load_schema()), [])
        self.assertEqual(set(r) - {"dataset", "run", "errors", "warnings", "issues"}, {"name", "repo", "revision"})


class RecordTest(unittest.TestCase):
    def test_wall_time_is_the_median_of_the_samples(self):
        self.assertEqual(record()["run"]["wall_ms"], 10.0)

    def test_dataset_facts_come_from_info_json(self):
        d = record()["dataset"]
        self.assertEqual((d["format"], d["episodes"], d["frames"], d["fps"]), ("v2.1", 206, 25650, 10))

    def test_issues_drop_the_local_dataset_path(self):
        issues = record()["issues"]
        self.assertEqual(issues, [{"severity": "warning", "episode": 54, "code": "action_jump", "message": "1 frame-to-frame jump"}])
        self.assertNotIn("/home/someone", json.dumps(record()))

    def test_dataset_wide_issue_has_null_episode(self):
        report = copy.deepcopy(REPORT)
        del report["issues"][0]["episode"]
        r = site_run.build_record(ENTRY, INFO, report, [10.0], META)
        self.assertIsNone(r["issues"][0]["episode"])

    def test_counts_match_the_report(self):
        r = record()
        self.assertEqual((r["errors"], r["warnings"]), (0, 1))


class SchemaTest(unittest.TestCase):
    schema = site_run.load_schema()

    def test_a_built_record_is_valid(self):
        self.assertEqual(site_run.validate(record(), self.schema), [])

    def test_missing_wall_time_is_rejected(self):
        r = record()
        del r["run"]["wall_ms"]
        self.assertTrue(any("wall_ms" in e for e in site_run.validate(r, self.schema)))

    def test_missing_machine_is_rejected(self):
        r = record()
        del r["run"]["machine"]
        self.assertTrue(any("machine" in e for e in site_run.validate(r, self.schema)))

    def test_wrong_type_is_rejected(self):
        r = record()
        r["dataset"]["episodes"] = "206"
        self.assertTrue(any("episodes" in e for e in site_run.validate(r, self.schema)))

    def test_bad_severity_is_rejected(self):
        r = record()
        r["issues"][0]["severity"] = "fatal"
        self.assertTrue(any("severity" in e for e in site_run.validate(r, self.schema)))


if __name__ == "__main__":
    unittest.main()
