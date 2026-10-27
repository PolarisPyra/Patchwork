#!/usr/bin/env bash
# Patchwork's changelog: user-facing notes, written in plain words. docs/changelog-style.md has the
# rules and docs/release.md how it fits into a release.
#
#   scripts/changelog.sh hook MSGFILE       The commit-msg hook: a commit that changes the app needs
#                                           a `Changelog:` paragraph for users, or `Changelog: none`.
#   scripts/changelog.sh lint               Checks the entry on stdin against the style rules.
#   scripts/changelog.sh check FILE VERSION Checks a drafted release section for VERSION.
#   scripts/changelog.sh draft TAG FILE     Drafts TAG's section into FILE from the commits since the
#                                           last tag, with Claude, unless FILE already exists.
#   scripts/changelog.sh insert FILE VERSION
#                                           Adds VERSION's section in FILE to the top of CHANGELOG.md.
#   scripts/changelog.sh section TAG        Prints TAG's section of CHANGELOG.md.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

STYLE=docs/changelog-style.md
# Changes to these paths are internal and never need an entry. Keep in step with is_app_path.
INTERNAL_PATHS=(':!.github' ':!scripts' ':!docs' ':!*.md')
# Left out of the diffs Claude reads: generated and binary files.
UNREAD_PATHS=(':!*.lock' ':!pnpm-lock.yaml' ':!*.png' ':!*.jpg' ':!*.ico' ':!*.icns' ':!*.svg' ':!assets')
# Diff lines of one commit Claude reads; the file list always comes in full.
DIFF_LINES=600
MODEL="${PATCHWORK_NOTES_MODEL:-claude-opus-5-5}"
# Drafts and checks, each draft getting the problems the last one had.
ATTEMPTS=3
SECTIONS=(Added Changed Fixed Removed Security)
# Product and platform names whose capitals the identifier checks would flag.
NAMES='GitHub|GitLab|AppImage|JavaScript|TypeScript|OpenCode|PowerShell|YouTube|macOS|iOS|iPadOS|LaTeX|DeepSeek|OpenAI|OpenRouter|ChatGPT|WebKit|WebView|Node\.js|Next\.js'

die() {
  echo "$*" >&2
  exit 1
}

is_app_path() {
  case "$1" in
    .github/* | scripts/* | docs/* | *.md) return 1 ;;
    *) return 0 ;;
  esac
}

# The text of a message's `Changelog:` paragraph, joined into one line, or nothing.
paragraph() {
  awk '
    /^Changelog:/ { on = 1; sub(/^Changelog:[ \t]*/, "") }
    on && (/^[ \t]*$/ || /^[A-Za-z-]+-[Bb]y:/) { exit }
    on { printf "%s%s", (n++ ? " " : ""), $0 }
  ' <<< "$1" | sed 's/[[:space:]]*$//'
}

is_none() {
  [[ "$1" =~ ^[Nn][Oo][Nn][Ee]\.?$ ]]
}

