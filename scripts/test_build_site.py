import json
import os
import sys
import tempfile
import unittest
from urllib.parse import parse_qs, urlparse

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import build_site


def issue(code, severity="warning", episode=None, message="m"):
    return {"severity": severity, "episode": episode, "code": code, "message": message}


def record(name="pusht", repo="lerobot/pusht", issues=None, wall_ms=10.0, fmt="v2.1"):
    issues = issues if issues is not None else [issue("action_jump", episode=54, message="a <b>jump</b>")]
    return {
        "name": name,
        "repo": repo,
        "revision": "6e1d9c95aaf8abb19342e723935b6af7c54b5903",
        "dataset": {"format": fmt, "episodes": 206, "frames": 25650, "fps": 10, "robot": "unknown"},
        "run": {
            "wall_ms": wall_ms, "runs": 5, "cache": "warm", "threads": 10, "video": "container", "lecop_version": "0.3.1",
            "lecop_commit": "29ff292", "rustc": "1.99.1", "machine": "Apple M2 Pro", "os": "Darwin 25.5.0",
            "date": "2026-11-17",
        },
        "errors": sum(i["severity"] == "error" for i in issues),
        "warnings": sum(i["severity"] == "warning" for i in issues),
        "issues": issues,
    }


class GroupTest(unittest.TestCase):
    def test_errors_come_before_warnings_then_larger_groups_first(self):
        issues = [issue("w_small"), issue("w_big"), issue("w_big"), issue("e_one", "error")]
        self.assertEqual([g["code"] for g in build_site.group_issues(issues)], ["e_one", "w_big", "w_small"])

    def test_a_group_shows_at_most_the_cap_and_counts_the_rest(self):
        issues = [issue("c", episode=i) for i in range(build_site.CAP + 5)]
        (g,) = build_site.group_issues(issues)
        self.assertEqual((len(g["shown"]), g["more"], g["count"]), (build_site.CAP, 5, build_site.CAP + 5))


class StatusTest(unittest.TestCase):
    def test_status_follows_errors_then_warnings(self):
        self.assertEqual(build_site.status(record(issues=[])), "clean")
        self.assertEqual(build_site.status(record()), "warnings")
        self.assertEqual(build_site.status(record(issues=[issue("c", "error")])), "errors")


