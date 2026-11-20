#!/usr/bin/env bash
# Build mao's macOS disk image (.dmg), signed with the Developer ID certificate and, by default, notarised and stapled,
# and publish it as a GitHub release.
#
#   ./build.sh                       build, sign and notarise dmg/mao_<version>_<arch>.dmg in dist/
#   ./build.sh --nonotarize          build and sign only (no Apple notarisation)
#   ./build.sh --unsigned            build without signing or notarising (for local tests only)
#   ./build.sh --plain-dmg           make the disk image without the Finder layout step (for SSH or CI sessions)
#   ./build.sh --check               report the signing identity, notary credentials and tools, and build nothing
#   ./build.sh --release [--draft] [--yes] [--dry-run]
#                                    tag v<version>, push the tag and create the GitHub release with the dist/ files for
#                                    that version. The working tree must be committed and pushed. It never commits.
#   ./build.sh --delete-release [<version>] [--keep-tag] [--yes] [--dry-run]
#                                    delete the GitHub release v<version> and its tag (--keep-tag keeps the tag)
#   ./build.sh --list-release       list the GitHub releases of this repository
#
# Signing uses the "Developer ID Application" identity in the keychain, or APPLE_SIGNING_IDENTITY when there is more than one.
# Notarisation needs either an App Store Connect API key (APPLE_API_KEY, APPLE_API_ISSUER, APPLE_API_KEY_PATH) or an Apple ID
# (APPLE_ID, APPLE_PASSWORD, APPLE_TEAM_ID). They come from the environment, or from the first file found of
# $MAO_NOTARY_ENV, ~/.config/mao/notary.env and ~/.config/cheetoh/notary.env (KEY=VALUE lines, sourced like a shell file).
#
# The version is the "version" in package.json, which is kept equal to the Cargo, Tauri and README versions by the tests.
# Release files are copied to ./dist/ (gitignored), named mao_<version>_<arch>.dmg.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")" && pwd)
DIST="$ROOT/dist"
BUNDLE="$ROOT/src-tauri/target/release/bundle"
VERSION=$(sed -n 's/^  "version": "\([^"]*\)".*/\1/p' "$ROOT/package.json" | head -1)
[ -n "$VERSION" ] || { echo "build: no version in package.json" >&2; exit 1; }
TAG="v$VERSION"

notarize=1 sign=1 plain=0 check=0 release=0 delete=0 del_version= list=0 draft=0 yes=0 dry=0 keep_tag=0
while [ $# -gt 0 ]; do
    case "$1" in
        --notarize) notarize=1; shift ;;
        --nonotarize) notarize=0; shift ;;
        --unsigned) sign=0; notarize=0; shift ;;
        --plain-dmg) plain=1; shift ;;
        --check) check=1; shift ;;
        --release) release=1; shift ;;
        --delete-release)
            delete=1; shift
            case "${1:-}" in -*|"") ;; *) del_version="${1#v}"; shift ;; esac ;;
        --list-release) list=1; shift ;;
        --draft) draft=1; shift ;;
        --keep-tag) keep_tag=1; shift ;;
        --yes) yes=1; shift ;;
        --dry-run) dry=1; shift ;;
        -h|--help) sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "build: unknown option $1 (try --help)" >&2; exit 1 ;;
    esac
done

say() { printf 'build: %s\n' "$*"; }
die() { printf 'build: %s\n' "$*" >&2; exit 1; }
# Run a command, or only print it with --dry-run.
run() { if [ "$dry" = 1 ]; then echo "  [dry-run] $*"; else "$@"; fi; }

need() { command -v "$1" >/dev/null 2>&1 || die "'$1' is required. $2"; }

# The Developer ID Application identity to sign with. Sets identity, or leaves it empty when there is none.
find_identity() {
    identity="${APPLE_SIGNING_IDENTITY:-}"
    [ -n "$identity" ] && return
    local found
    found=$(security find-identity -v -p codesigning 2>/dev/null | sed -n 's/.*"\(Developer ID Application[^"]*\)".*/\1/p')
    case "$(printf '%s\n' "$found" | grep -c . || true)" in
        0) ;;
        1) identity="$found" ;;
        *) die "more than one Developer ID Application certificate: set APPLE_SIGNING_IDENTITY to the one to use" ;;
    esac
}

