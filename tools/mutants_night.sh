#!/usr/bin/env bash
# Mutation testing night run: proves the tests are not there for show. cargo-mutants breaks the
# code one small edit at a time and runs the crate's own tests; every edit no test notices
# ("missed") goes into a report an agent works from (mutants/LATEST.md).
#
#   tools/mutants_night.sh 8h                          # the whole crate: the overnight run
#   tools/mutants_night.sh 2h 'src/m3/**,src/json.rs'  # only these files (cargo-mutants globs)
#   tools/mutants_night.sh 8h all 2                    # two mutants at a time
#
# Resumable: the whole crate can be more mutants than one night holds. The run stops when the
# budget is spent; the next run skips everything already caught and goes on from there, while
# missed mutants are tried again (so a fix shows up as caught). `rm -rf mutants/state` starts
# over, e.g. after a large refactor.
#
# JOBS: keep 1, at most 2. Each job is its own build directory plus a nextest run on every core.
#
# Output, all under mutants/ (gitignored):
#   REPORT_<time>.md, LATEST.md → newest report    what was missed, the diffs, what to do
#   state/mutants.out/                             cargo-mutants' own output: logs, diffs, json
# Scratch copies of the tree being mutated go to target/mutants-tmp/.
set -uo pipefail

cd "$(git rev-parse --show-toplevel)" || exit 2
if [[ -z "${IN_NIX_SHELL:-}" ]]; then
  exec nix develop --quiet -c bash "$0" "$@"
fi
# shellcheck source=tools/lib/night.sh
source tools/lib/night.sh

dur_arg="${1:-}"
files="${2:-all}"
jobs="${3:-1}"

if [[ -z "$dur_arg" ]]; then
  echo "usage: $0 <budget> [all|glob,glob…] [jobs]   (e.g. 8h, 2h 'src/m3/**' 1)" >&2
  exit 2
fi
if ! [[ "$jobs" =~ ^[12]$ ]]; then
  echo "refusing jobs '$jobs': use 1 (at most 2); more hangs this machine" >&2
  exit 2
fi
budget="$(parse_dur "$dur_arg")"
if ((budget <= 0)); then
  echo "bad duration: '$dur_arg'" >&2
  exit 2
fi

scope=()
if [[ "$files" != all ]]; then
  IFS=',' read -ra globs <<<"$files"
  for g in "${globs[@]}"; do
    scope+=(-f "$g")
  done
fi

ts="$(date +%Y%m%d_%H%M%S)"
state="mutants/state"
out="$state/mutants.out"
# The scratch copies live on the repo's disk (/tmp is on the small root disk and each copy grows
# a target directory of its own), inside target/, which cargo-mutants never copies: a scratch
# directory inside the copied tree would copy itself recursively.
export TMPDIR="$PWD/target/mutants-tmp"
rm -rf "$TMPDIR"
mkdir -p "$TMPDIR" "$state"
started=$SECONDS

# ── what is in scope ─────────────────────────────────────────────────────────────────────────
total="$(cargo mutants --list "${scope[@]}" 2>"$state/list.log" | grep -c ':')"
if ((total == 0)); then
  echo "cargo mutants --list found no mutants for '$files': nothing to test" >&2
  tail -n 20 "$state/list.log" >&2
  exit 1
fi
known=0
for f in caught.txt previously_caught.txt unviable.txt; do
  [[ -f "$out/$f" ]] && known=$((known + $(sort -u "$out/$f" | wc -l)))
done
say "${night_bold}mutation run ${ts}${night_reset}: $files · $total mutants in scope · ~$known settled by earlier runs and skipped · budget $dur_arg · $jobs job(s)"
say "${night_dim}cargo-mutants draws its own progress bar below; each MISSED line is a hole in the tests.${night_reset}"
say "${night_dim}report at the end: mutants/LATEST.md   Ctrl-C stops and still writes it${night_reset}"
say ""

# ── run ──────────────────────────────────────────────────────────────────────────────────────
# stdout stays on the terminal: cargo-mutants shows its progress bar only there. SIGINT at the
# budget makes it stop cleanly and keep what it has; KILL five minutes later if it does not.
# --iterate: skip what earlier runs caught (the night-to-night resume).
timeout --foreground -s INT -k 5m "$budget" \
  cargo mutants "${scope[@]}" --iterate --output "$state" --jobs "$jobs"
rc=$?
rm -rf "$TMPDIR"

# 124 = budget spent; otherwise cargo-mutants' own code: 0 all caught, 2 missed, 3 timeouts,
# 4 the unmutated tree already fails its tests.
case "$rc" in
  0 | 2 | 3) status="finished" ;;
  124) status="budget spent, the next run continues" ;;
  130) status="interrupted" ;;
  4) status="BASELINE FAILED: the tests fail without any mutation" ;;
  *) status="RUN FAILED: cargo-mutants exited with $rc" ;;
esac

# ── report ───────────────────────────────────────────────────────────────────────────────────
report="mutants/REPORT_${ts}.md"
outcomes="$out/outcomes.json"
count() { jq -r --arg s "$1" '[.outcomes[] | select(.summary == $s)] | length' "$outcomes"; }

