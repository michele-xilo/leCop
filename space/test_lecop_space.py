import json
import os
import tempfile
import unittest

from lecop_space import (
    Refused,
    dataset_choices,
    fit_status,
    load_fits,
    ordered_choices,
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


def lister(tree):
    """A stand-in for the Hub's tree listing: `tree` maps a folder to its (path, size) entries."""
    return lambda folder: iter(tree.get(folder, []))


META = [("meta/info.json", 100), ("meta/episodes.jsonl", 200)]
DATA = [("data/chunk-000/episode_000000.parquet", 1000)]


class PlanDownload(unittest.TestCase):
    def test_fetches_meta_and_data_listed_by_folder(self):
        files, total = plan_download(lister({"meta": META, "data": DATA}), 10_000, 100)
        self.assertEqual(files, ["data/chunk-000/episode_000000.parquet", "meta/episodes.jsonl", "meta/info.json"])
        self.assertEqual(total, 1300)

    def test_finds_info_json_in_a_repo_with_more_files_than_one_listing_returns(self):
        # The Hub's dataset info stops at 100,000 file names, in sorted order, so a repo with more files than that
        # loses meta/ after data/. Listing meta/ on its own finds it whatever data/ holds.
        data = (("data/chunk-%03d/episode_%06d.parquet" % (n // 1000, n), 1) for n in range(100_001))
        with self.assertRaises(Refused) as e:
            plan_download(lister({"meta": META, "data": data}), 10**9, 5_000)
        self.assertNotIn("info.json", str(e.exception))
        self.assertIn("more than 5,000 files", str(e.exception))

    def test_stops_reading_the_listing_once_over_the_file_cap(self):
        def endless():
            n = 0
            while True:
                yield f"data/{n}.parquet", 1
                n += 1

        with self.assertRaises(Refused):
            plan_download(lister({"meta": META, "data": endless()}), 10**9, 50)

    def test_over_the_byte_cap_is_refused_with_the_sizes(self):
        with self.assertRaises(Refused) as e:
            plan_download(lister({"meta": META, "data": DATA}), 1000, 100)
        self.assertIn("over this Space's limit of 1.0 KB", str(e.exception))

    def test_without_info_json_is_refused_before_listing_data(self):
        def data():
            raise AssertionError("data/ listed although meta/ has no info.json")
            yield

        with self.assertRaises(Refused) as e:
            plan_download(lister({"meta": [("meta/stats.json", 1)], "data": data()}), 10, 10)
        self.assertIn("no `meta/info.json`", str(e.exception))

    def test_unknown_sizes_count_as_zero(self):
        self.assertEqual(plan_download(lister({"meta": [("meta/info.json", None)]}), 0, 1), (["meta/info.json"], 0))


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


class FitStatus(unittest.TestCase):
    def test_refusals_from_the_caps_carry_their_reason(self):
        cases = [
            (lister({"meta": META, "data": DATA}), 10_000, 2, "too_many_files"),
            (lister({"meta": META, "data": DATA}), 1000, 100, "too_big"),
            (lister({"meta": [("meta/stats.json", 1)]}), 10, 10, "not_lerobot"),
        ]
        for folders, cap_bytes, cap_files, reason in cases:
            with self.subTest(reason=reason), self.assertRaises(Refused) as e:
                plan_download(folders, cap_bytes, cap_files)
            self.assertEqual(e.exception.reason, reason)

    def test_status_is_fits_or_the_reason(self):
        self.assertEqual(fit_status(lister({"meta": META, "data": DATA}), 10_000, 100), "fits")
        self.assertEqual(fit_status(lister({"meta": META, "data": DATA}), 10_000, 2), "too_many_files")


class LoadFits(unittest.TestCase):
    def write(self, content):
        f = tempfile.NamedTemporaryFile("w", suffix=".json", delete=False)
        f.write(content)
        f.close()
        self.addCleanup(os.remove, f.name)
        return f.name

    def test_reads_the_status_built_for_the_same_caps(self):
        path = self.write(json.dumps({"cap_bytes": 10, "cap_files": 2, "status": {"a/b": "fits"}}))
        self.assertEqual(load_fits(path, 10, 2), {"a/b": "fits"})

    def test_ignores_a_file_built_for_other_caps(self):
        path = self.write(json.dumps({"cap_bytes": 10, "cap_files": 2, "status": {"a/b": "fits"}}))
        self.assertEqual(load_fits(path, 10, 3), {})

    def test_missing_or_broken_file_is_empty(self):
        self.assertEqual(load_fits("/nonexistent/fits.json", 1, 1), {})
        self.assertEqual(load_fits(self.write("{"), 1, 1), {})


class OrderedChoices(unittest.TestCase):
    def test_fitting_first_then_unknown_then_the_rest_labelled_and_non_lerobot_left_out(self):
        repos = ["a/big", "b/ok", "c/new", "d/many", "e/ok", "f/notlr"]
        status = {"a/big": "too_big", "b/ok": "fits", "d/many": "too_many_files", "e/ok": "fits", "f/notlr": "not_lerobot"}
        self.assertEqual(
            ordered_choices(repos, status, 2_000_000_000, 2000),
            [
                ("b/ok", "b/ok"),
                ("e/ok", "e/ok"),
                ("c/new", "c/new"),
                ("a/big: over 2.0 GB", "a/big"),
                ("d/many: over 2,000 files", "d/many"),
            ],
        )

    def test_an_unknown_status_counts_as_not_classified(self):
        self.assertEqual(ordered_choices(["a/b"], {"a/b": "error"}, 1, 1), [("a/b", "a/b")])


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
