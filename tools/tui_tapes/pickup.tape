# Beat 2 — a hand card lifted, its legal targets in gold.
# Tab to the hand, step to Frenzy Kit (a 2-cost the opening Anima can afford), Enter
# to pick it up: the cursor jumps to the board and every tile it can be PLAYED onto
# lights gold `< >` (select-shows-targets — the engine's own legal placements, never a
# superset). Image twin of cursor-pickup.txt.
Source "tools/tui_tapes/_common.tape"
Output "/tmp/vhs-recollect-tui/pickup.gif"   # video sink — the committed still is the Screenshot below

Hide
Type "recollect --seed 6 --faction solace"
Enter
Wait+Screen@20s /choose a style/
Type "1"
Enter
Wait+Screen@20s /Legal tellings/
# Focus the hand, drop to Frenzy Kit (hand index 1), and lift it.
Tab
Sleep 400ms
Down
Sleep 400ms
Enter
Wait+Screen@10s /placing/
Show
Sleep 800ms
Screenshot "docs/gallery/tui/shot-pickup.png"
Sleep 400ms
