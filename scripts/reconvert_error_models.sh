#!/usr/bin/env bash
# Reconvert the error_models/ test set from their original M3 sources.
#
# These are War3-Reforged / SC2 models gathered because their conversion was
# buggy; this script makes regeneration + spec-validation one command after a
# converter fix.
#
# Usage:
#   scripts/reconvert_error_models.sh [MODEL_NAME ...]
#
# With no args, reconverts all models. Pass one or more names (the output GLB
# stem, e.g. War3_Kelthuzad) to reconvert just those.
#
# Env:
#   TEX_ROOT   texture search root passed via -t   (default below)
#   SRC_ROOT   StarCraft extraction root           (default below)
#   BIN        converter binary                     (default ./target/release/m3-to-glb)
set -euo pipefail

SRC_ROOT="${SRC_ROOT:-/mnt/Projects/StarCraftExtracted/out}"
TEX_ROOT="${TEX_ROOT:-$SRC_ROOT}"
BIN="${BIN:-./target/release/m3-to-glb}"
OUT_DIR="error_models"

# output stem  ->  source .m3 path, relative to $SRC_ROOT
declare -A MODELS=(
  [Primalisk]="mods/liberty.sc2mod/base.sc2assets/Assets/Units/Zerg/Primalisk/Primalisk.m3"
  [Ultralisk_RCZ]="mods/liberty.sc2mod/base.sc2assets/Assets/Units/Zerg/Ultralisk_RCZ/Ultralisk_RCZ.m3"
  [Ultralisk_Remastered]="mods/liberty.sc2mod/base.sc2assets/Assets/Units/Zerg/Ultralisk_Remastered/Ultralisk_Remastered.m3"
  [War3_Acolyte]="mods/war3.sc2mod/base.sc2assets/Assets/Units/Undead/War3_Acolyte/War3_Acolyte.m3"
  [War3_Archnathid]="mods/war3.sc2mod/base.sc2assets/Assets/Units/Creeps/War3_Archnathid/War3_Archnathid.m3"
  [War3_Bandit]="mods/war3.sc2mod/base.sc2assets/Assets/Units/Creeps/War3_Bandit/War3_Bandit.m3"
  [War3_Banshee]="mods/war3.sc2mod/base.sc2assets/Assets/Units/Undead/War3_Banshee/War3_Banshee.m3"
  [War3_HeroDreadLord]="mods/war3.sc2mod/base.sc2assets/Assets/Units/Undead/War3_HeroDreadLord/War3_HeroDreadLord.m3"
  [War3_HeroLich]="mods/war3.sc2mod/base.sc2assets/Assets/Units/Undead/War3_HeroLich/War3_HeroLich.m3"
  [War3_HeroLichCIN]="mods/war3.sc2mod/base.sc2assets/Assets/Units/Undead/War3_HeroLichCIN/War3_HeroLichCIN.m3"
  [War3_Kelthuzad]="mods/war3.sc2mod/base.sc2assets/Assets/Units/Undead/War3_Kelthuzad/War3_Kelthuzad.m3"
  [War3_Satyr]="mods/war3.sc2mod/base.sc2assets/Assets/Units/Creeps/War3_Satyr/War3_Satyr.m3"
  [War3_Shade]="mods/war3.sc2mod/base.sc2assets/Assets/Units/Undead/War3_Shade/War3_Shade.m3"
)

names=("$@")
if [ ${#names[@]} -eq 0 ]; then
  names=("${!MODELS[@]}")
fi

mkdir -p "$OUT_DIR"
fail=0
for name in "${names[@]}"; do
  rel="${MODELS[$name]:-}"
  if [ -z "$rel" ]; then
    echo "!! unknown model: $name" >&2; fail=1; continue
  fi
  src="$SRC_ROOT/$rel"
  out="$OUT_DIR/$name.glb"
  if [ ! -f "$src" ]; then
    echo "!! missing source: $src" >&2; fail=1; continue
  fi
  echo ">> $name"
  "$BIN" "$src" -o "$out" -t "$TEX_ROOT" -q
  if command -v gltf-transform >/dev/null 2>&1; then
    gltf-transform validate "$out" 2>&1 | grep -iE 'error:|No errors found' | head -1 || true
  fi
done

exit $fail