# Loads the notary credentials into the environment, from the first settings file that exists.
load_notary() {
    if [ -n "${APPLE_API_KEY:-}" ] || [ -n "${APPLE_ID:-}" ]; then return; fi
    local file
    for file in "${MAO_NOTARY_ENV:-}" "$HOME/.config/mao/notary.env" "$HOME/.config/cheetoh/notary.env"; do
        [ -n "$file" ] && [ -f "$file" ] || continue
        set -a
        # shellcheck disable=SC1090
        . "$file"
        set +a
        NOTARY_SOURCE="$file"
        return
    done
}

notary_ready() {
    { [ -n "${APPLE_API_KEY:-}" ] && [ -n "${APPLE_API_ISSUER:-}" ] && [ -f "${APPLE_API_KEY_PATH:-/nonexistent}" ]; } ||
        { [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_PASSWORD:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ]; }
}

# The notarytool arguments for whichever credentials are set.
notary_auth() {
    if [ -n "${APPLE_API_KEY:-}" ]; then
        printf '%s\n' --key-id "$APPLE_API_KEY" --key "$APPLE_API_KEY_PATH" --issuer "$APPLE_API_ISSUER"
    else
        printf '%s\n' --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID"
    fi
}

check_report() {
    say "tools"
    for tool in npx cargo xcrun codesign spctl hdiutil security; do
        if command -v "$tool" >/dev/null 2>&1; then echo "  ok       $tool"; else echo "  missing  $tool"; fi
    done
    say "signing"
    find_identity
    if [ -n "$identity" ]; then echo "  identity $identity"; else echo "  no Developer ID Application certificate found"; fi
    say "notarisation"
    load_notary
    if notary_ready; then
        echo "  credentials ready (${NOTARY_SOURCE:-environment})"
    else
        echo "  credentials missing: set APPLE_API_KEY, APPLE_API_ISSUER and APPLE_API_KEY_PATH, or APPLE_ID, APPLE_PASSWORD and APPLE_TEAM_ID,"
        echo "           in the environment or in $HOME/.config/mao/notary.env"
    fi
    say "GitHub"
    if gh auth status >/dev/null 2>&1; then echo "  gh logged in"; else echo "  gh not logged in (gh auth login)"; fi
}

