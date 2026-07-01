# Beat 4 — the Glimpse prompt (the burn step).
# Pick play 0 (Glimpse) from the always-visible list: the engine raises the
# burn choice, drawn as a centred overlay — "Glimpse — burn a card to Glimpse
# (it leaves play):" over the brass-gold, cursor-pickable burn options. Image twin
# of cursor-glimpse.txt. (Vocabulary: a card is BURNED to Glimpse — the cost — never
# "discarded".)
Source "tools/tui_tapes/_common.tape"
Output "/tmp/vhs-recollect-tui/glimpse.gif"   # video sink — the committed still is the Screenshot below

Hide
Type "recollect --seed 6 --faction solace"
Enter
Wait+Screen@20s /choose a style/
Type "1"
Enter
Wait+Screen@20s /Legal plays/
# Play 0 is Glimpse; the number opens the verb buffer pre-seeded, Enter applies it.
Type "0"
Enter
Wait+Screen@10s /burn a card to Glimpse/
Show
Sleep 800ms
Screenshot "docs/gallery/tui/shot-glimpse.png"
Sleep 400ms
