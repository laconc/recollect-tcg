# Beat 1 — the cursor board at rest.
# The gold cursor `[ ]` sits on the centre tile (c3); the rich Match pane (round /
# score / projections) and the always-visible "Legal tellings" list flank it; the
# green "quiet dots" mark tiles with an available action. Image twin of cursor-board.txt.
#
# Driven by `recollect` on PATH (the freshly-built binary; see tools/gen_tui_shots.sh)
# with the gallery's fixed seed so the opening is identical every run.
Source "tools/tui_tapes/_common.tape"
Output "/tmp/vhs-recollect-tui/board.gif"   # video sink — the committed still is the Screenshot below

Hide
Type "recollect --seed 6 --faction solace"
Enter
Wait+Screen@20s /choose a style/
Type "1"
Enter
Wait+Screen@20s /Legal tellings/
Show
Sleep 1s
Screenshot "docs/gallery/tui/shot-board.png"
Sleep 400ms
