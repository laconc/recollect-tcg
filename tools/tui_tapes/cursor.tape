# The motion clip — the gold cursor moving, then a pickup -> place.
# A short GIF (the only animated artifact): the gold cursor steers the board with the
# arrows, then Tab -> hand -> lift Frenzy Kit -> the targets light gold -> place it, and
# a Lorekeeper spirit arrives on the page. The terminal twin of the web's gold-cursor
# interaction. Only the moving beats are `Show`n; the launch + deck pick stay `Hide`den.
Source "tools/tui_tapes/_common.tape"
Output "docs/gallery/tui/cursor.gif"
Set PlaybackSpeed 1.0

Hide
Type "recollect --seed 6 --faction solace"
Enter
Wait+Screen@20s /choose a style/
Type "1"
Enter
Wait+Screen@20s /Legal plays/
Sleep 500ms
Show

# Steer the gold cursor around the page (a small tour from the centre, c3).
Sleep 700ms
Up
Sleep 450ms
Right
Sleep 450ms
Down
Sleep 450ms
Down
Sleep 450ms
Left
Sleep 450ms
Left
Sleep 700ms

# Lift a hand card: Tab to the hand, step to Frenzy Kit (index 1), pick it up. Focus
# jumps to the board and every legal landing tile lights gold `< >`.
Tab
Sleep 600ms
Down
Sleep 600ms
Enter
Wait+Screen@10s /placing/
Sleep 900ms

# Aim a gold target and place it. After the lift the cursor is back on the centre tile;
# Down x3 clamps it down to row 1 (a guaranteed gold target), then Enter places it —
# Frenzy Kit arrives on the page as a Lorekeeper spirit.
Down
Sleep 400ms
Down
Sleep 400ms
Down
Sleep 500ms
Enter
Wait+Screen@10s /Play Frenzy Kit/
Sleep 1600ms