# outcome → markdown sections, one per mutant: location, the function to re-check, the diff.
sections() {
  jq -r --arg s "$1" '
    .outcomes[] | select(.summary == $s) | .diff_path as $diff | .scenario.Mutant
    | "\(.file)\t\(.function.function_name // "")\t\(.name)\t\($diff)"' "$outcomes" |
    sort -t $'\t' -k1,1 -k3,3V |
    while IFS=$'\t' read -r file fn name diff; do
      echo "### \`${name}\`"
      echo
      if [[ -n "$fn" ]]; then
        # -F is a regex: escape the function name (`<T>`, `[f32]`, `*` …) to match it literally.
        # shellcheck disable=SC2001 # a character class with a back-reference needs sed
        echo "Re-check: \`cargo mutants -f $file -F '$(sed 's/[][\\.^$*+?(){}|]/\\&/g' <<<"$fn")'\`"
        echo
      fi
      echo '```diff'
      if [[ -f "$out/$diff" ]]; then
        cat "$out/$diff"
      else
        echo "(no diff saved: $out/$diff)"
      fi
      echo '```'
      echo
    done
}

sha="$(git rev-parse --short HEAD)"
branch="$(git branch --show-current)"
dirty=""
[[ -n "$(git status --porcelain)" ]] && dirty=" (working tree had uncommitted changes)"
missed=0 timeouts=0 caught=0 unviable=0
if [[ -f "$outcomes" ]]; then
  missed="$(count MissedMutant)" timeouts="$(count Timeout)"
  caught="$(count CaughtMutant)" unviable="$(count Unviable)"
fi
tested=$((missed + timeouts + caught + unviable))
{
  echo "# Mutation run $ts"
  echo
  echo "- git: \`$sha\` on \`$branch\`$dirty"
  echo "- scope: \`$files\`, $total mutants; budget \`$dur_arg\`, $jobs job(s); wall time $(fmt_dur $((SECONDS - started)))"
  echo "- status: **$status**"
  echo "- tested this run: $tested · caught $caught · **missed $missed** · timeouts $timeouts · unviable $unviable (did not compile, fine)"
  echo "- settled by earlier runs and skipped: ~$known; not reached yet: ~$((total - known - tested < 0 ? 0 : total - known - tested))"
  echo
  if ((rc == 4)); then
    echo "## The baseline fails"
    echo
    echo "The tests fail on the unmutated tree, so no mutant was tried. Fix the suite first:"
    echo "\`$out/log/baseline.log\`, then \`cargo t\`."
    echo
  elif [[ "$status" == RUN\ FAILED* ]]; then
    echo "## The run failed"
    echo
    echo "cargo-mutants stopped with exit code $rc before finishing; the numbers above are partial."
    echo "Its last messages: \`$out/debug.log\` (when it got that far) and the terminal output."
    echo
  fi
  if ((missed > 0 || timeouts > 0)); then
    cat <<'EOF'
## How to work this report (agent)

A **missed** mutant is a change to `src/` that every test still passes: behaviour no test pins
down. For each one, in a `test/` branch:

1. Read the diff and ask what observable behaviour it breaks. Write the test that notices: a
   bolero invariant in `tests/` (repo rules apply: named scenario
   types, generators in range, invariants rather than one hard-coded output). Prefer one stronger
   invariant that kills several mutants of a function over one test per mutant.
2. Prove it: the `Re-check` command must now report the mutant as caught (no `MISSED` line).
   Apply the diff by hand if you want to see the new test fail on it.
3. If the mutant truly cannot change behaviour (an equivalent mutant, e.g. a branch that only
   saves work), do not write a fake test: add its name to `exclude_re` in `.cargo/mutants.toml`
   with a comment saying why. If it shows dead code, delete the code instead.
4. `tools/gate quick`, commit (`test(<module>): …`). The next night run re-tries every missed
   mutant, so fixed ones move to caught on their own.

A **timeout** means the mutant made a test hang (a loop that never ends). It counts as caught,
but check that the loop's bound is really tested and the test would fail fast, not just hang.

EOF
  fi
  if ((missed > 0)); then
    echo "## Missed ($missed)"
    echo
    sections MissedMutant
  fi
  if ((timeouts > 0)); then
    echo "## Timeouts ($timeouts)"
    echo
    sections Timeout
  fi
  if ((missed == 0 && timeouts == 0)) && [[ "$status" != *FAILED* ]]; then
    echo "## Result"
    echo
    echo "Every mutant tested this run was caught. Nothing to fix."
    echo
  fi
  echo "Full output: \`$out/\` (\`outcomes.json\`, per-mutant \`log/\` and \`diff/\`)."
} >"$report"
ln -sfn "REPORT_${ts}.md" mutants/LATEST.md

say ""
if [[ "$status" == *FAILED* ]]; then
  say "${night_red}${night_bold}${status}${night_reset}"
  say "report: $report  (mutants/LATEST.md)"
elif ((missed > 0)); then
  say "${night_red}${night_bold}$missed missed mutant(s)${night_reset} · $caught caught · $timeouts timeouts · $unviable unviable · $status"
  say "report: $report  (mutants/LATEST.md)  → hand it to the agent: \"kill the missed mutants in mutants/LATEST.md\""
else
  say "${night_green}${night_bold}no missed mutants${night_reset} · $caught caught · $timeouts timeouts · $unviable unviable · $status"
  say "report: $report  (mutants/LATEST.md)"
fi
case "$rc" in
  0 | 124) ((missed > 0)) && exit 1 || exit 0 ;;
  2 | 3) exit 1 ;;
  *) exit "$rc" ;;
esac
