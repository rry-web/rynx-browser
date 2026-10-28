#!/bin/sh
# Make a release: scripts/release.sh 0.2.0-beta.7
#
# On an up-to-date main with the changes listed under "## [Unreleased]" in CHANGELOG.md, this sets
# the version (Cargo.toml, both Cargo.locks, README, CHANGELOG), runs every check, commits and pushes,
# waits for CI to pass, then tags. The tag starts the release workflow, which builds, signs and
# publishes the GitHub release and the crate. Needs the GitHub CLI (gh), signed in.
set -eu
cd "$(dirname "$0")/.."

new=${1:-}
case "$new" in
    "" | v*) echo "usage: scripts/release.sh <version>   (e.g. 0.2.0-beta.7, without the v)"; exit 2 ;;
esac
old=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)

fail() { echo "release: $1" >&2; exit 1; }
[ "$(git branch --show-current)" = "main" ] || fail "switch to main first"
[ -z "$(git status --porcelain)" ] || fail "commit or stash your changes first"
git fetch --quiet origin main
[ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] || fail "main isn't the same as origin/main; pull or push first"
git rev-parse -q --verify "refs/tags/v$new" >/dev/null && fail "v$new is already tagged"
grep -q '^## \[Unreleased\]' CHANGELOG.md || fail 'CHANGELOG.md has no "## [Unreleased]" section to release'
# Something must be listed under it
awk '/^## \[Unreleased\]/{f=1;next} /^## \[/{exit} f && /^- /{found=1} END{exit !found}' CHANGELOG.md \
    || fail 'nothing is listed under "## [Unreleased]" in CHANGELOG.md'

echo "Releasing $new (was $old)"
sed -i.bak "s/^version = \"$old\"/version = \"$new\"/" Cargo.toml && rm Cargo.toml.bak
sed -i.bak "s/^## \[Unreleased\]/## [$new]/" CHANGELOG.md && rm CHANGELOG.md.bak
sed -i.bak "s/$old/$new/g" README.md && rm README.md.bak
# Brings both lock files to the new version (the fuzz targets' records rynx's version too)
cargo check --quiet
cargo check --quiet --manifest-path fuzz/Cargo.toml

# Everything CI checks, so CI doesn't fail after the push
scripts/check.sh

git add Cargo.toml Cargo.lock fuzz/Cargo.lock CHANGELOG.md README.md
git commit --quiet -m "Release $new"
git push --quiet origin main
echo "Pushed; waiting for CI..."
sleep 15
run=$(gh run list --branch main --workflow ci.yml --commit "$(git rev-parse HEAD)" --limit 1 --json databaseId -q '.[0].databaseId')
[ -n "$run" ] || fail "couldn't find the CI run; tag by hand once it passes: git tag -a v$new -m 'rynx $new' && git push origin v$new"
gh run watch "$run" --exit-status >/dev/null || fail "CI failed (gh run view $run); fix it, then tag by hand"

git tag -a "v$new" -m "rynx $new"
git push --quiet origin "v$new"
echo "Tagged v$new: the release workflow is building it."
echo "Follow it with:  gh run watch \$(gh run list --workflow release.yml --limit 1 --json databaseId -q '.[0].databaseId')"