build_dmg() {
    need npx "Install Node.js."
    need cargo "Install Rust from https://rustup.rs."
    need xcrun "Install the Xcode command line tools: xcode-select --install."

    say "checking the version is the same everywhere ($VERSION)"
    cargo test --quiet --manifest-path "$ROOT/src-tauri/Cargo.toml" version_agrees_in_every_place >/dev/null

    if [ "$sign" = 1 ]; then
        find_identity
        if [ -z "$identity" ]; then
            [ "$notarize" = 0 ] || die "no Developer ID Application certificate in the keychain. Use --nonotarize or --unsigned, or install the certificate"
            say "no Developer ID Application certificate: building unsigned"
            sign=0
        fi
    fi

    if [ "$notarize" = 1 ]; then
        load_notary
        notary_ready || die "notarisation credentials are missing: set them in the environment or in $HOME/.config/mao/notary.env (see --check), or build with --nonotarize"
    fi

    say "building the audio tap"
    sh "$ROOT/scripts/build-tap.sh"
    say "freezing the streamer"
    sh "$ROOT/scripts/build-streamer.sh"

    # Tauri signs the app with APPLE_SIGNING_IDENTITY and notarises it when the notary variables are set. Give it only what
    # this build wants, so that --nonotarize does not notarise the app. env takes the -u options before the assignments.
    local unset=() set=()
    if [ "$sign" = 1 ]; then
        set+=("APPLE_SIGNING_IDENTITY=$identity")
    else
        unset+=(-u APPLE_SIGNING_IDENTITY)
    fi
    local name
    for name in APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_PATH APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID; do
        if [ "$notarize" = 1 ] && [ -n "${!name:-}" ]; then
            set+=("$name=${!name}")
        else
            unset+=(-u "$name")
        fi
    done
    # CI=true makes Tauri skip the Finder layout step, which needs Automation permission.
    [ "$plain" = 0 ] || set+=("CI=true")

    say "building the app and disk image"
    local stamp
    stamp=$(mktemp)
    # Tauri warns that it skips notarising the app when no notary variables are set. That is the
    # intent of --nonotarize, so the line is dropped. The build's own exit status still counts.
    (cd "$ROOT" && env ${unset[@]+"${unset[@]}"} ${set[@]+"${set[@]}"} npx tauri build --bundles app,dmg 2>&1 | sed '/skipping app notarization/d')

    local dmg
    dmg=$(find "$BUNDLE/dmg" -maxdepth 1 -name '*.dmg' -newer "$stamp" 2>/dev/null | head -1)
    rm -f "$stamp"
    [ -n "$dmg" ] || die "no disk image was made (see the Tauri output above)"
    local app="$BUNDLE/macos/mao.app"

    if [ "$sign" = 1 ]; then
        say "signing the disk image"
        codesign --force --timestamp --sign "$identity" "$dmg"
        codesign --verify --strict --verbose=2 "$dmg"
    fi

    if [ "$notarize" = 1 ]; then
        say "notarising the disk image (this takes a few minutes)"
        local auth=()
        while IFS= read -r line; do auth+=("$line"); done < <(notary_auth)
        if ! xcrun notarytool submit "$dmg" "${auth[@]}" --wait; then
            die "notarisation failed: see xcrun notarytool history"
        fi
        xcrun stapler staple "$dmg"
        xcrun stapler validate "$dmg"
        spctl -a -vvv -t install "$dmg"
    fi

    if [ "$sign" = 1 ]; then
        codesign --verify --deep --strict "$app"
    fi

    local arch
    case "$(uname -m)" in arm64|aarch64) arch=aarch64 ;; *) arch=x64 ;; esac
    mkdir -p "$DIST"
    local out="$DIST/mao_${VERSION}_${arch}.dmg"
    cp "$dmg" "$out"
    say "done: $out"
    if [ "$notarize" = 1 ]; then echo "  signed, notarised and stapled"; elif [ "$sign" = 1 ]; then echo "  signed, not notarised"; else echo "  unsigned"; fi
}

# The release notes: the CHANGELOG.md section for this version.
release_notes() {
    local notes="$1"
    awk -v ver="$VERSION" 'index($0, "## [" ver "]") == 1 { p = 1; next } /^## \[/ { p = 0 } p' "$ROOT/CHANGELOG.md" > "$notes"
}

release_it() {
    need gh "Install the GitHub CLI from https://cli.github.com, then gh auth login."
    gh auth status >/dev/null 2>&1 || die "gh is not logged in (gh auth login)"
    cd "$ROOT"
    git rev-parse --git-dir >/dev/null 2>&1 || die "not a git repository"

    shopt -s nullglob
    local assets=("$DIST"/mao_"$VERSION"_*.dmg)
    shopt -u nullglob
    [ "${#assets[@]}" -gt 0 ] || die "no disk image for $VERSION in dist/: build it first with ./build.sh"

    git fetch -q origin 2>/dev/null || true
    [ -z "$(git status --porcelain)" ] || die "uncommitted changes: commit them first (this script never commits)"
    local upstream commit
    upstream=$(git rev-parse --abbrev-ref '@{u}' 2>/dev/null) || die "this branch has no upstream: push it first"
    commit=$(git rev-parse HEAD)
    [ "$commit" = "$(git rev-parse '@{u}')" ] || die "HEAD is not the same as $upstream: push (or pull) first"
    if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null && [ "$(git rev-list -n1 "$TAG")" != "$commit" ]; then
        die "tag $TAG already exists on another commit"
    fi

    local notes exists=0
    notes=$(mktemp)
    release_notes "$notes"
    local notes_args=(--notes-file "$notes")
    if [ ! -s "$notes" ]; then
        say "CHANGELOG.md has no section for $VERSION: GitHub will generate the notes"
        notes_args=(--generate-notes)
    fi
    gh release view "$TAG" >/dev/null 2>&1 && exists=1

    say "release $TAG"
    echo "  commit: $(git log -1 --format='%h %s' "$commit")"
    echo "  files:"
    printf '    %s\n' "${assets[@]##*/}"
    if [ "$exists" = 1 ]; then echo "  the release exists: the files are added to it (replacing any with the same name)"
    else echo "  a new release will be created$([ "$draft" = 1 ] && echo ' as a draft')"; fi

    if [ "$yes" != 1 ] && [ "$dry" != 1 ]; then
        read -r -p "Tag, push and publish? [y/N] " answer
        case "$answer" in y|Y|yes) ;; *) echo "stopped"; rm -f "$notes"; exit 1 ;; esac
    fi

    git rev-parse -q --verify "refs/tags/$TAG" >/dev/null || run git tag -a "$TAG" -m "mao $VERSION" "$commit"
    run git push origin "$TAG"
    if [ "$exists" = 1 ]; then
        run gh release upload "$TAG" "${assets[@]}" --clobber
    else
        local draft_args=()
        [ "$draft" = 1 ] && draft_args=(--draft)
        run gh release create "$TAG" "${assets[@]}" --title "mao $VERSION" "${notes_args[@]}" ${draft_args[@]+"${draft_args[@]}"}
    fi
    rm -f "$notes"
    if [ "$dry" = 1 ]; then echo "dry run: nothing was tagged, pushed or published"; return; fi
    say "release done: $(gh release view "$TAG" --json url -q .url 2>/dev/null || echo "$TAG")"
}

