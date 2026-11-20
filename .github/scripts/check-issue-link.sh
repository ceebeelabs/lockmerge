#!/usr/bin/env bash
# Fails unless the pull request body links an open issue of this repository.
#
# Environment:
#   PR_BODY     pull request body (never interpolated into a shell line)
#   PR_AUTHOR   login of the pull request author
#   REPO        owner/name, for example ceebeelabs/lockmerge
#   GH_TOKEN    token for `gh api` (read access to issues)
#
# Usage:
#   check-issue-link.sh              check PR_BODY
#   check-issue-link.sh --self-test  offline test with a stubbed lookup
set -u

# Prints "<kind> <state>" for issue number $1, where kind is issue or pull and
# state is open or closed; prints "missing" if there is no such number.
lookup_issue() {
  local n=$1 out
  if ! out=$(gh api "repos/${REPO}/issues/${n}" \
    --jq '(if .pull_request then "pull" else "issue" end) + " " + .state' 2>/dev/null); then
    echo missing
    return
  fi
  echo "$out"
}

# Prints the issue numbers referenced with a closing keyword or "Part of".
# A reference to another repository (owner/name#N) never matches because the
# keyword must be followed directly by "#".
extract_refs() {
  local rest=$1 m c
  local pat='(^|[^[:alnum:]_])(fix(e[sd])?|close[sd]?|resolve[sd]?|part of)[[:space:]]+#([0-9]+)'
  local restore
  restore=$(shopt -p nocasematch)
  shopt -s nocasematch
  # Match one reference at a time and continue after it, so the text between
  # two references never decides whether the second one is found.
  while [[ $rest =~ $pat ]]; do
    m=${BASH_REMATCH[0]}
    rest=${rest#*"$m"}
    c=${rest:0:1}
    case $c in
      [[:alnum:]_]) ;; # "#1abc" is not a reference
      *)
        # Keep the number a decimal string: arithmetic would wrap on overflow.
        m=${BASH_REMATCH[4]}
        while [ "${#m}" -gt 1 ] && [ "${m:0:1}" = 0 ]; do m=${m:1}; done
        echo "$m"
        ;;
    esac
  done | sort -u
  eval "$restore"
}

check() {
  local body=$1 author=$2 refs n info kind state bad=0
  if [ "$author" = "dependabot[bot]" ]; then
    echo "Dependabot pull request: no issue link needed."
    return 0
  fi
  refs=$(extract_refs "$body")
  if [ -z "$refs" ]; then
    echo "No issue link found. Add one of these to the pull request description:"
    echo "  Fixes #N, Closes #N, Resolves #N (when this completes the issue)"
    echo "  Part of #N (when it is one piece of the issue)"
    echo "N must be an open issue of ${REPO}. Editing the description re-runs this check."
    return 1
  fi
  for n in $refs; do
    info=$(lookup_issue "$n")
    if [ "$info" = missing ]; then
      echo "#${n}: no such issue or pull request in ${REPO}."
      bad=1
      continue
    fi
    kind=${info%% *}
    state=${info##* }
    if [ "$kind" != issue ]; then
      echo "#${n}: is a pull request, not an issue."
      bad=1
    elif [ "$state" != open ]; then
      echo "#${n}: issue is ${state}, not open."
      bad=1
    else
      echo "#${n}: open issue in ${REPO}."
    fi
  done
  if [ "$bad" -ne 0 ]; then
    echo "Fix the reference in the description; editing it re-runs this check."
    return 1
  fi
  return 0
}

self_test() {
  REPO=ceebeelabs/lockmerge
  lookup_issue() {
    case $1 in
      1) echo "issue open" ;;
      2) echo "issue open" ;;
      3) echo "issue closed" ;;
      4) echo "pull open" ;;
      *) echo missing ;;
    esac
  }
  local fails=0
  t() { # name expected(0|1) author body
    local out rc
    out=$(check "$4" "$3" 2>&1)
    rc=$?
    if [ "$rc" -ne "$2" ]; then
      echo "FAIL: $1 (exit $rc, wanted $2)"
      echo "$out" | sed 's/^/    /'
      fails=$((fails + 1))
    else
      echo "ok:   $1"
    fi
  }
  t "no reference" 1 alice "Just a change."
  t "bare number is not a reference" 1 alice "See #1."
  t "Fixes" 0 alice "Fixes #1"
  t "Closes" 0 alice "Closes #1"
  t "Resolves" 0 alice "Resolves #1"
  t "Part of" 0 alice "Part of #1"
  t "past tense" 0 alice "Fixed #1"
  t "mixed case" 0 alice "fIxEs #1"
  t "Part of mixed case" 0 alice "PART OF #1"
  t "keyword mid-text" 0 alice $'Summary\n\nThis change fixes #1 for good.'
  t "two valid issues" 0 alice $'Fixes #1\nPart of #2'
  t "two refs, second closed" 1 alice "Fixes #1 closes #3"
  t "two refs, comma" 0 alice "Fixes #1, Part of #2"
  t "two refs, space" 0 alice "Fixes #1 Part of #2"
  t "two refs, second a pull request" 1 alice "Fixes #1 fixes #4"
  t "number that overflows to 1" 1 alice "Fixes #18446744073709551617"
  t "leading zeros" 0 alice "Fixes #001"
  t "pull request number" 1 alice "Fixes #4"
  t "closed issue" 1 alice "Fixes #3"
  t "missing number" 1 alice "Fixes #99"
  t "one bad of two" 1 alice $'Fixes #1\nFixes #3'
  t "other repository" 1 alice "Fixes other/repo#1"
  t "other repository URL-style" 1 alice "Part of someone/else#2"
  t "keyword as part of a word" 1 alice "prefixes #1"
  t "number prefix is not a match" 1 alice "Fixes #1abc"
  t "Dependabot" 0 "dependabot[bot]" ""
  t "Dependabot with bad ref" 0 "dependabot[bot]" "Fixes #99"
  t "other bot is not exempt" 1 "renovate[bot]" ""
  t "metacharacters, no ref" 1 alice '$(touch pwned) `id` ; rm -rf / | #1 "'"'"
  t "metacharacters with ref" 0 alice 'Fixes #1 $(touch pwned) `id` ; "x" '"'"
  if [ -e pwned ]; then
    echo "FAIL: body was executed"
    rm -f pwned
    fails=$((fails + 1))
  fi
  if [ "$fails" -ne 0 ]; then
    echo "$fails self-test failure(s)."
    return 1
  fi
  echo "All self-tests passed."
}

if [ "${1:-}" = "--self-test" ]; then
  self_test
  exit $?
fi

: "${REPO:?REPO is required}"
check "${PR_BODY:-}" "${PR_AUTHOR:-}"
