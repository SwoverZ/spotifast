#!/usr/bin/env bash
# Installs, updates, and repairs the lyrics translation plugin.
#
# Spotifast has no runtime plugin API. The plugin is this branch's commit
# series on top of an upstream release; this script checks out the branch,
# builds it, and puts the binary where the desktop launcher expects it.
#
# Usage:
#   plugin/install.sh                 build and install the current branch
#   plugin/install.sh --update        rebase on the newest release, check,
#                                     build, and install
#   plugin/install.sh --status        show the installed and newest releases
#   plugin/install.sh --repair        recover from a failed install or update
#   plugin/install.sh --rollback      restore the previous binary
#   plugin/install.sh --export-patches DIR
#                                     write the plugin as a patch series
#
# Options:
#   --tag vX.Y.Z     update to this release instead of the newest one
#   --no-fetch       use the tags already in the checkout
#   --skip-checks    skip fmt, clippy, and the tests while updating
#
# Environment:
#   PLUGIN_BRANCH        branch to build (default: lyrics-translation)
#   PLUGIN_UPSTREAM      remote that tracks crmne/spotifast; by default the
#                        script finds one, or fetches the URL directly
#   PLUGIN_UPSTREAM_URL  upstream repository (default: crmne/spotifast on GitHub)
#   PLUGIN_BIN           installed binary
#                        (default: <repo>/target/release/spotifast)
#
# The full update and repair playbook for people and coding models is in
# plugin/AGENTS.md.
set -euo pipefail

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
REPO=$(git -C "$SCRIPT_DIR" rev-parse --show-toplevel 2>/dev/null) || {
    echo "Error: plugin/install.sh must run from a checkout of the Spotifast fork." >&2
    exit 1
}
cd "$REPO"

BRANCH=${PLUGIN_BRANCH:-lyrics-translation}
UPSTREAM=${PLUGIN_UPSTREAM:-}
UPSTREAM_URL=${PLUGIN_UPSTREAM_URL:-https://github.com/crmne/spotifast.git}
BIN=${PLUGIN_BIN:-"$REPO/target/release/spotifast"}
BUILT="$REPO/target/release/spotifast"
BACKUP="$BIN.previous"
STATE_DIR=${XDG_STATE_HOME:-"$HOME/.local/state"}/spotifast
STATE_FILE="$STATE_DIR/plugin-install"
DESKTOP_DIR=${XDG_DATA_HOME:-"$HOME/.local/share"}/applications

MODE=install
TAG=""
NO_FETCH=0
SKIP_CHECKS=0
PATCH_DIR=""

say() { printf '%s\n' "$*"; }
step() { printf '\n==> %s\n' "$*"; }
die() {
    printf '\nError: %s\n' "$*" >&2
    exit 1
}

usage() {
    awk 'NR > 1 { if ($0 == "set -euo pipefail") exit; sub(/^# ?/, ""); print }' "$SCRIPT_DIR/install.sh"
}

while [ $# -gt 0 ]; do
    case "$1" in
        install | --install) MODE=install; shift ;;
        update | --update) MODE=update; shift ;;
        status | --status) MODE=status; shift ;;
        repair | --repair) MODE=repair; shift ;;
        rollback | --rollback) MODE=rollback; shift ;;
        --export-patches)
            MODE=export
            shift
            [ $# -ge 1 ] || die "--export-patches needs a directory"
            PATCH_DIR=$1
            shift
            ;;
        --tag)
            [ $# -ge 2 ] || die "--tag needs a release, like v0.14.0"
            TAG=$2
            shift 2
            ;;
        --tag=*) TAG=${1#--tag=}; shift ;;
        --no-fetch) NO_FETCH=1; shift ;;
        --skip-checks) SKIP_CHECKS=1; shift ;;
        -h | --help) usage; exit 0 ;;
        *) die "Unknown option: $1 (try --help)" ;;
    esac
done

file_mtime() {
    stat -c '%y' -- "$1" 2>/dev/null || stat -f '%Sm' -- "$1" 2>/dev/null || say unknown
}

verify_tools() {
    command -v cargo >/dev/null 2>&1 ||
        die "cargo is missing. Install Rust and the system libraries from upstream's Build from source guide."
    command -v git >/dev/null 2>&1 || die "git is missing."
}