delete_release() {
    need gh "Install the GitHub CLI from https://cli.github.com, then gh auth login."
    gh auth status >/dev/null 2>&1 || die "gh is not logged in (gh auth login)"
    cd "$ROOT"
    # the version to delete: the one given, else the current one
    TAG="v${del_version:-$VERSION}"
    local exists=0 remote_tag=0 local_tag=0
    gh release view "$TAG" >/dev/null 2>&1 && exists=1
    git ls-remote --exit-code --tags origin "refs/tags/$TAG" >/dev/null 2>&1 && remote_tag=1
    git rev-parse -q --verify "refs/tags/$TAG" >/dev/null && local_tag=1
    [ "$exists" = 1 ] || [ "$remote_tag" = 1 ] || [ "$local_tag" = 1 ] || die "no release or tag $TAG found"

    say "delete release $TAG"
    [ "$exists" = 1 ] && echo "  GitHub release: $(gh release view "$TAG" --json url -q .url), with its files" || echo "  no GitHub release"
    if [ "$keep_tag" = 1 ]; then echo "  the tag is kept"; else echo "  tags: remote $([ "$remote_tag" = 1 ] && echo yes || echo no), local $([ "$local_tag" = 1 ] && echo yes || echo no)"; fi
    if [ "$yes" != 1 ] && [ "$dry" != 1 ]; then
        read -r -p "Delete? This cannot be undone. [y/N] " answer
        case "$answer" in y|Y|yes) ;; *) echo "stopped"; exit 1 ;; esac
    fi
    if [ "$exists" = 1 ]; then
        if [ "$keep_tag" = 1 ]; then run gh release delete "$TAG" --yes; else run gh release delete "$TAG" --yes --cleanup-tag; fi
    fi
    if [ "$keep_tag" != 1 ]; then
        [ "$exists" = 1 ] || [ "$remote_tag" = 0 ] || run git push origin ":refs/tags/$TAG"
        [ "$local_tag" = 0 ] || run git tag -d "$TAG"
    fi
    say "deleted $TAG"
}

# The GitHub releases of this repository, newest first.
list_releases() {
    need gh "Install the GitHub CLI from https://cli.github.com, then gh auth login."
    gh auth status >/dev/null 2>&1 || die "gh is not logged in (gh auth login)"
    cd "$ROOT"
    gh release list --limit 100
}

if [ "$check" = 1 ]; then check_report; exit 0; fi
if [ "$delete" = 1 ]; then delete_release; exit 0; fi
if [ "$list" = 1 ]; then list_releases; exit 0; fi
if [ "$release" = 1 ]; then release_it; exit 0; fi
build_dmg