# Prints what breaks the style rules in one entry, or nothing.
problems() {
  local text="$1" bare
  bare=$(sed -E "s/($NAMES)//g" <<< "$text")
  [[ "$text" == *$'\n'* ]] && echo "it has a line break"
  [[ ${#text} -lt 25 ]] && echo "it is too short to say what changed"
  [[ "$text" == *'`'* ]] && echo "it has backticks"
  grep -qE '[A-Za-z0-9_-]+/[A-Za-z0-9_.-]+\.[a-z]{1,5}\b|\b[A-Za-z0-9_-]+\.(tsx?|jsx?|mjs|rs|toml|json|ya?ml|sh|css|html|md)\b' <<< "$bare" &&
    echo "it names a file"
  grep -qE '\b[a-z0-9]+_[a-z0-9_]+\b' <<< "$bare" && echo "it has a snake_case name"
  grep -qE '\b[a-z]+[A-Z][A-Za-z0-9]*\b|\b[A-Z][a-z0-9]+[A-Z][A-Za-z0-9]*\b' <<< "$bare" &&
    echo "it has a code-style name"
  grep -qE '\b[0-9]+ ?px\b' <<< "$text" && echo "it gives a pixel size"
  grep -qiE '\b(refactor(ed|ing|s)?|components?|backend|frontend|crates?|css|ipc|stdout|stderr|props)\b' <<< "$text" &&
    echo "it uses developer words"
  return 0
}

hook() {
  local file="$1" msg subject text found
  msg=$(tr -d '\r' < "$file" | grep -v '^#' || true)
  subject=$(head -n 1 <<< "$msg")
  [[ "$subject" =~ ^(Release\ v|Merge\ |fixup!\ |squash!\ |amend!\ ) ]] && return 0
  # An amend's commit also holds what the commit it replaces changed.
  local base=() path app=()
  [[ "${GIT_REFLOG_ACTION:-}" == *amend* ]] && git rev-parse --quiet --verify HEAD^ > /dev/null && base=(HEAD^)
  while IFS= read -r path; do
    [[ -n "$path" ]] && is_app_path "$path" && app+=("$path")
  done < <(git diff --cached --name-only ${base[@]+"${base[@]}"})
  [[ ${#app[@]} -eq 0 ]] && return 0
  found=$(grep -c '^Changelog:' <<< "$msg" || true)
  if [[ "$found" -eq 0 ]]; then
    cat >&2 << EOF
This commit changes the app (${app[0]}$([[ ${#app[@]} -gt 1 ]] && echo ", and $((${#app[@]} - 1)) more")),
so its message needs a paragraph for the changelog, which users read in Settings → Updates:

  Changelog: <what users see or can do now, where, and what is different, in plain words>

or, for a change users can't notice, "Changelog: none". The rules are in $STYLE.
EOF
    return 1
  fi
  text=$(paragraph "$msg")
  is_none "$text" && return 0
  local issues
  issues=$(problems "$text")
  if [[ -n "$issues" ]]; then
    {
      echo "The Changelog: paragraph breaks the rules in $STYLE:"
      sed 's/^/  - /' <<< "$issues"
      echo "  $text"
    } >&2
    return 1
  fi
}

lint() {
  local issues
  issues=$(problems "$(cat)")
  [[ -z "$issues" ]] || die "$issues"
}

# Checks a release section: one heading for the version, group names, and each entry.
check() {
  local file="$1" version="${2#v}" line bad=0 issues heads
  [[ -s "$file" ]] || die "$file is empty."
  heads=$(grep -c '^## ' "$file" || true)
  if [[ "$heads" -ne 1 ]] || ! grep -q "^## \[$version\] - [0-9]\{4\}-[0-9]\{2\}-[0-9]\{2\}\$" "$file"; then
    echo "It needs exactly one heading, \"## [$version] - YYYY-MM-DD\"." >&2
    bad=1
  fi
  while IFS= read -r line; do
    case "$line" in
      "" | "## ["*) ;;
      "### "*)
        [[ " ${SECTIONS[*]} " == *" ${line#\#\#\# } "* ]] || {
          echo "Unknown group: $line" >&2
          bad=1
        }
        ;;
      "- "*)
        issues=$(problems "${line#- }")
        if [[ -n "$issues" ]]; then
          printf '%s\n' "$line" "$(sed 's/^/    /' <<< "$issues")" >&2
          bad=1
        fi
        ;;
      *)
        echo "Not a heading or an entry: $line" >&2
        bad=1
        ;;
    esac
  done < "$file"
  [[ $bad -eq 0 ]] || die "Fix $file, then run the release again."
}

# The commits since the last tag that change the app, oldest first.
app_commits() {
  local since="$1"
  git log --reverse --format='%H %s' "$since..HEAD" -- . "${INTERNAL_PATHS[@]}" |
    grep -vE '^[0-9a-f]+ Release v' | cut -d' ' -f1 || true
}

# What Claude reads about each commit.
describe() {
  local hash text
  for hash in "$@"; do
    text=$(paragraph "$(git show -s --format=%B "$hash")")
    echo "=== commit ${hash:0:12}"
    echo "Subject: $(git show -s --format=%s "$hash")"
    if is_none "$text"; then
      echo "Changelog: none (its author judged it internal)"
    elif [[ -n "$text" ]]; then
      echo "Changelog: $text"
    else
      echo "Changelog: (none written)"
    fi
    echo "Files:"
    git show --format= --stat=200 "$hash" -- . "${INTERNAL_PATHS[@]}"
    echo "Diff:"
    git show --format= "$hash" -- . "${INTERNAL_PATHS[@]}" "${UNREAD_PATHS[@]}" | head -n "$DIFF_LINES"
    echo
  done
}

# Runs Claude with no tools on the prompt on stdin, and prints its structured output.
ask() {
  local system="$1" schema="$2" out
  out=$(claude -p --model "$MODEL" --output-format json --json-schema "$schema" --tools "" \
    --strict-mcp-config --disable-slash-commands --no-session-persistence \
    --system-prompt "$system") || die "claude failed: $out"
  jq -e '.is_error == false and .structured_output != null' <<< "$out" > /dev/null ||
    die "claude gave no notes: $(jq -r '.result // .' <<< "$out" | head -c 500)"
  jq -c .structured_output <<< "$out"
}

DRAFT_SCHEMA='{"type":"object","additionalProperties":false,"required":["entries","internal"],"properties":{
  "entries":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["group","text","commits"],"properties":{
    "group":{"enum":["Added","Changed","Fixed","Removed","Security"]},
    "text":{"type":"string"},
    "commits":{"type":"array","minItems":1,"items":{"type":"string"}}}}},
  "internal":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["commit","reason"],"properties":{
    "commit":{"type":"string"},"reason":{"type":"string"}}}}}}'

REVIEW_SCHEMA='{"type":"object","additionalProperties":false,"required":["problems"],"properties":{
  "problems":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["about","problem"],"properties":{
    "about":{"type":"string"},"problem":{"type":"string"}}}}}}'

