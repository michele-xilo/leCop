import json
import subprocess

import pytest

import lecop
from lecop import _binary


@pytest.fixture
def broken_dataset(tmp_path):
    (tmp_path / "meta").mkdir()
    (tmp_path / "meta" / "info.json").write_text("{}")
    return tmp_path


def test_validate_returns_the_report_with_errors_without_raising(broken_dataset):
    report = lecop.validate(broken_dataset)
    assert set(report) == {"errors", "warnings", "ignored", "issues"}
    assert report["errors"] >= 1
    assert {i["code"] for i in report["issues"]} == {"cannot_open"}
    assert set(report["issues"][0]) == {"dataset", "severity", "episode", "code", "message"}


def test_report_equals_the_cli_json(broken_dataset):
    cli = subprocess.run([_binary(), "validate", "--format", "json", str(broken_dataset)], capture_output=True, text=True)
    assert lecop.validate(str(broken_dataset)) == json.loads(cli.stdout)


def test_path_that_is_not_a_directory_raises(tmp_path):
    with pytest.raises(lecop.LecopError, match="not a directory"):
        lecop.validate(tmp_path / "missing")


def test_options_reach_the_cli(broken_dataset):
    report = lecop.validate(broken_dataset, jobs=1, ignore=["idle_episode"], select=["idle_episode"])
    assert report["errors"] == 1  # `cannot_open` cannot be hidden by --select or --ignore

    with pytest.raises(lecop.LecopError, match="nope"):
        lecop.validate(broken_dataset, ignore=["nope"])
