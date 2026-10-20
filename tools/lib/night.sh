# shellcheck shell=bash
# Shared by the unattended night runs (tools/fuzz_night.sh, tools/mutants_night.sh): durations,
# a text progress bar and a status block that redraws in place on a terminal. Sourced, not run.
#
# On a terminal the status block is redrawn every second; when stdout is a file or a pipe (an
# agent, `> night.log`) the block is printed as plain lines once a minute instead, so a log
# stays readable and still shows that the run is alive.

night_tty=0
[[ -t 1 ]] && night_tty=1
night_block_lines=0
night_last_beat=0

# The colours are read by the scripts that source this file.
# shellcheck disable=SC2034
if ((night_tty)); then
  night_bold=$'\033[1m' night_dim=$'\033[2m' night_red=$'\033[31m' night_green=$'\033[32m'
  night_yellow=$'\033[33m' night_reset=$'\033[0m'
else
  night_bold='' night_dim='' night_red='' night_green='' night_yellow='' night_reset=''
fi

# "90", "15m", "8h" → seconds; 0 when malformed.
parse_dur() {
  local d="$1"
  if [[ "$d" =~ ^([0-9]+)([smhSMH]?)$ ]]; then
    local n="${BASH_REMATCH[1]}" unit="${BASH_REMATCH[2],,}"
    case "$unit" in
      h) echo $((n * 3600)) ;;
      m) echo $((n * 60)) ;;
      *) echo $((n)) ;;
    esac
  else
    echo 0
  fi
}

# Seconds → "2h05m", "4m07s", "12s".
fmt_dur() {
  local s=$(($1 < 0 ? 0 : $1))
  if ((s >= 3600)); then
    printf '%dh%02dm' $((s / 3600)) $((s % 3600 / 60))
  elif ((s >= 60)); then
    printf '%dm%02ds' $((s / 60)) $((s % 60))
  else
    printf '%ds' "$s"
  fi
}

# 1234567 → "1.2M", 9100 → "9.1k".
fmt_count() {
  local n="$1"
  if ((n >= 1000000)); then
    printf '%d.%dM' $((n / 1000000)) $((n % 1000000 / 100000))
  elif ((n >= 1000)); then
    printf '%d.%dk' $((n / 1000)) $((n % 1000 / 100))
  else
    printf '%d' "$n"
  fi
}

# bar <done> <total> <width> → "[██████░░░░]"
bar() {
  local done="$1" total="$2" width="$3" fill=0
  ((total > 0)) && fill=$((done * width / total))
  ((fill > width)) && fill=$width
  local full empty
  printf -v full '%*s' "$fill" ''
  printf -v empty '%*s' $((width - fill)) ''
  printf '[%s%s]' "${full// /█}" "${empty// /░}"
}

night_cols() {
  local size
  size="$(stty size </dev/tty 2>/dev/null)" || size=""
  local cols="${size#* }"
  [[ "$cols" =~ ^[0-9]+$ ]] && ((cols > 20)) && echo "$cols" || echo 120
}

# Erase the status block so a permanent line can be printed above it.
block_clear() {
  if ((night_tty && night_block_lines > 0)); then
    printf '\033[%dA\033[J' "$night_block_lines"
  fi
  night_block_lines=0
}

# block_draw <line>... : replace the status block with these lines (cut to the terminal width).
block_draw() {
  if ((!night_tty)); then
    if ((SECONDS - night_last_beat >= 60)); then
      night_last_beat=$SECONDS
      printf '%s\n' "$@"
    fi
    return
  fi
  block_clear
  local cols line
  cols="$(night_cols)"
  for line in "$@"; do
    printf '%s\033[K\n' "${line:0:cols-1}"
  done
  night_block_lines=$#
}

# A permanent line (a finished target, an error) printed above the status block.
say() {
  block_clear
  printf '%s\n' "$*"
}