# What is wrong with a draft that a script can tell: commits it leaves out or makes up, and entries
# that break the style rules.
mechanical() {
  local draft="$1"
  shift
  local want got issues issue text
  want=$(printf '%s\n' "$@" | cut -c1-12 | sort -u)
  got=$(jq -r '(.entries[].commits[], .internal[].commit) | .[0:12]' <<< "$draft" | sort -u | sed '/^$/d')
  comm -23 <(echo "$want") <(echo "$got") | sed 's/^/commit /; s/$/: in neither an entry nor internal/'
  comm -13 <(echo "$want") <(echo "$got") | sed 's/^/commit /; s/$/: not one of the commits in this release/'
  while IFS= read -r text; do
    issues=$(problems "$text")
    [[ -z "$issues" ]] && continue
    while IFS= read -r issue; do
      printf 'entry "%s": %s\n' "$text" "$issue"
    done <<< "$issues"
  done < <(jq -r '.entries[].text' <<< "$draft")
  return 0
}

render() {
  local draft="$1" version="$2" group
  echo "## [$version] - $(date +%F)"
  if [[ $(jq '.entries | length' <<< "$draft") -eq 0 ]]; then
    printf '\n### Changed\n\n- Behind-the-scenes improvements, with nothing new to see.\n'
    return
  fi
  for group in "${SECTIONS[@]}"; do
    jq -e --arg g "$group" 'any(.entries[]; .group == $g)' <<< "$draft" > /dev/null || continue
    printf '\n### %s\n\n' "$group"
    jq -r --arg g "$group" '.entries[] | select(.group == $g) | "- \(.text)"' <<< "$draft"
  done
}

