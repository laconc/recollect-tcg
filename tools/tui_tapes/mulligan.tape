# Beat 5 — the once-per-match Mulligan, offered at the opening.
# Open the verb mini-buffer (`:`) and type `mull` (the Mulligan verb) without
# committing: the Status line shows `verb> mull` while the Legal plays list still
# carries entry 2, "Mulligan (redraw a fresh hand, bottom one — once, at the opening)".
# This is the opening-window affordance — gone once you've acted. The numbered entry and
# the verb resolve to the same `Mulligan` command.
Source "tools/tui_tapes/_common.tape"
Output "/tmp/vhs-recollect-tui/mulligan.gif"   # video sink — the committed still is the Screenshot below

Hide
Type "recollect --seed 6 --faction solace"
Enter
Wait+Screen@20s /choose a style/
Type "1"
Enter
Wait+Screen@20s /Legal plays/
# Open the verb buffer and type the mulligan verb (no Enter — show the affordance).
Type ":"
Sleep 300ms
Type "mull"
Wait+Screen@10s /verb> mull/
Show
Sleep 800ms
Screenshot "docs/gallery/tui/shot-mulligan.png"
Sleep 400ms
