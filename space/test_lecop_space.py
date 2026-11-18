import unittest

from lecop_space import Refused, group_by_code, parse_repo, plan_download, refuse_access, render_markdown


class ParseRepo(unittest.TestCase):
    def test_owner_and_name(self):
        self.assertEqual(parse_repo(" lerobot/pusht "), ("lerobot/pusht", None))

    def test_revision_after_at(self):
        self.assertEqual(parse_repo("lerobot/pusht@v3.0"), ("lerobot/pusht", "v3.0"))

    def test_hub_url_with_tree(self):
        self.assertEqual(
            parse_repo("https://huggingface.co/datasets/lerobot/pusht/tree/abc123"), ("lerobot/pusht", "abc123")
        )

    def test_hub_url(self):
        self.assertEqual(parse_repo("huggingface.co/datasets/lerobot/pusht/"), ("lerobot/pusht", None))

    def test_rejects_a_name_without_owner_and_path_tricks(self):
        for text in ("", "pusht", "a/b/c", "../etc", "lerobot/.."):
            with self.subTest(text=text), self.assertRaises(Refused):
                parse_repo(text)


class Access(unittest.TestCase):
    def test_public_passes(self):
        refuse_access(False, False)

    def test_private_and_gated_are_refused(self):
        for private, gated in ((True, False), (False, "auto"), (False, "manual")):
            with self.subTest(private=private, gated=gated), self.assertRaises(Refused):
                refuse_access(private, gated)


class PlanDownload(unittest.TestCase):
    FILES = [
        ("README.md", 10),
        ("meta/info.json", 100),
        ("meta/episodes.jsonl", 200),
        ("data/chunk-000/episode_000000.parquet", 1000),
        ("videos/chunk-000/observation.image/episode_000000.mp4", 10**9),
        ("database/x.parquet", 5),
    ]

    def test_fetches_meta_and_data_only(self):
        files, total = plan_download(self.FILES, 10_000)
        self.assertEqual(
            files, ["data/chunk-000/episode_000000.parquet", "meta/episodes.jsonl", "meta/info.json"]
        )
        self.assertEqual(total, 1300)

    def test_over_the_cap_is_refused_with_the_sizes(self):
        with self.assertRaises(Refused) as e:
            plan_download(self.FILES, 1000)
        self.assertIn("1.3 KB", str(e.exception))
        self.assertIn("1.0 KB", str(e.exception))

    def test_without_info_json_is_refused(self):
        with self.assertRaises(Refused):
            plan_download([("data/a.parquet", 1)], 10)

    def test_unknown_sizes_count_as_zero(self):
        self.assertEqual(plan_download([("meta/info.json", None)], 0), (["meta/info.json"], 0))


ISSUES = [
    {"severity": "warning", "episode": 54, "code": "action_jump", "message": "jump a"},
    {"severity": "error", "episode": None, "code": "frame_count", "message": "frames"},
    {"severity": "warning", "episode": 174, "code": "episode_length_outlier", "message": "long"},
    {"severity": "warning", "episode": 159, "code": "action_jump", "message": "jump b"},
]


class Render(unittest.TestCase):
    def test_groups_errors_first_then_codes_by_name_keeping_order_within(self):
        groups = group_by_code(ISSUES)
        self.assertEqual(list(groups), ["frame_count", "action_jump", "episode_length_outlier"])
        self.assertEqual([i["episode"] for i in groups["action_jump"]], [54, 159])

    def test_markdown_lists_every_finding_and_links_the_checks(self):
        report = {"errors": 1, "warnings": 3, "ignored": {}, "issues": ISSUES}
        page = render_markdown("lerobot/pusht", "6e1d9c95aaf8abb19342e723935b6af7c54b5903", report, "0.3.1")
        self.assertIn("1 error, 3 warnings", page)
        self.assertIn("#### `action_jump` (warning, 2)", page)
        self.assertIn("- episode 159: jump b", page)
        self.assertIn("- frames", page)
        self.assertIn("docs/checks.md", page)
        self.assertIn("Video checks were skipped", page)

    def test_clean_report_says_so(self):
        page = render_markdown("a/b", "abc", {"errors": 0, "warnings": 0, "ignored": {}, "issues": []}, "0.3.1")
        self.assertIn("leCop reported nothing", page)


if __name__ == "__main__":
    unittest.main()
