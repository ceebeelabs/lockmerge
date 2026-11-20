#!/usr/bin/env bash
# Checks the architecture decision records in docs/adr/.
#
# Usage:
#   check-adrs.sh              check docs/adr/ (or the directory in ADR_DIR)
#   check-adrs.sh --self-test  offline test on sample trees
#
# Prints every problem and exits non-zero if there are any; on success prints
# "adrs: N records ok".
set -u

problems=0
fail() {
  echo "adrs: $1" >&2
  problems=$((problems + 1))
}

valid_date() {
  [[ $1 =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]] || return 1
  local y=$((10#${1:0:4})) m=$((10#${1:5:2})) d=$((10#${1:8:2})) max
  ((y >= 1 && m >= 1 && m <= 12 && d >= 1)) || return 1
  case $m in
    1 | 3 | 5 | 7 | 8 | 10 | 12) max=31 ;;
    4 | 6 | 9 | 11) max=30 ;;
    *) if ((y % 4 == 0 && (y % 100 != 0 || y % 400 == 0))); then max=29; else max=28; fi ;;
  esac
  ((d <= max))
}

# Keyed storage that works in bash 3.2 (no associative arrays): put NAME KEY VALUE,
# get NAME KEY. Keys are four digits.
put() { printf -v "_$1_$2" '%s' "$3"; }
get() { local v="_$1_$2"; printf '%s' "${!v:-}"; }

# Status words allowed without a link.
plain_status() {
  case $1 in Proposed | Accepted | Rejected | Deprecated) return 0 ;; esac
  return 1
}

