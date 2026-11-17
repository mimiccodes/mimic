<!-- Into `dev` (a release into `main` comes from `dev`). One change per pull request. -->

**What and why:**

**Checklist:**
- [ ] A test: a golden program (`tests/fixtures/run`), an expected failure (`tests/fixtures/fail`) or a unit test
- [ ] Anything a user can see is under **Unreleased** in `CHANGELOG.md`, and in the tutorial and reference where it belongs
- [ ] A reported problem has its check in `tools/gaps.sh`
- [ ] Code generation or runtime changes: benchmarks run, numbers measured
- [ ] Closes a step of `docs/versions.md`: its **Done when** checks pass (say how)
