# TDD for bug fixes

When fixing a bug, **write a failing test before the fix**. The sequence is non-negotiable: diagnose → failing test → minimal fix → green test → quality checks. Never write the fix and the test in the same step — if you do, you have no evidence the test actually exercises the bug.

See `modus-operandi.md` for test structure conventions (file placement, style, real-data gating). This file is about the *workflow* for bug fixes specifically. A performance regression is a bug too: see `performance.md` for its benchmark-first variant.

## The loop

### 1. Diagnose first

Trace the bug to its root cause before touching any code. Identify the exact file, line, and conditions that trigger it. If you can't articulate the failure precisely ("when X happens, Y returns Z instead of W"), you're not ready to write a test.

For bugs that span the pipeline (a dataset lecop rejects or mis-reports), pin down which stage actually fails: the metadata loading in `dataset` (info.json, episodes metadata, path templates), the file reading in `validate`, a specific check, or the CLI output. A wrong report on a real dataset is still caused by something specific: find the field, column or value, and test the stage that handled it.

### 2. Write the failing test

The test must:

- **Assert the desired post-fix behaviour**, not the current buggy behaviour. The point is to encode the contract the fix must satisfy.
- **Target the smallest testable unit** that contains the bug. In lecop, prefer this order: pure function (path templating, row-run computation, a single timestamp or range check) > `validate()` on a synthetic dataset built in a temp dir > the CLI on a real dataset. Drop one level only if the level above doesn't actually contain the bug.
- **Reproduce the bug deterministically** — no flakiness. A test that needs a real dataset from `datasets/`, the network or a particular thread schedule cannot be the failing test; build a synthetic fixture that mimics the real schema and the real quirk instead.

If the buggy code lives somewhere hard to test (inline in the long per-episode loop, in the CLI's `main`), **extract a pure function first** — preserving the buggy behaviour exactly — and write the test against the extracted function. Refactor in a behaviour-preserving step, then test, then fix.

### 3. Run the test and verify it fails *for the right reason*

Read the failure message. It must reflect the bug you diagnosed in step 1, not a setup mistake (missing fixture file, wrong column name or type in the fixture writer, wrong import). If the test fails for an unrelated reason, fix the test before moving on — a test that fails for the wrong reason will pass for the wrong reason later.

### 4. Apply the minimal fix

Change only what's needed to flip the failing assertion green. Resist scope creep — no surrounding cleanup, refactors, or "while I'm here" tweaks. Those go in a separate step (or PR).

### 5. Re-run the failing test, then the full local suite

The targeted test must pass. Then run `cargo test` to catch regressions, then `cargo fmt --check && cargo clippy --all-targets -- -D warnings`. For bugs that affect what lecop reports on real data, rerun `lecop validate` on every dataset in `datasets/` (expect no new findings on the healthy ones) and redo the mutation test before declaring victory: passing synthetic tests do not prove the real datasets still load. If the fix touched a hot path, run `cargo bench` and confirm no regression (`performance.md`).

### 6. Communicate the trail

When reporting the fix, state explicitly: *what failed, why it failed, what changed, what now passes*. The test names should make this obvious without needing the diff.

## Anti-patterns

- **Writing the fix and test together.** You have no evidence the test exercises the bug. The test passing means nothing.
- **Asserting the buggy behaviour to "lock it in"**, then changing the assertion when fixing. The test gives no signal — it always passes against current code.
- **Skipping the failure-reason check.** A test that fails because of a missing setup will pass when you add the setup — even if your fix is wrong.
- **Bundling unrelated cleanup with the fix.** If the cleanup breaks something, the bisect is muddier and the review harder.
- **Testing through too many layers.** A bug in a pure check (a tolerance, an off-by-one in a row range) shouldn't require a dataset on disk or the CLI. Extract and test the unit.
- **Hiding bugs behind a skipped test.** If the bug only repros on a big real dataset, the underlying logic still has a deterministic input/output you can exercise on a synthetic one. Find it and test it.
- **"Fixing" a false positive by loosening a check blindly.** Establish from the LeRobot source or a real dataset what is actually valid, then encode that in the test.
