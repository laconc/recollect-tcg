# Beat 6 — Nightfall, the result screen.
# The cursor TUI hands back to the normal screen at Nightfall, where the closing board +
# the "— NIGHTFALL —" result line print (identically to the line mode). A move-by-move
# keystroke playout of twelve rounds isn't deterministic to script, so this beat uses
# `watch` (spectate two AIs) on the SAME seed — a fully auto, reproducible playout — and
# captures its final screenful.
#
# Why a fixed Sleep here (not a Wait+Screen gate like the other scenes): `watch` is a
# DETERMINISTIC, instant firehose — it prints the whole match and exits in well under a
# second, faster than VHS samples the screen, so a `Wait+Screen /NIGHTFALL/` never catches
# the final frame. The playout being deterministic, a short settle is reliable: `clear`
# first so the closing board is the only thing on the page, then photograph the stable
# final screen (the Nightfall board + "— NIGHTFALL —" result line). Image twin of
# tui-result.txt.
Source "tools/tui_tapes/_common.tape"
Output "/tmp/vhs-recollect-tui/result.gif"   # video sink — the committed still is the Screenshot below

Hide
Type "clear; recollect watch --seed 6 --faction solace"
Enter
Sleep 4s
Show
Sleep 600ms
Screenshot "docs/gallery/tui/shot-result.png"
Sleep 400ms
