# Shared VHS settings for the Recollect cursor-TUI image gallery.
# Every scene tape `Source`s this so the terminal size, font, padding, window bar,
# and the BRASS-GOLD brand theme are identical across all stills + the clip.
#
# The theme maps the cursor TUI's ANSI colours onto Recollect's canonical palette
# ("paper & ink, a fading Memory" — docs/decisions/brand_and_accessibility.md):
#   background -> NIGHT  #17171F (the ink night / the Dusk)
#   foreground -> PAPER  #F5F0E3 (the page)
#   yellow     -> brass-gold     (the gold cursor `[ ]`, lifted-piece targets `< >`,
#                                 choice overlays — Color::Yellow in src/tui.rs)
#   blue/cyan  -> SEAT_A ink-blue (Lorekeepers; the TUI draws Seat A in Cyan, lifted
#                                  to a legible blue on the dark page)
#   magenta    -> SEAT_B ink-red  (the Solace)
#   green      -> growth green    (the "quiet dot" actionable cue, Color::Green)
#   brightBlack-> a dim parchment-grey (faded ground / dim text, Color::DarkGray)
#
# Determinism: the binary is driven with a FIXED --seed, and every capture is gated
# on a `Wait+Screen /…/` of on-screen text (not a wall-clock sleep), so the same
# beat is captured regardless of machine speed. See tools/gen_tui_shots.sh.

Set Shell "bash"
Set FontFamily "Menlo"
Set FontSize 20
Set Width 1500
Set Height 820
Set Padding 18
Set Margin 16
Set MarginFill "#0E0E14"
Set BorderRadius 10
Set WindowBar Colorful
Set WindowBarSize 36
Set TypingSpeed 28ms

Set Theme { "name": "Recollect Brass", "background": "#17171F", "foreground": "#F5F0E3", "cursor": "#F0C46A", "selection": "#3A3A52", "black": "#2B2B38", "red": "#A83D33", "green": "#7FB069", "yellow": "#D4A24E", "blue": "#5B8DD6", "magenta": "#C9655B", "cyan": "#5B8DD6", "white": "#F5F0E3", "brightBlack": "#6E6E84", "brightRed": "#C95A4E", "brightGreen": "#9FCF88", "brightYellow": "#F0C46A", "brightBlue": "#7FA8E0", "brightMagenta": "#D98A80", "brightCyan": "#8FB8EC", "brightWhite": "#FFFDF7" }