draft() {
  local tag="$1" file="$2" version="${1#v}"
  if [[ -s "$file" ]]; then
    echo "Using the notes already drafted in $file."
    check "$file" "$version"
    return
  fi
  command -v claude > /dev/null || die "claude is required to draft the release notes."
  local since
  since=$(git describe --tags --abbrev=0 --match 'v[0-9]*' HEAD) || die "No earlier release tag."
  local commits=()
  local hash
  while IFS= read -r hash; do commits+=("$hash"); done < <(app_commits "$since")
  mkdir -p "$(dirname "$file")"
  if [[ ${#commits[@]} -eq 0 ]]; then
    render '{"entries":[],"internal":[]}' "$version" > "$file"
    return
  fi

  local guide context system draft="" problems_found="" attempt review
  guide=$(cat "$STYLE")
  context=$(describe "${commits[@]}")
  system="You write the release notes for Patchwork, a desktop app for working with AI coding agents. \
Its users read them in the app and on GitHub. Follow this style guide exactly.

$guide"
  for attempt in $(seq "$ATTEMPTS"); do
    echo "Drafting the notes for $tag from ${#commits[@]} commits (attempt $attempt of $ATTEMPTS)…" >&2
    draft=$(ask "$system" "$DRAFT_SCHEMA" << EOF
Write the notes for Patchwork $version from the commits below, which are every commit since $since that changes the app.

- Each commit has its author's "Changelog:" text for users, when they wrote one. Use it to see what the author meant, but check it against the diff and rewrite it to the style guide.
- Merge commits that make up one change into one entry, describing the end result.
- Put every commit in exactly one place: in the "commits" of the entries it contributes to (by its 12-character hash), or in "internal" with a short reason when users can't notice it. A commit whose author wrote "Changelog: none" is internal unless its diff plainly changes something users see.
- Claim only what the diffs show.
$([[ -n "$problems_found" ]] && printf '\nYour last draft had these problems. Fix all of them:\n%s\n\nYour last draft:\n%s\n' "$problems_found" "$draft")

$context
EOF
    )
    problems_found=$(mechanical "$draft" "${commits[@]}")
    if [[ -z "$problems_found" ]]; then
      echo "Checking the draft against the diffs…" >&2
      review=$(ask "You check release notes before they reach Patchwork's paying users. Be strict but report only real problems.

$guide" "$REVIEW_SCHEMA" << EOF
Here are draft notes for Patchwork $version and the commits they come from. Report each real problem:

- An entry claims something the diffs don't show, or gets a detail wrong.
- An entry is vague: a user couldn't tell what changed or where.
- An entry breaks the style guide, or is in the wrong group.
- A commit listed as internal changes something users can see.
- A user-visible change in the commits has no entry.

Return an empty list when there are none.

Draft:
$(jq . <<< "$draft")

$context
EOF
      )
      problems_found=$(jq -r '.problems[] | "\(.about): \(.problem)"' <<< "$review")
    fi
    if [[ -z "$problems_found" ]]; then
      render "$draft" "$version" > "$file"
      return
    fi
    echo "The draft had problems:" >&2
    sed 's/^/  - /' <<< "$problems_found" >&2
  done
  jq . <<< "$draft" > "$file.json"
  die "No draft passed the checks in $ATTEMPTS attempts. The last one is in $file.json. Write the \
section into $file yourself (see $STYLE) and run the release again."
}

insert() {
  local file="$1"
  check "$file" "$2"
  local header body
  header=$(awk '/^## \[/ { exit } { print }' CHANGELOG.md)
  body=$(awk 'found || /^## \[/ { found = 1; print }' CHANGELOG.md)
  printf '%s\n\n%s\n\n%s\n' "$header" "$(cat "$file")" "$body" > CHANGELOG.md
}

section() {
  local version="${1#v}"
  awk -v head="## [$version]" '
    index($0, head) == 1 { on = 1; print; next }
    on && /^## \[/ { exit }
    on { print }
  ' CHANGELOG.md | sed -e :a -e '/^\n*$/{$d;N;ba' -e '}'
}

case "${1:-}" in
  hook) hook "${2:?}" ;;
  lint) lint ;;
  check) check "${2:?}" "${3:?}" ;;
  draft) draft "${2:?}" "${3:?}" ;;
  insert) insert "${2:?}" "${3:?}" ;;
  section) section "${2:?}" ;;
  *) sed -n '2,/^set -euo/p' "${BASH_SOURCE[0]}" | sed '$d' | sed 's/^# \{0,1\}//' >&2 && exit 1 ;;
esac
