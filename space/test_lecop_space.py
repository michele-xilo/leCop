import unittest

from lecop_space import (
    Refused,
    dataset_choices,
    group_by_code,
    notice_html,
    parse_repo,
    plan_download,
    refuse_access,
    render_html,
    site_page,
)


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

    def test_html_lists_every_finding_by_code_and_links_the_checks(self):
        report = {"errors": 1, "warnings": 3, "ignored": {}, "issues": ISSUES}
        page = render_html("lerobot/pusht", "6e1d9c95aaf8abb19342e723935b6af7c54b5903", report, "0.3.1")
        self.assertIn("1 error", page)
        self.assertIn("3 warnings", page)
        self.assertIn('<strong class="lc-mono">action_jump</strong><span class="lc-muted">2</span>', page)
        self.assertIn('<span class="lc-muted">episode 159</span>jump b', page)
        self.assertIn('<span class="lc-muted">dataset</span>frames', page)
        self.assertIn("docs/checks.md", page)
        self.assertIn("video checks were skipped", page)
        self.assertIn("@ 6e1d9c9", page)
        self.assertNotIn("results site", page)

    def test_html_escapes_messages(self):
        issue = {"severity": "warning", "episode": 1, "code": "action_jump", "message": "<script>x</script>"}
        page = render_html("a/b", "abc", {"errors": 0, "warnings": 1, "ignored": {}, "issues": [issue]}, "0.3.1")
        self.assertNotIn("<script>", page)
        self.assertIn("&lt;script&gt;", page)

    def test_html_folds_findings_past_five_and_caps_them(self):
        issues = [{"severity": "warning", "episode": n, "code": "episode_short", "message": "m"} for n in range(250)]
        page = render_html("a/b", "abc", {"errors": 0, "warnings": 250, "ignored": {}, "issues": issues}, "0.3.1")
        self.assertIn("<summary>+ 245 more</summary>", page)
        self.assertIn("episode 199<", page)
        self.assertNotIn("episode 200<", page)
        self.assertIn("+ 50 not shown", page)

    def test_clean_report_says_so(self):
        page = render_html("a/b", "abc", {"errors": 0, "warnings": 0, "ignored": {}, "issues": []}, "0.3.1")
        self.assertIn("Clean", page)
        self.assertIn("leCop reported nothing", page)

    def test_html_links_the_results_site_when_it_has_the_dataset(self):
        url = "https://michele-xilo.github.io/leCop/datasets/pusht.html"
        page = render_html("lerobot/pusht", "abc", {"errors": 0, "warnings": 0, "ignored": {}, "issues": []}, "0.3.1", url)
        self.assertIn(url, page)

    def test_notice_shows_code_spans_and_escapes(self):
        page = notice_html("`<b>` is not a Hub dataset name")
        self.assertIn("Not checked.", page)
        self.assertIn("<code>&lt;b&gt;</code>", page)


class DatasetChoices(unittest.TestCase):
    def test_hub_order_first_then_site_datasets_without_duplicates(self):
        index = [{"repo": "lerobot/pusht"}, {"repo": "c/d"}, {"repo": "c/d"}]
        self.assertEqual(dataset_choices(["a/b", "lerobot/pusht"], index), ["a/b", "lerobot/pusht", "c/d"])

    def test_site_alone_when_the_hub_list_failed(self):
        self.assertEqual(dataset_choices([], [{"repo": "c/d"}]), ["c/d"])


class SitePage(unittest.TestCase):
    INDEX = [{"name": "v3_pusht", "repo": "lerobot/pusht"}, {"name": "aloha", "repo": "lerobot/aloha"}]

    def test_finds_the_record_of_a_repo(self):
        self.assertEqual(site_page("lerobot/aloha", self.INDEX), "https://michele-xilo.github.io/leCop/datasets/aloha.html")

    def test_none_when_the_site_has_no_record(self):
        self.assertIsNone(site_page("x/y", self.INDEX))
        self.assertIsNone(site_page("x/y", []))


if __name__ == "__main__":
    unittest.main()
