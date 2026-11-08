# Issues and priorities

Work that is not being done right now must not live only in a chat or in a note. Every new package of
work gets a GitHub issue with a priority, so the backlog is visible to everyone and can be ordered.

## When to create an issue

Create one, without asking (this rule is the standing authorization), whenever we find a package of work we
are not doing in the current change:

- a bug or a false positive/negative found while testing (on a real dataset, in review, in a benchmark),
- a gap in coverage or a check we decided to skip ("not checked yet" in a note is a missing issue),
- a performance problem or opportunity a profile or benchmark showed,
- a follow-up named in a PR description, review or `docs/agent/` note,
- a feature idea the user mentions that we are not building now.

One issue per independently shippable package. If two items could be merged and released separately, they
are two issues. Do not create an issue for something we are fixing in the current change; the PR is enough.
Before creating one, search for an existing issue (`gh issue list --search "<words>"`) and add a comment
instead of duplicating.

## What goes in the issue

- **Title**: imperative and specific ("Check observation.state exists", not "state stuff").
- **Body**: why it matters with the evidence (dataset, command, numbers, links to the PR or note), the
  proposed scope, acceptance criteria that can be tested, and what is explicitly out of scope.
- **Priority line**: `Priority: P1, because <one sentence>`, plus the matching label.
- Public repo: never put dataset contents, credentials, tokens or private data in an issue. Name public
  datasets and give figures instead.

## Priorities

Exactly one priority label per issue:

| Label | Meaning | Examples |
|---|---|---|
| `P0` | Wrong results or crash on valid input, security problem, broken release or CI | a panic on a real dataset, false errors on healthy data |
| `P1` | Core value: speed, correctness of the main checks, anything blocking users or a release | slow validate on large datasets, missing check for a common fault |
| `P2` | Valuable improvement or new check, not blocking | new semantic warning, better messages, `--json` |
| `P3` | Nice to have, idea, polish | extra formats, cosmetic output |

Also one type label: `bug`, `feature`, `perf`, `docs` or `chore`.

Re-triage when evidence changes (a P2 that real users hit becomes P1) and say why in a comment.

## Working the backlog

- Pick work in priority order (P0 first) unless the user says otherwise. When the user asks for something
  that is lower priority than open P0/P1 issues, do it, and mention the open higher-priority items.
- Branch names carry the issue number (`feat/12-observation-state-check`), the PR body says `Closes #12`,
  and the PR description's evidence answers the issue's acceptance criteria.
- Issues track *what to do*; `docs/agent/decisions.md` records *what was decided and why*. Do not duplicate
  one in the other: link them.
- If a task grows and splits, create the new issues and link them from the original before closing it.

## Commands

```bash
gh issue create --title "..." --label "P2,feature" --body-file /path/to/body.md
gh issue list --label P0,P1 --state open
gh issue comment 12 --body "Re-triaged to P1: ..."
```