check() {
  local dir=$1 f base n i expected=1 count=0
  local -a nums=() files=()
  if [[ ! -d $dir ]]; then
    fail "$dir does not exist"
    return
  fi
  local v
  for v in $(compgen -A variable | grep -E '^_(file_of|status_of|seen)_'); do unset "$v"; done
  shopt -s nullglob
  for f in "$dir"/*.md; do
    base=${f##*/}
    [[ $base == README.md || $base == template.md ]] && continue
    if [[ ! $base =~ ^([0-9]{4})-[a-z0-9]+(-[a-z0-9]+)*\.md$ ]]; then
      fail "$base: name must be NNNN-short-title.md (lowercase, hyphens)"
      continue
    fi
    n=${BASH_REMATCH[1]}
    if [[ -n $(get file_of "$n") ]]; then
      fail "$base: number $n is also used by $(get file_of "$n")"
      continue
    fi
    put file_of "$n" "$base"
    nums+=("$n")
    files+=("$base")
  done
  shopt -u nullglob

  # Numbers run from 0001 with no gaps.
  local sorted
  sorted=$(printf '%s\n' "${nums[@]+"${nums[@]}"}" | sort)
  for n in $sorted; do
    if ((10#$n != expected)); then
      fail "numbers must run from 0001 without gaps: expected $(printf '%04d' "$expected"), found $n"
      expected=$((10#$n))
    fi
    expected=$((expected + 1))
  done

  local title status date line first
  local words='(proposed|accepted|rejected|deprecated|superseded)'
  local statustitle="[\\(\\[][[:space:]]*${words}|[-:–—][[:space:]]*${words}[[:space:]]*\$"
  local restore
  restore=$(shopt -p nocasematch)
  shopt -s nocasematch
  for base in "${files[@]+"${files[@]}"}"; do
    n=${base:0:4}
    count=$((count + 1))
    IFS= read -r first <"$dir/$base" || true
    first=${first%$'\r'}
    if [[ ! $first =~ ^\#\ ([0-9]{4})\.\ (.*)$ ]]; then
      fail "$base: first line must be '# $n. <title>'"
    else
      if [[ ${BASH_REMATCH[1]} != "$n" ]]; then
        fail "$base: title number ${BASH_REMATCH[1]} does not match the file number $n"
      fi
      title=${BASH_REMATCH[2]}
      if [[ -z ${title//[[:space:]]/} ]]; then
        fail "$base: title is empty"
      elif [[ $title =~ $statustitle ]]; then
        fail "$base: title must not carry a status"
      fi
    fi

    status=
    date=
    local nstatus=0 ndate=0
    while IFS= read -r line; do
      line=${line%$'\r'}
      if [[ $line == "- Status: "* ]]; then
        status=${line#- Status: }
        nstatus=$((nstatus + 1))
      elif [[ $line == "- Date: "* ]]; then
        date=${line#- Date: }
        ndate=$((ndate + 1))
      fi
    done <"$dir/$base"
    if ((nstatus != 1)); then
      fail "$base: expected exactly one '- Status: ' line, found $nstatus"
    elif plain_status "$status"; then
      :
    elif [[ $status =~ ^Superseded\ by\ \[([0-9]{4})\]\(([0-9]{4}-[^/\)]+\.md)\)$ ]]; then
      if [[ ${BASH_REMATCH[1]} != "${BASH_REMATCH[2]:0:4}" ]]; then
        fail "$base: superseding number and link disagree"
      elif [[ $(get file_of "${BASH_REMATCH[1]}") != "${BASH_REMATCH[2]}" ]]; then
        fail "$base: superseded by ${BASH_REMATCH[2]}, which is not a record"
      fi
    else
      fail "$base: status '$status' is not Proposed, Accepted, Rejected, Deprecated or 'Superseded by [NNNN](NNNN-title.md)'"
    fi
    put status_of "$n" "$status"
    if ((ndate != 1)); then
      fail "$base: expected exactly one '- Date: ' line, found $ndate"
    elif ! valid_date "$date"; then
      fail "$base: date '$date' is not a real YYYY-MM-DD date"
    fi
  done

  eval "$restore"

  # Index table: exactly one row per record.
  local readme=$dir/README.md
  if [[ ! -f $readme ]]; then
    fail "$readme does not exist"
    return
  fi
  local cell_n cell_file cell_status rowre
  rowre='^\|[[:space:]]*\[([0-9]{4})\]\(([^)]*)\)[[:space:]]*\|(.*)\|[[:space:]]*([^|]*[^|[:space:]])[[:space:]]*\|[[:space:]]*$'
  while IFS= read -r line; do
    line=${line%$'\r'}
    [[ $line == "| ["* ]] || continue
    if [[ ! $line =~ $rowre ]]; then
      fail "index row is malformed: $line"
      continue
    fi
    cell_n=${BASH_REMATCH[1]}
    cell_file=${BASH_REMATCH[2]}
    cell_status=${BASH_REMATCH[4]}
    if [[ -n $(get seen "$cell_n") ]]; then
      fail "index has more than one row for $cell_n"
      continue
    fi
    put seen "$cell_n" 1
    if [[ -z $(get file_of "$cell_n") ]]; then
      fail "index row for $cell_n has no record file"
    elif [[ $cell_file != "$(get file_of "$cell_n")" ]]; then
      fail "index row for $cell_n links $cell_file, expected $(get file_of "$cell_n")"
    elif [[ $cell_status != "$(get status_of "$cell_n")" ]]; then
      fail "index row for $cell_n says '$cell_status', the record says '$(get status_of "$cell_n")'"
    fi
  done <"$readme"
  for n in "${nums[@]+"${nums[@]}"}"; do
    [[ -n $(get seen "$n") ]] || fail "index has no row for $(get file_of "$n")"
  done
  CHECKED=$count
}

run_check() {
  problems=0
  CHECKED=0
  check "$1"
  if ((problems > 0)); then
    return 1
  fi
  echo "adrs: $CHECKED records ok"
}

# --- self-test --------------------------------------------------------------

mkrecord() { # dir file number title status date
  printf '# %s. %s\n\n- Status: %s\n- Date: %s\n\n## Context\n\nText.\n' "$3" "$4" "$5" "$6" >"$1/$2"
}
mkindex() { # dir rows...
  local d=$1
  shift
  { printf '# ADRs\n\n## Index\n\n| ADR | Decision | Status |\n| --- | --- | --- |\n'; printf '%s\n' "$@"; } >"$d/README.md"
}
good_tree() {
  mkdir -p "$1"
  mkrecord "$1" 0001-first.md 0001 "First" Accepted 2026-01-31
  mkrecord "$1" 0002-second.md 0002 "Second" Proposed 2026-02-01
  echo "# Template" >"$1/template.md"
  mkindex "$1" '| [0001](0001-first.md) | First | Accepted |' '| [0002](0002-second.md) | Second | Proposed |'
}

self_test() {
  local fails=0 t out
  tmp=$(mktemp -d "${TMPDIR:-/tmp}/adrs.XXXXXX") || exit 1
  trap 'rm -rf "${tmp:-}"' EXIT
  t=0

  expect() { # name expected(ok|fail) pattern dir
    local name=$1 want=$2 pat=$3 dir=$4 rc
    out=$(run_check "$dir" 2>&1)
    rc=$?
    if [[ $want == ok && $rc -ne 0 ]] || [[ $want == fail && $rc -eq 0 ]]; then
      echo "FAIL: $name (exit $rc): $out"
      fails=$((fails + 1))
    elif [[ -n $pat && $out != *"$pat"* ]]; then
      echo "FAIL: $name: output lacks '$pat': $out"
      fails=$((fails + 1))
    else
      echo "ok: $name"
    fi
    t=$((t + 1))
  }

  local d
  d=$tmp/good; good_tree "$d"
  expect "valid tree" ok "adrs: 2 records ok" "$d"

  d=$tmp/superseded; good_tree "$d"
  mkrecord "$d" 0001-first.md 0001 "First" "Superseded by [0002](0002-second.md)" 2026-03-01
  mkindex "$d" '| [0001](0001-first.md) | First | Superseded by [0002](0002-second.md) |' '| [0002](0002-second.md) | Second | Proposed |'
  expect "superseded by an existing record" ok "2 records ok" "$d"
  mkrecord "$d" 0001-first.md 0001 "First" "Superseded by [0009](0009-nope.md)" 2026-03-01
  mkindex "$d" '| [0001](0001-first.md) | First | Superseded by [0009](0009-nope.md) |' '| [0002](0002-second.md) | Second | Proposed |'
  expect "superseded by a missing record" fail "not a record" "$d"

  d=$tmp/leap; good_tree "$d"
  mkrecord "$d" 0001-first.md 0001 "First" Accepted 2024-02-29
  expect "leap day" ok "" "$d"

  d=$tmp/badname; good_tree "$d"
  mkrecord "$d" 12-short.md 12 "Short" Proposed 2026-02-02
  expect "bad file name" fail "name must be NNNN-short-title.md" "$d"

  d=$tmp/dupe; good_tree "$d"
  mkrecord "$d" 0002-other.md 0002 "Other" Proposed 2026-02-02
  expect "duplicate number" fail "also used by" "$d"

  d=$tmp/gap; good_tree "$d"
  mkrecord "$d" 0002-second.md 0003 "Second" Proposed 2026-02-01
  mv "$d/0002-second.md" "$d/0003-second.md"
  mkindex "$d" '| [0001](0001-first.md) | First | Accepted |' '| [0003](0003-second.md) | Second | Proposed |'
  expect "gap in numbers" fail "without gaps" "$d"

  d=$tmp/start; good_tree "$d"
  rm "$d/0001-first.md"
  mkindex "$d" '| [0002](0002-second.md) | Second | Proposed |'
  expect "numbers not starting at 0001" fail "expected 0001" "$d"

  d=$tmp/titlenum; good_tree "$d"
  mkrecord "$d" 0002-second.md 0003 "Second" Proposed 2026-02-01
  expect "title number mismatch" fail "does not match the file number" "$d"

  d=$tmp/notitle; good_tree "$d"
  printf 'Second\n\n- Status: Proposed\n- Date: 2026-02-01\n' >"$d/0002-second.md"
  expect "missing heading" fail "first line must be" "$d"

  d=$tmp/emptytitle; good_tree "$d"
  mkrecord "$d" 0002-second.md 0002 "" Proposed 2026-02-01
  expect "empty title" fail "title is empty" "$d"

  d=$tmp/statustitle; good_tree "$d"
  mkrecord "$d" 0002-second.md 0002 "Second (Proposed)" Proposed 2026-02-01
  expect "status in title" fail "must not carry a status" "$d"
  mkrecord "$d" 0002-second.md 0002 "Second [Accepted]" Proposed 2026-02-01
  expect "status in brackets in title" fail "must not carry a status" "$d"

  d=$tmp/badstatus; good_tree "$d"
  mkrecord "$d" 0002-second.md 0002 "Second" "Approved" 2026-02-01
  mkindex "$d" '| [0001](0001-first.md) | First | Accepted |' '| [0002](0002-second.md) | Second | Approved |'
  expect "unknown status" fail "is not Proposed" "$d"

  d=$tmp/nostatus; good_tree "$d"
  printf '# 0002. Second\n\n- Date: 2026-02-01\n' >"$d/0002-second.md"
  expect "missing status line" fail "exactly one '- Status: '" "$d"

  d=$tmp/baddate; good_tree "$d"
  mkrecord "$d" 0002-second.md 0002 "Second" Proposed 2026-02-30
  expect "impossible date" fail "not a real" "$d"
  mkrecord "$d" 0002-second.md 0002 "Second" Proposed 2026-2-1
  expect "wrong date form" fail "not a real" "$d"
  mkrecord "$d" 0002-second.md 0002 "Second" Proposed 2025-02-29
  expect "non-leap Feb 29" fail "not a real" "$d"

  d=$tmp/nodate; good_tree "$d"
  printf '# 0002. Second\n\n- Status: Proposed\n' >"$d/0002-second.md"
  expect "missing date line" fail "exactly one '- Date: '" "$d"

  d=$tmp/missingrow; good_tree "$d"
  mkindex "$d" '| [0001](0001-first.md) | First | Accepted |'
  expect "record missing from index" fail "index has no row for 0002-second.md" "$d"

  d=$tmp/extrarow; good_tree "$d"
  mkindex "$d" '| [0001](0001-first.md) | First | Accepted |' '| [0002](0002-second.md) | Second | Proposed |' '| [0003](0003-third.md) | Third | Proposed |'
  expect "index row without a file" fail "has no record file" "$d"

  d=$tmp/duprow; good_tree "$d"
  mkindex "$d" '| [0001](0001-first.md) | First | Accepted |' '| [0001](0001-first.md) | First | Accepted |' '| [0002](0002-second.md) | Second | Proposed |'
  expect "duplicate index row" fail "more than one row" "$d"

  d=$tmp/wronglink; good_tree "$d"
  mkindex "$d" '| [0001](0001-other.md) | First | Accepted |' '| [0002](0002-second.md) | Second | Proposed |'
  expect "index links the wrong file" fail "expected 0001-first.md" "$d"

  d=$tmp/wrongstatus; good_tree "$d"
  mkindex "$d" '| [0001](0001-first.md) | First | Proposed |' '| [0002](0002-second.md) | Second | Proposed |'
  expect "index status differs" fail "the record says 'Accepted'" "$d"

  d=$tmp/all; good_tree "$d"
  mkrecord "$d" 0002-second.md 0002 "Second" Bogus 2026-13-01
  mkindex "$d" '| [0001](0001-first.md) | First | Accepted |'
  out=$(run_check "$d" 2>&1)
  if [[ $(grep -c '^adrs:' <<<"$out") -ge 3 ]]; then
    echo "ok: every problem is printed"
  else
    echo "FAIL: every problem is printed: $out"
    fails=$((fails + 1))
  fi
  t=$((t + 1))

  d=$tmp/empty; mkdir -p "$d"; mkindex "$d"
  expect "no records" ok "0 records ok" "$d"

  d=$tmp/nodir
  expect "missing directory" fail "does not exist" "$d"

  if ((fails > 0)); then
    echo "adrs self-test: $fails of $t failed"
    exit 1
  fi
  echo "adrs self-test: $t passed"
}

if [[ ${1:-} == --self-test ]]; then
  self_test
  exit 0
fi
run_check "${ADR_DIR:-docs/adr}"