class PageTest(unittest.TestCase):
    def test_messages_are_html_escaped(self):
        page = build_site.dataset_page(record())
        self.assertIn("a &lt;b&gt;jump&lt;/b&gt;", page)
        self.assertNotIn("<b>jump</b>", page)

    def test_report_link_opens_the_wrong_finding_template_for_this_dataset(self):
        page = build_site.dataset_page(record())
        url = build_site.report_url(record())
        q = parse_qs(urlparse(url).query)
        self.assertEqual(q["template"], ["wrong-finding.yml"])
        self.assertIn("lerobot/pusht", q["dataset"][0])
        self.assertIn(url.replace("&", "&amp;"), page)

    def test_dataset_page_names_the_run_and_the_reproduce_command(self):
        page = build_site.dataset_page(record())
        for text in ["Apple M2 Pro", "0.3.1", "29ff292", "1.99.1", "hf download lerobot/pusht", "lecop validate",
                     "Scanned in 10 ms", "median of 5 runs", "2026-11-17"]:
            self.assertIn(text, page)

    def test_dataset_page_describes_each_finding_group_and_links_the_check_docs(self):
        page = build_site.dataset_page(record())
        self.assertIn(build_site.DESCRIPTIONS["action_jump"], page)
        self.assertIn("About this check", page)

    def test_every_code_in_the_committed_records_has_a_description(self):
        codes = set()
        for name in os.listdir(build_site.RUNS):
            with open(os.path.join(build_site.RUNS, name)) as f:
                codes |= {i["code"] for i in json.load(f)["issues"]}
        self.assertEqual(sorted(codes - set(build_site.DESCRIPTIONS)), [])

    def test_no_page_says_wall_time(self):
        recs = [record(), record("v3_pusht", fmt="v3.0")]
        decode = record(wall_ms=4321.0)
        for page in [build_site.home_page(recs), build_site.dataset_page(recs[0]), build_site.dataset_page(recs[0], decode)]:
            self.assertNotIn("wall time", page.lower())
            self.assertNotIn("wall-time", page.lower())

    def test_home_lists_every_record_without_javascript(self):
        recs = [record(), record("v3_pusht", fmt="v3.0", wall_ms=12.0)]
        home = build_site.home_page(recs)
        self.assertEqual(home.count('class="row"'), 2)
        self.assertIn("datasets/v3_pusht.html", home)

    def test_home_rows_say_scanned_in_and_the_filter_line_gives_the_median(self):
        home = build_site.home_page([record(wall_ms=10.0), record("v3_pusht", fmt="v3.0", wall_ms=12.0)])
        self.assertIn("Scanned in 10 ms", home)
        self.assertIn("Scanned in 12 ms", home)
        self.assertIn("Median scan 12 ms", home)
        self.assertIn('placeholder="Search 2 datasets"', home)

    def test_home_has_no_sort_control_bars_or_format_filter(self):
        home = build_site.home_page([record()])
        for gone in ["<select", 'id="sort"', 'class="bar"', "Most findings", "data-ms"]:
            self.assertNotIn(gone, home)

    def test_home_row_shows_findings_label_and_meta(self):
        rows = build_site.home_page([record(), record("c", "o/c", issues=[]), record("e", "o/e", issues=[issue("x", "error")])])
        for text in ["1 warning<", "Clean<", "1 error<", "v2.1 &middot; 206 episodes &middot; 25,650 frames"]:
            self.assertIn(text, rows)

    def test_unknown_status_in_the_url_falls_back_to_all(self):
        self.assertIn("['all','clean','warnings','errors']", build_site.SCRIPT.replace(" ", ""))

    def test_search_box_needs_javascript_so_it_is_hidden_without_it(self):
        self.assertIn('class="search js-only"', build_site.home_page([record()]))

    def test_a_capped_group_summary_counts_what_expands(self):
        issues = [issue("c", episode=i) for i in range(build_site.CAP + 5)]
        page = build_site.dataset_page(record(issues=issues))
        self.assertIn(f"+ {build_site.CAP - 1} more of {build_site.CAP + 4}", page)
        self.assertIn("+ 5 not shown", page)

    def test_record_fields_are_escaped_in_rows_and_details(self):
        r = record(fmt="<i>v</i>")
        r["run"]["lecop_version"] = "<b>1</b>"
        r["run"]["threads"] = "<u>"
        r["run"]["runs"] = "<s>"
        for page in [build_site.home_page([r]), build_site.dataset_page(r)]:
            for tag in ["<i>", "<b>", "<u>", "<s>"]:
                self.assertNotIn(tag, page.split("</style>", 1)[1])

    def test_search_still_matches_issue_codes(self):
        self.assertIn('data-search="lerobot/pusht pusht action_jump"', build_site.home_page([record()]))

    def test_scan_time_shows_one_decimal_only_below_ten_ms(self):
        self.assertEqual(build_site.fmt_ms(9.3), "9.3 ms")
        self.assertEqual(build_site.fmt_ms(35.5), "36 ms")
        self.assertEqual(build_site.fmt_ms(1234.0), "1.23 s")


class BuildTest(unittest.TestCase):
    def test_build_writes_a_page_per_record_and_an_index(self):
        with tempfile.TemporaryDirectory() as tmp:
            runs, dist = os.path.join(tmp, "runs"), os.path.join(tmp, "dist")
            os.makedirs(runs)
            for r in [record(), record("v3_pusht", fmt="v3.0")]:
                with open(os.path.join(runs, r["name"] + ".json"), "w") as f:
                    json.dump(r, f)
            build_site.build(runs, dist)
            for page in ["index.html", "index.json", "datasets/pusht.html", "datasets/v3_pusht.html"]:
                self.assertTrue(os.path.exists(os.path.join(dist, page)), page)
            with open(os.path.join(dist, "index.json")) as f:
                self.assertEqual([d["name"] for d in json.load(f)], ["pusht", "v3_pusht"])


    def test_a_decode_record_joins_its_dataset_page_and_is_not_a_second_dataset(self):
        default = record(wall_ms=10.0)
        decode = record(issues=[issue("action_jump", episode=54), issue("video_frozen", message="frozen camera")], wall_ms=4321.0)
        decode["run"]["decode"] = True
        with tempfile.TemporaryDirectory() as tmp:
            runs, dist = os.path.join(tmp, "runs"), os.path.join(tmp, "dist")
            os.makedirs(runs)
            for name, r in [("pusht.json", default), ("pusht.decode.json", decode)]:
                with open(os.path.join(runs, name), "w") as f:
                    json.dump(r, f)
            self.assertEqual(build_site.build(runs, dist), 1)
            with open(os.path.join(dist, "index.json")) as f:
                (entry,) = json.load(f)
            self.assertEqual(entry["wall_ms"], 10.0)
            with open(os.path.join(dist, "index.html")) as f:
                self.assertEqual(f.read().count('class="row"'), 1)
            with open(os.path.join(dist, "datasets", "pusht.html")) as f:
                page = f.read()
            for text in ["Scanned in 10 ms", "--decode", "Scanned in 4.32 s", "video_frozen", "frozen camera"]:
                self.assertIn(text, page)