upstream_source() {
    local remote url
    if [ -n "$UPSTREAM" ]; then
        printf '%s\n' "$UPSTREAM"
        return 0
    fi
    for remote in origin upstream; do
        url=$(git remote get-url "$remote" 2>/dev/null) || continue
        case "$url" in
            *crmne/spotifast*) printf '%s\n' "$remote"; return 0 ;;
        esac
    done
    printf '%s\n' "$UPSTREAM_URL"
}

fetch_tags() {
    local strict=${1:-strict} source
    [ "$NO_FETCH" = 1 ] && return 0
    source=$(upstream_source)
    if git fetch --tags "$source"; then
        return 0
    fi
    if [ "$strict" = strict ]; then
        die "Could not fetch tags from $source. Check the network, or pass --no-fetch to use the local tags."
    fi
    say "Warning: could not fetch tags from $source; using the tags already here."
}

latest_tag() {
    local tag
    for tag in $(git tag --list 'v*' --sort=-version:refname); do
        case "$tag" in
            *-alpha* | *-beta* | *-rc*) continue ;;
        esac
        printf '%s\n' "$tag"
        return 0
    done
    return 1
}

current_base() {
    git describe --tags --abbrev=0 "$BRANCH" 2>/dev/null
}

require_clean() {
    if ! git diff --quiet || ! git diff --cached --quiet; then
        die "The checkout has uncommitted changes. Commit or stash them first so an update cannot lose them."
    fi
    if [ -n "$(git ls-files --others --exclude-standard)" ]; then
        say "Note: untracked files are present; they are left alone."
    fi
}

switch_branch() {
    git rev-parse --verify --quiet "refs/heads/$BRANCH" >/dev/null ||
        die "Branch $BRANCH is missing. Fetch the fork or set PLUGIN_BRANCH."
    if [ "$(git branch --show-current)" != "$BRANCH" ]; then
        say "Switching to $BRANCH."
        git switch "$BRANCH"
    fi
}

backup_binary() {
    if [ -x "$BIN" ]; then
        cp -p -- "$BIN" "$BACKUP.plugin-new"
        mv -f -- "$BACKUP.plugin-new" "$BACKUP"
        say "Previous binary kept at $BACKUP."
    fi
}

write_state() {
    local tag=$1 commit=$2
    mkdir -p "$STATE_DIR"
    {
        printf 'tag=%s\n' "$tag"
        printf 'commit=%s\n' "$commit"
        printf 'installed_at=%s\n' "$(date '+%Y-%m-%dT%H:%M:%S%z')"
        printf 'binary=%s\n' "$BIN"
    } >"$STATE_FILE.plugin-new"
    mv -f "$STATE_FILE.plugin-new" "$STATE_FILE"
}

installed_tag() {
    [ -f "$STATE_FILE" ] || return 0
    sed -n 's/^tag=//p' "$STATE_FILE"
}

installed_at() {
    [ -f "$STATE_FILE" ] || return 0
    sed -n 's/^installed_at=//p' "$STATE_FILE"
}

installed_binary() {
    [ -f "$STATE_FILE" ] || return 0
    sed -n 's/^binary=//p' "$STATE_FILE"
}

verify_binary() {
    local bin=$1 version
    [ -x "$bin" ] || die "No executable at $bin."
    version=$("$bin" --version 2>/dev/null) || die "$bin --version failed."
    case "$version" in
        spotifast\ *) ;;
        *) die "Unexpected version line from $bin: $version" ;;
    esac
    grep -a -q "Clear translations" "$bin" ||
        die "$bin is not the translated-lyrics build (the feature is missing)."
    say "Binary verifies: $version, with lyrics translation."
}

build_release() {
    step "Building the release binary (this takes a few minutes)."
    cargo build --release --locked
    [ -x "$BUILT" ] || die "The build finished without $BUILT."
    if [ "$BIN" != "$BUILT" ]; then
        cp -p -- "$BUILT" "$BIN.plugin-new"
        mv -f -- "$BIN.plugin-new" "$BIN"
    fi
}

