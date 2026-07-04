# Releasing Phonehome

The v0.1.0 code, docs, and changelog are prepared. Cutting the tag + GitHub release is a maintainer action (outward-facing). Do it once the M5 stack is merged to `main` and CI is green.

## Preconditions
- All M5 PRs merged to `main`; `main` CI green (`build-test`, `playwright-smoke`, `docker-smoke`).
- `CHANGELOG.md` has the `[0.1.0]` entry (it does).
- `Cargo.toml` crates and `ui/package.json` are at `0.1.0` (they are).

## Cut the release

```sh
git checkout main && git pull
git tag -a v0.1.0 -m "Phonehome v0.1.0 — first public release"
git push origin v0.1.0

# GitHub release from the changelog section:
gh release create v0.1.0 \
  --title "Phonehome v0.1.0" \
  --notes-file <(awk '/^## \[0.1.0\]/{f=1;next} /^## \[/{f=0} f' CHANGELOG.md)
```

## After

- Verify the release page renders and the `docker compose up` quickstart in `README.md` works from a clean clone.
- **Distribution / soft-launch** (RESEARCH §5, Pi-hole community) waits until a **real anonymized household fixture** replaces the synthetic one and the hero GIF is re-recorded from it — see [D-009](DECISIONS.md). Until then the release is functional but demo media stays synthetic-labeled.
