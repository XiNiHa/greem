#!/usr/bin/env bash
# Keeps the graphql-js port under tests/graphql_js in step with upstream.
#
#   graphql-js-sync.sh pin            the pinned upstream tag
#   graphql-js-sync.sh suites         each suite and the upstream file it mirrors
#   graphql-js-sync.sh poll           the newest stable tag above the pin, or nothing
#   graphql-js-sync.sh audit [TAG]    the ordered case paths of each suite against
#                                     upstream at TAG (default: the pin); fails on a difference
#   graphql-js-sync.sh diff TAG       what changed upstream between the pin and TAG
#
# Needs curl, jq, awk and cargo. GITHUB_TOKEN, when set, authenticates the API calls.
set -euo pipefail

crate=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
tests=$crate/tests/graphql_js
upstream=graphql/graphql-js
dirs=(src/execution/__tests__ src/execution/incremental/__tests__)

api() {
  local auth=()
  [[ -n ${GITHUB_TOKEN:-} ]] && auth=(-H "Authorization: Bearer $GITHUB_TOKEN")
  curl -sSL ${auth[@]+"${auth[@]}"} -H "Accept: application/vnd.github+json" "https://api.github.com/$1"
}

raw() {
  local auth=()
  [[ -n ${GITHUB_TOKEN:-} ]] && auth=(-H "Authorization: Bearer $GITHUB_TOKEN")
  curl -sSL ${auth[@]+"${auth[@]}"} -o "$3" -w '%{http_code}' "https://raw.githubusercontent.com/$upstream/$1/$2"
}

pin() {
  cargo metadata --manifest-path "$crate/Cargo.toml" --format-version 1 --no-deps |
    jq -r '.packages[] | select(.name == "greem-compliance") | .metadata.graphql_js.tag'
}

suites() {
  local file suite path
  for file in "$tests"/*.rs; do
    suite=$(basename "$file" .rs)
    [[ $suite == main ]] && continue
    # shellcheck disable=SC2016
    path=$(head -1 "$file" | sed -n 's/^\/\/! graphql-js `\([^`]*\)`.*/\1/p')
    [[ -n $path ]] || { echo "$file: first line must read //! graphql-js \`<upstream path>\`" >&2; return 1; }
    grep -qE "^mod (r#)?$suite;$" "$tests/main.rs" || { echo "$tests/main.rs: no mod $suite" >&2; return 1; }
    printf '%s\t%s\n' "$suite" "$path"
  done
  local mods
  mods=$(grep -E '^mod (r#)?[a-z_]+;$' "$tests/main.rs" | grep -vc '^mod common;')
  [[ $mods -eq $(suites_count) ]] || { echo "$tests/main.rs declares $mods suites, $(suites_count) have headers" >&2; return 1; }
}

suites_count() {
  find "$tests" -name '*.rs' ! -name main.rs | wc -l | tr -d ' '
}

# Stable tags only, highest semver last.
stable_tags() {
  git ls-remote --tags "https://github.com/$upstream" |
    sed -n 's|.*refs/tags/\(v[0-9]*\.[0-9]*\.[0-9]*\)$|\1|p' |
    while read -r tag; do printf '%s %s\n' "$(key "$tag")" "$tag"; done | sort | cut -d' ' -f2
}

key() {
  printf '%s' "$1" | awk -F'[v.]' '{ printf "%09d%09d%09d", $2, $3, $4 }'
}

poll() {
  local current newest
  current=$(pin)
  newest=$(stable_tags | tail -1)
  [[ $(key "$newest") > $(key "$current") ]] && echo "$newest"
  return 0
}

# Flattens describe/it lines into one "describe > describe > it" path per case,
# nesting by indentation. $1 is the prefix a marker line starts with.
flatten() {
  awk -v prefix="$1" '
    function title(line,    s) {
      s = line
      sub("^[ ]*" prefix "(it|describe)\\(\x27", "", s)
      sub("\x27.*$", "", s)
      return s
    }
    function indent(line) { return match(line, /[^ ]/) - 1 }
    function pop(ind) { while (depth > 0 && at[depth] >= ind) depth-- }
    function path(    i, p) {
      p = ""
      for (i = 1; i <= depth; i++) p = p name[i] " > "
      return p
    }
    $0 ~ "^[ ]*" prefix "describe\\(\x27" { pop(indent($0)); depth++; at[depth] = indent($0); name[depth] = title($0); next }
    $0 ~ "^[ ]*" prefix "it\\(\x27" { pop(indent($0)); print path() title($0) }
  '
}

audit() {
  local tag=${1:-$(pin)} tmp status failed=0 suite path
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' RETURN
  while IFS=$'\t' read -r suite path; do
    status=$(raw "$tag" "$path" "$tmp/$suite.ts")
    [[ $status == 200 ]] || { echo "$suite: upstream $path at $tag: HTTP $status" >&2; failed=1; continue; }
    flatten '' <"$tmp/$suite.ts" >"$tmp/$suite.upstream"
    flatten '//[/]? ' <"$tests/$suite.rs" >"$tmp/$suite.port"
    if ! diff -u --label "upstream $path@$tag" --label "tests/graphql_js/$suite.rs" "$tmp/$suite.upstream" "$tmp/$suite.port"; then
      failed=1
    fi
  done < <(suites)
  [[ $failed -eq 0 ]] && echo "audit: every suite matches upstream at $tag"
  return $failed
}

listing() {
  local dir
  for dir in "${dirs[@]}"; do
    api "repos/$upstream/contents/$dir?ref=$1" |
      jq -r 'if type == "array" then .[] | select(.type == "file") | .path else empty end'
  done | sort
}

diff_cmd() {
  local tag=$1 current tmp suite path status
  current=$(pin)
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' RETURN
  echo "# graphql-js $current -> $tag"
  echo
  listing "$current" >"$tmp/before"
  listing "$tag" >"$tmp/after"
  echo "## Files under ${dirs[*]}"
  echo
  comm -13 "$tmp/before" "$tmp/after" | sed 's/^/added: /'
  comm -23 "$tmp/before" "$tmp/after" | sed 's/^/removed: /'
  echo
  while IFS=$'\t' read -r suite path; do
    status=$(raw "$current" "$path" "$tmp/$suite.before")
    [[ $status == 200 ]] || { echo "## $suite: $path missing at $current (HTTP $status)"; continue; }
    status=$(raw "$tag" "$path" "$tmp/$suite.after")
    [[ $status == 200 ]] || { echo "## $suite: $path missing at $tag (HTTP $status)"; echo; continue; }
    if cmp -s "$tmp/$suite.before" "$tmp/$suite.after"; then
      echo "## $suite: unchanged"
    else
      echo "## $suite: $path"
      echo
      echo '```diff'
      diff -u --label "$path@$current" --label "$path@$tag" "$tmp/$suite.before" "$tmp/$suite.after" || true
      echo '```'
    fi
    echo
  done < <(suites)
}

case ${1:-} in
  pin) pin ;;
  suites) suites ;;
  poll) poll ;;
  audit) audit "${2:-}" ;;
  diff) [[ -n ${2:-} ]] || { echo "usage: $0 diff TAG" >&2; exit 2; }; diff_cmd "$2" ;;
  *) sed -n '2,11p' "$0" >&2; exit 2 ;;
esac
