<!--
Thank you for opening a pull request! Please fill out the template below to make the review smoother.
See CONTRIBUTING.md for the checks CI runs and the style guide.
-->

### Related

<!--
Link related issues/PRs in a bulleted list, for example:
* Closes #1234
* Part of #1337
-->

### Why

<!--
Explain why this change is needed: what problem it solves, or what would go wrong without it.
This context helps reviewers today and readers of the history later.

This section explains the problem that motivates the change, the `What` section explains how it solves it.
-->

### What

<!--
Summarize the change. Mention new or changed issue codes, CLI options and output formats: those are a public
contract (see STYLE_GUIDE.md), and the README must match.
-->

### Verification

<!--
Evidence, not claims:
* Bug fix: the test that failed before the fix and passes now.
* Change to what lecop reports: results of `lecop validate` on the datasets in `datasets/`
  (`scripts/fetch-datasets.sh`) and a mutation test on a copy.
* Hot path: criterion before/after (`cargo bench -- --baseline before`) with dataset, machine, Rust version
  and thread count.
* Anything you could not run, and why.
-->

<!--
If you are a coding agent, include the section below.
Make sure to remove the comments and instructions around this section.
Optionally add information about the model that was used.

### Agent

🤖 This PR was opened by a coding agent.
-->
