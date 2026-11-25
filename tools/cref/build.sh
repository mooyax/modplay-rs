#!/bin/bash
# Builds a headless reference renderer from the ORIGINAL pt2-clone C sources (Linux/WSL).
# usage: tools/cref/build.sh /path/to/pt2-clone
set -e
S="$1/src"
D="$(cd "$(dirname "$0")" && pwd)"
FILES="$S/pt2_paula.c $S/pt2_blep.c $S/pt2_rcfilters.c $S/pt2_downsample2x.c $S/pt2_replayer.c $S/pt2_audio.c $S/pt2_tables.c $S/pt2_module_loader.c $S/modloaders/pt2_load_mod31.c $S/modloaders/pt2_load_mod15.c $S/modloaders/pt2_pp_unpack.c $S/modloaders/pt2_xpk_unpack.c $S/pt2_structs.c $S/pt2_hpc.c"
gcc -O2 -w -I"$S" -o "$D/harness" "$D/harness.c" "$D/stubs.c" $FILES $(sdl2-config --cflags --libs) -lm
echo "built $D/harness"
