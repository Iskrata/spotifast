#!/bin/sh
# Renders demo states to PNG without opening a window (examples/render.rs),
# each in every theme and size, as OUT/THEME/SIZE/NAME.png.
#
#   packaging/render-shots.sh OUT [STATES]
#
# STATES is a file of `NAME render-arguments...` lines (# starts a comment;
# arguments cannot contain spaces). Without it, the pages a review usually
# needs are rendered. THEMES (default "dark light"), SIZES (default
# "1280x800") and SCALE (default 1) choose the variants.
#
# For a before-and-after review, render the baseline and the candidate into
# two folders and run packaging/render-review.py on them.
set -eu

if [ $# -lt 1 ] || [ $# -gt 2 ]; then
  echo "usage: $0 OUT [STATES]" >&2
  exit 2
fi
out=$1
states=${2:-}
themes=${THEMES:-dark light}
sizes=${SIZES:-1280x800}
scale=${SCALE:-1}

root=$(cd "$(dirname "$0")/.." && pwd)
cargo build --quiet --locked --manifest-path "$root/Cargo.toml" --features render --example render
render="${CARGO_TARGET_DIR:-$root/target}/debug/examples/render"

default_states() {
  cat <<'EOF'
home      --page home
album     --page album:alb0
playlist  --page playlist:pl1
artist    --page artist:art0
settings  --page settings
friends   --page home --show friends
queue     --page home --show queue
EOF
}

list() {
  if [ -n "$states" ]; then cat "$states"; else default_states; fi
}

list | while read -r name args; do
  case $name in '' | '#'*) continue ;; esac
  for theme in $themes; do
    for size in $sizes; do
      # shellcheck disable=SC2086 # the state's arguments split on spaces
      "$render" $args --theme "$theme" --size "$size" --scale "$scale" \
        --out "$out/$theme/$size/$name.png" </dev/null
    done
  done
done