class VideoMarkerTest(unittest.TestCase):
    def test_the_dataset_page_says_how_the_videos_were_checked(self):
        self.assertIn("Videos: metadata only", build_site.dataset_page(record()))
        no_video = record()
        no_video["run"]["video"] = "none"
        self.assertIn("No video", build_site.dataset_page(no_video))
        decode = record()
        decode["run"].update(decode=True, video="decoded")
        self.assertIn("Videos: decoded", build_site.dataset_page(record(), decode))

    def test_a_run_without_the_videos_says_so_and_how_to_reproduce_it(self):
        skipped = record()
        skipped["run"]["video"] = "skipped"
        page = build_site.dataset_page(skipped)
        self.assertIn("Videos: not downloaded, not checked", page)
        self.assertIn('--exclude &quot;videos/*&quot;', page)
        self.assertIn("--ignore video_missing", page)
        self.assertIn("Videos not checked", build_site.row(skipped))

    def test_the_home_row_names_the_videos_only_when_they_were_decoded(self):
        self.assertIn("Videos decoded", build_site.row(record(), decoded=True))
        self.assertNotIn("Videos", build_site.row(record()))


class DownloadsTest(unittest.TestCase):
    DL = {"date": "2026-11-18", "counts": {"a/small": 10, "b/big": 500, "c/gone": None}}

    def test_home_ranks_by_30_day_downloads_with_unknown_counts_last(self):
        recs = [record("c", repo="c/gone"), record("a", repo="a/small"), record("b", repo="b/big")]
        home = build_site.home_page(recs, downloads=self.DL)
        self.assertLess(home.index("b/big"), home.index("a/small"))
        self.assertLess(home.index("a/small"), home.index("c/gone"))
        self.assertIn("500 downloads", home)
        self.assertIn("as of 2026-11-18", home)

    def test_a_failed_fetch_keeps_the_last_published_count(self):
        def get(repo):
            if repo == "x/flaky":
                raise OSError("timeout")
            return {"y/ok": 7, "z/gated": None}[repo]
        dl = build_site.downloads_for(["x/flaky", "y/ok", "z/gated"], get=get,
                                      previous=lambda: ({"x/flaky": 42}, "2026-11-10"), today="2026-11-18")
        self.assertEqual(dl["counts"], {"x/flaky": 42, "y/ok": 7, "z/gated": None})
        self.assertEqual(dl["date"], "2026-11-10")

    def test_counts_fetched_in_full_are_dated_today(self):
        dl = build_site.downloads_for(["y/ok"], get=lambda r: 7, previous=lambda: ({}, None), today="2026-11-18")
        self.assertEqual(dl, {"date": "2026-11-18", "counts": {"y/ok": 7}})

    def test_build_puts_the_counts_in_the_index(self):
        with tempfile.TemporaryDirectory() as tmp:
            runs, dist = os.path.join(tmp, "runs"), os.path.join(tmp, "dist")
            os.makedirs(runs)
            with open(os.path.join(runs, "pusht.json"), "w") as f:
                json.dump(record(), f)
            build_site.build(runs, dist, downloads={"date": "2026-11-18", "counts": {"lerobot/pusht": 1234}})
            with open(os.path.join(dist, "index.json")) as f:
                (entry,) = json.load(f)
            self.assertEqual((entry["downloads"], entry["downloads_date"]), (1234, "2026-11-18"))


class ContrastTest(unittest.TestCase):
    def luminance(self, hex_color):
        c = [int(hex_color[i : i + 2], 16) / 255 for i in (1, 3, 5)]
        c = [x / 12.92 if x <= 0.03928 else ((x + 0.055) / 1.055) ** 2.4 for x in c]
        return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]

    def ratio(self, a, b):
        la, lb = sorted([self.luminance(a), self.luminance(b)], reverse=True)
        return (la + 0.05) / (lb + 0.05)

    def test_text_colours_reach_wcag_aa_on_both_backgrounds_in_both_themes(self):
        for theme, p in build_site.THEMES.items():
            for fg in ["text", "muted", "link", "ok", "warn", "err"]:
                self.assertGreaterEqual(self.ratio(p[fg], p["bg"]), 4.5, f"{theme} {fg} on bg")

    def test_the_stylesheet_only_uses_defined_colour_tokens_and_the_only_background_is_bg(self):
        import re
        used = set(re.findall(r"var\(--(\w+)\)", build_site.css()))
        self.assertLessEqual(used, set(build_site.THEMES["light"]))
        self.assertEqual(sorted(k for k in build_site.THEMES["light"] if k not in {"line", "text", "muted", "link", "ok", "warn", "err"}), ["bg"])


if __name__ == "__main__":
    unittest.main()
