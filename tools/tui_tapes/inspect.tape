# Beat 3 — the inspect overlay (the passive "what could this threaten" read).
# Tab to the hand and press `i`: a centred panel shows the full card under the cursor —
# stats, keywords, rules text, and the REACH grid (★ the card, ● every tile it threatens).
# Select-shows-targets; inspect-shows-reach — the same two reads the web canvas draws.
Source "tools/tui_tapes/_common.tape"
Output "/tmp/vhs-recollect-tui/inspect.gif"   # video sink — the committed still is the Screenshot below

Hide
Type "recollect --seed 6 --faction solace"
Enter
Wait+Screen@20s /choose a style/
Type "1"
Enter
Wait+Screen@20s /Legal tellings/
Tab
Sleep 300ms
Type "i"
Wait+Screen@10s /Inspect/
Show
Sleep 800ms
Screenshot "docs/gallery/tui/shot-inspect.png"
Sleep 400ms