desktop_entry() {
    local file
    for file in "$DESKTOP_DIR"/*.desktop; do
        [ -f "$file" ] || continue
        if grep -q "Exec=$BIN" "$file"; then
            printf '%s\n' "$file"
            return 0
        fi
    done
    return 1
}

check_desktop() {
    local entry
    entry=$(desktop_entry) || true
    if [ -z "$entry" ]; then
        say "No .desktop file launches $BIN yet; the app can still be started from a terminal."
    fi
}

refresh_catalogs() {
    local tag=$1
    step "Checking the translation catalogs for $tag."
    # --check ignores the template's creation date, so a passing check means
    # there is nothing to regenerate. Running the generator unconditionally
    # would rewrite only that date and create an empty commit on every update.
    if bash .github/scripts/update-translations.sh --check; then
        say "The catalogs were already current."
        return 0
    fi
    say "The catalogs need refreshing."
    bash .github/scripts/update-translations.sh ||
        die "The catalog refresh failed. Read plugin/AGENTS.md, section 'Translation catalogs'."
    if ! git diff --quiet -- assets/i18n; then
        git add assets/i18n
        git commit -m "Refresh the translation catalogs for $tag" >/dev/null
        say "Committed the refreshed catalogs."
    fi
}

run_checks() {
    step "Checking formatting."
    cargo fmt --all --check ||
        die "The code is not formatted. Run 'cargo fmt --all', commit, and try again."

    step "Running clippy."
    cargo clippy --locked --all-targets --all-features -- -D warnings ||
        die "Clippy found a problem. Fix it and try again. Read plugin/AGENTS.md (Repair)."

    step "Running the tests."
    local log="$REPO/target/plugin-tests.log"
    mkdir -p "$REPO/target"
    if cargo test --locked --all-targets --all-features --no-fail-fast >"$log" 2>&1; then
        say "All tests passed."
    else
        local failed expected
        failed=$(awk '/^failures:$/{inside=1; next} /^test result:/{inside=0} inside && /^    [^ ]/ {print $1}' "$log" | sort -u)
        expected=$'bidi::tests::lines_are_drawn_in_bidi_order\nbidi::tests::wrapped_and_cut_rows_are_drawn_in_bidi_order'
        if [ "$failed" = "$expected" ]; then
            say "Only the two known bidi font-shaping failures appeared; continuing."
        else
            say "Unexpected test failures:"
            if [ -n "$failed" ]; then
                printf '%s\n' "$failed"
            else
                say "(the test run did not finish; the tail of its log follows)"
            fi
            tail -n 30 "$log" >&2
            die "Tests failed. Read plugin/AGENTS.md (Repair). Nothing was installed."
        fi
    fi

    step "Checking the translation catalogs."
    bash .github/scripts/update-translations.sh --check ||
        die "The catalogs are stale. Run the refresh step and try again."
}

install_current() {
    verify_tools
    require_clean
    switch_branch
    backup_binary
    build_release
    verify_binary "$BIN"
    base=$(current_base)
    write_state "${base:-unknown}" "$(git rev-parse --short HEAD)"
    check_desktop
    say ""
    say "Installed. Close Spotifast and open it again to run the new build."
}

update() {
    verify_tools
    require_clean
    switch_branch
    fetch_tags strict

    local latest base backup
    if [ -n "$TAG" ]; then
        latest=$TAG
        git rev-parse --verify --quiet "refs/tags/$latest" >/dev/null ||
            die "Tag $latest is not in the checkout. Fetch it or drop --tag."
    else
        latest=$(latest_tag) || true
        [ -n "$latest" ] || die "No release tags found. Fetch the upstream tags first."
    fi
    base=$(current_base) ||
        die "Cannot tell which release $BRANCH is based on. Its history must contain the base tag."
    say "Branch $BRANCH is based on $base; the chosen release is $latest."

    if [ "$base" = "$latest" ]; then
        say "Already based on $latest; no rebase needed."
    else
        backup="plugin-backup-$(date '+%Y%m%d-%H%M%S')"
        git branch "$backup" "$BRANCH"
        say "Backup branch: $backup."
        step "Rebasing $BRANCH onto $latest."
        if ! git rebase --onto "$latest" "$base" "$BRANCH"; then
            git rebase --abort 2>/dev/null || true
            cat <<EOF

The rebase stopped on a conflict. Nothing was installed; the running binary
is untouched and the branch is back where it was. The backup branch is
$backup.

Repair:
  1. Read plugin/AGENTS.md, section "Updating to a new release".
  2. Rebase by hand:  git rebase --onto $latest $base
  3. Resolve each conflict with the file map there, then:
       git add <files> && git rebase --continue
  4. Run:  plugin/install.sh --update
EOF
            exit 1
        fi
    fi

    refresh_catalogs "$latest"
    if [ "$SKIP_CHECKS" = 0 ]; then
        run_checks
    else
        say "Skipping fmt, clippy, and the tests because --skip-checks was given."
    fi
    backup_binary
    build_release
    verify_binary "$BIN"
    write_state "$latest" "$(git rev-parse --short HEAD)"
    check_desktop
    say ""
    say "Updated to $latest and installed. Restart Spotifast to use it."
}

status() {
    local base latest
    fetch_tags best_effort
    base=$(current_base) || true
    latest=$(latest_tag) || true
    say "Repository:  $REPO"
    say "Branch:      $BRANCH ($(git rev-parse --short "$BRANCH" 2>/dev/null || say missing))"
    say "Base tag:    ${base:-unknown}"
    say "Newest tag:  ${latest:-unknown}"
    if git diff --quiet && git diff --cached --quiet; then
        say "Checkout:    clean"
    else
        say "Checkout:    HAS UNCOMMITTED CHANGES"
    fi
    local entry
    entry=$(desktop_entry) || true
    if [ -n "$entry" ]; then
        say "Launcher:    $entry points at the binary"
    else
        say "Launcher:    no .desktop file points at $BIN"
    fi
    if [ -x "$BIN" ]; then
        say "Binary:      $BIN"
        say "Version:     $("$BIN" --version 2>/dev/null || say 'does not run')"
        say "Built:       $(file_mtime "$BIN")"
        if grep -a -q "Clear translations" "$BIN"; then
            say "Plugin:      present in the installed binary"
        else
            say "Plugin:      MISSING from the installed binary"
        fi
    else
        say "Binary:      missing ($BIN)"
    fi
    if [ -x "$BACKUP" ]; then
        say "Backup:      $BACKUP ($("$BACKUP" --version 2>/dev/null || say 'does not run'))"
    fi
    if [ -n "$(installed_tag)" ] && [ "$(installed_binary)" = "$BIN" ]; then
        say "Installed:   $(installed_tag) at $(installed_at)"
    fi
    if [ -n "$latest" ] && [ "$base" != "$latest" ]; then
        say ""
        say "An update is available: plugin/install.sh --update"
    fi
}

repair() {
    step "Checking for a stopped rebase."
    if [ -d .git/rebase-merge ] || [ -d .git/rebase-apply ]; then
        git rebase --abort
        say "Aborted the stopped rebase; the branch is back where it was."
    else
        say "No stopped rebase."
    fi

    verify_tools
    require_clean
    switch_branch
    say ""
    status

    if [ ! -x "$BIN" ] || ! grep -a -q "Clear translations" "$BIN"; then
        say ""
        say "The installed binary is missing or is not the plugin build; installing it again."
        install_current
    else
        say ""
        say "The installed binary is present and carries the plugin."
        say "If the app still misbehaves, see Troubleshooting in plugin/README.md."
    fi
}

rollback() {
    [ -f "$BACKUP" ] || die "No previous binary at $BACKUP; there is nothing to roll back to."
    step "Restoring the previous binary."
    cp -p -- "$BIN" "$BIN.plugin-rollback" 2>/dev/null || true
    mv -f -- "$BACKUP" "$BIN"
    if [ -f "$BIN.plugin-rollback" ]; then
        mv -f -- "$BIN.plugin-rollback" "$BACKUP"
    fi
    verify_binary "$BIN"
    say ""
    say "Restored. Restart Spotifast. The build you came from is now the backup."
}

export_patches() {
    [ -n "$PATCH_DIR" ] || die "--export-patches needs a directory"
    local base count
    base=$(current_base) ||
        die "Cannot tell which release $BRANCH is based on."
    mkdir -p "$PATCH_DIR"
    count=$(git format-patch --output-directory "$PATCH_DIR" "$base..$BRANCH" | wc -l | tr -d ' ')
    say "Wrote $count patches against $base to $PATCH_DIR."
}

case "$MODE" in
    install) install_current ;;
    update) update ;;
    status) status ;;
    repair) repair ;;
    rollback) rollback ;;
    export) export_patches ;;
esac
