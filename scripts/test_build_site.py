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
            "wall_ms": wall_ms, "runs": 5, "cache": "warm", "threads": 10, "lecop_version": "0.3.1",
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
        for text in ["Apple M2 Pro", "0.3.1", "29ff292", "hf download lerobot/pusht", "lecop validate", "10 ms"]:
            self.assertIn(text, page)

    def test_home_lists_every_record_without_javascript(self):
        recs = [record(), record("v3_pusht", fmt="v3.0", wall_ms=12.0)]
        home = build_site.home_page(recs)
        self.assertEqual(home.count('class="row"'), 2)
        self.assertIn("datasets/v3_pusht.html", home)

    def test_wall_time_shows_one_decimal_only_below_ten_ms(self):
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
                for bg in ["bg", "subtle"]:
                    self.assertGreaterEqual(self.ratio(p[fg], p[bg]), 4.5, f"{theme} {fg} on {bg}")


if __name__ == "__main__":
    unittest.main()
