#!/bin/bash
# usage: compare.sh <args for both: stereo model loops fade>  -- runs over all mods
ST=${ST:-20}; MD=${MD:-a1200}; LP=${LP:-0}; FD=${FD:-0}
MDN=0; [ "$MD" = a500 ] && MDN=1
RS=${RS:-./target/release/pt2wav}
pass=0; fail=0; skip=0
while read -r f; do
  b=/home/claude/cmp/$(basename "$f")
  MAXFRAMES=$((44100*240)) timeout -s KILL 120 /home/claude/cref/harness "$f" "$b.c.wav" $ST $MDN $LP $FD >/dev/null 2>&1; rc=$?
  ropt="--max-seconds 240 --stereo $ST --model $MD --loops $LP"; [ "$FD" != 0 ] && ropt="$ropt --fadeout $FD"
  timeout -s KILL 120 $RS "$f" "$b.r.wav" $ropt >/dev/null 2>"$b.r.err"; rr=$?
  if [ $rc != 0 ] && [ $rr != 0 ]; then skip=$((skip+1)); continue; fi
  if [ $rc != 0 ] || [ $rr != 0 ]; then echo "STATUS-DIFF c=$rc r=$rr $f $(head -1 $b.r.err)"; fail=$((fail+1)); continue; fi
  if cmp -s "$b.c.wav" "$b.r.wav"; then pass=$((pass+1)); else
    echo "DIFF $f $(stat -c%s "$b.c.wav") $(stat -c%s "$b.r.wav") first@$(cmp "$b.c.wav" "$b.r.wav" | awk '{print $5}')"; fail=$((fail+1)); fi
  rm -f "$b.c.wav" "$b.r.wav"
done
echo "pass=$pass fail=$fail skip(both reject)=$skip"
