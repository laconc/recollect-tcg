# RECOLLECT — DESIGN (v1)
**The complete current law. Card data in `app/crates/recollect-core/data/cards.toml` (card design in `docs/cards_design.md`) · the engine that implements this law in `architecture.md` + `docs/engine.md`.**

## 0. The language law: Banish

Recollect's rules never kill. A spirit defeated in an exchange is **banished from the match**: it Fades, and its impression falls in the **banisher's color**. *Banish* replaces kill, slay, destroy, and death in every rules text, card text, UI string, log line, and metric name (sims henceforth report *banishments per game*). *Defeat* stays as the neutral umbrella; *Fade*, *dissolve*, and *Parting* are unchanged; *strike* is contact, not death. The law binds hardest where players read — cards, UI, lore — and it buys the lore something for free: **banishment leaves an impression. The Memory keeps a mark of everything the Narrators contest. Unwriting leaves nothing.** What is banished is still remembered; what is Unwritten never was. The ill intent is not gentler under this law — it is lonelier.

**The banished linger one last turn — the standing-Faded window.** A spirit
banished in combat does **not** dissolve at once. It enters a **standing Faded
state**: still on its tile, Fading (it no longer acts, retaliates, or intercepts —
a fading spirit was never an actor), and it **persists until the end of its
owner's next turn**. That window exists for one reason: it is the owner's chance to
**Primal-evolve** the base — a Primal needs a base that is *Fading and still
standing* (see §5), and this window is what makes that state reachable in a Main phase.
**The timing is the crux.** The Faded base must survive **into the owner's Main** —
the dissolve fires at the **owner's turn-END**, in the **Fade phase that now sits AFTER
Main** (the turn runs **Flow → Main → Fade**), **not** before Main. A base banished on
the opponent's turn *N* is still standing-Faded when its owner's turn *N+1* Main begins;
if the owner does **not** evolve **or devolve** it that Main, it dissolves at the **end**
of turn *N+1* (in that turn's Fade, laying the banisher's impression as ever). A base
banished on the **owner's own** turn skips that turn's Fade and dissolves at the owner's
**next** turn-end (so it, too, gets a full Main to redeem it in) — the `fade_deadline`
gates this. **The final round is the exception**: on round 12 there is no next owner turn
to host a window — but the spirit still lingers standing-Faded through the rest of the
round rather than vanishing the instant it is defeated. It dissolves at the **end of round
12** — after every player has taken their final turn, in the Nightfall step **before
scoring** — laying the banisher's impression so the opponent scores the tile.
(Dissolving on defeat would rob the round of a body that should stand until the match
ends; dissolving *after* scoring would wrongly let the faded spirit, not the banisher's
impression, hold the tile.) **The Fade phase is now uniformly the turn-END dissolve of a
banished base** (one rule, one place); there is no turn-start fade. **The Dusk is
decoupled from Fade**: at the Curl (the round-8 contraction) it sweeps its rim — the empty
rim tiles darken and the **Unwritten on the now-dark rim dissolve IMMEDIATELY**, in that
same contraction step — **no window, no deferred fade**. (Only Unwritten are ever swept: a
player's standing rim spirit is HELD, never swept. The Unwritten leave **nothing** — no
body, no impression, no tally — the Solace-no-mark rule.) So every spirit that is Fading at
any observable moment is a *combat-banished* base inside its standing-Faded window (it
always carries a `fade_deadline`); the Dusk never leaves a fading body waiting.

## 1. The Arrival Law

**Combat is born from arrival. A spirit fights the instant it arrives — and only then.**

The four arrivals: **Placement** (play from hand to a legal tile; may engage one enemy in Reach), **Overwrite** (§2), a **Mobile step** (move 1 tile; may engage on arrival), and a **Lurker reveal** (§7 — the lie stands up; may engage from where it hid). Standing spirits never initiate; they **retaliate** when struck (no Reach required), they **intercept** (§3), their Reach **projects placement legality** (§4), and they hold scoring ground. Stillness is safety; arrival is violence; the hand is the ammunition.

**Engagement** resolves simultaneously and fully forecast: attacker's Attack (+10 wheel edge where the Resonance wheel grants it, +Echo branch if eligible) against defender's Defense; defender retaliates likewise. Reach ignores intervening occupancy — memory, not ballistics. **Momentum:** a defeat grants **one** bonus engagement against another enemy in Reach at +10 Attack, with retaliation; **Relentless** spirits chain while defeats continue. Strays have no Momentum — they never arrive and never engage; the wild only answers (§3). **Echo** resolves before any chain decision. Arcane applies to every strike a spirit makes — arrival, interception, retaliation — ignoring 20 Defense unless the target is Warded. **Combat is meant to bite:** the catalog is statted attack-forward and defense-light over a meatier HP pool, so a typical strike lands roughly a third of a spirit's HP — an average spirit falls in **~3 interactions**. Spirits trade and the board populates; stillness is safety, but no longer near-immunity. (Defenses now sit low enough that Arcane's −20 ignores nearly all of most targets — a watched edge, but Arcane is a tiny slice of the pool.)

## 2. Overwrite

Play a spirit from hand **onto a tile occupied by any spirit you don't control** — enemies and unclaimed Strays; never your own or a teammate's. Target must be **face-up** and **within your projection**. Pay full cost; resolve one simultaneous exchange. **Defeat the occupant → your spirit takes the tile** (the banisher's impression beneath it; Momentum may chain from there). **The occupant survives → your spirit dissolves, no impression**; the damage you dealt persists. The overwriter arrives at full HP and so never Echoes; the defender can — variance flows to the besieged. An Overwrite is an arrival: other interceptors may strike the newcomer after the exchange. Bait arithmetic — spending a body to bring a wall into banishing range — is intended play.

**Overwrite reaches a Stray** (it stands on its tile — the shimmer is *on the board*, even though the engine keeps an unclaimed Stray in its own slot). A **revealed** Stray (a face-up Gentle or Feral, or an unveiled Wary) is a legal Overwrite target and resolves by the rule above: one simultaneous exchange against the Stray's stats — defeat it and your spirit takes the cleared tile (a banished Stray lays the banisher's impression beneath it, §6); survive it and your spirit dissolves, the damage persisting on the wild. **A *hidden* thing is denied entry, not fought.** A **veiled (Wary) Stray — or any Stray not yet surfaced face-up to the overwriter — is not face-up**, and the face-up requirement is the whole of it: *you cannot Overwrite onto what you cannot see by name*. So an Overwrite aimed at a hidden Stray's tile **denies the hidden thing entry** — it simply **leaves: no impression, no reveal, no Echo, nothing** (it never *was* there to be fought — "denied entry, and it just disappears") — and **the overwriter then takes the cleared tile** as an uncontested arrival (no exchange; there was nothing face-up to contest). The denial **must not name the hidden thing**: it leaves without ever surfacing its identity, so the redaction holds — a veiled Stray's card never reaches the opponent's view (§2's face-up rule and the hidden-Stray redaction are one law seen twice).

## 3. Interception

A standing spirit's Reach is a live zone. When an enemy **arrives** on a tile within it, one covering spirit **may strike** the newcomer after its arrival-engage resolves: Attack vs Defense, no counter-retaliation. **Caps: one interception per arrival** — the defender chooses which spirit — **and each spirit may intercept once per round.** **Ordering: interception resolves after the arrival's engage and BEFORE any Momentum chain decision** — the zone bites the rampager mid-stride; interception is the designed brake on Momentum, and the chainer decides with whatever HP remains (its Echo now live if driven below half). **Interceptions never retaliate, never Echo-chain, and never grant Momentum** — they are answers, not assaults; chains belong to narrated violence. Attack buffs sharpen interceptions and retaliations alike. Hidden Lurkers do not intercept. **Feral Strays always intercept** — deterministically, reach-shaped (approach a Lance-toothed Feral from behind its teeth), once per round, and **in addition to** the defender's one interception: the wild does not coordinate with your enemy. Mid-chain strikes are not arrivals; the interception window for an arrival passes once. Forecasts stack the entire arrival — engage, interception, Echo branches, Promise redirections — in one panel.

## 4. Rooted Play & the Margin Rule

A spirit may be placed on any empty tile that is: **within the Reach of your spirits in play**, or **adjacent to your impressions**, or **adjacent to your Landmarks**, or **in your home two rows** (while they exist). Landmarks are placed under the same legality as spirits — authored terrain must be connected; only lies travel free. **Face-down cards — Fabrications and hidden Lurkers — project adjacency only**: a lie holds a little ground, and showing its reach would show its name. Legality is checked **only at arrival**; severed networks keep projecting. Banishments write new footholds — your impressions are expansion fuel; aggression self-roots. **Player one's first placement is restricted to her home two rows.** Overwrite legality follows projection. In 2v2, **placement** may use either teammate's projection; **Overwrite requires your own**.

**The Margin Rule:** a Narrator with no legal placement may place on any empty tile. The board always leaves a margin.

## 5. The Clock

**Twelve rounds — the Memory keeps a clock.** At the end of round 8 comes **the Dusk** — the Memory's light failing at its edges, the clock metaphor made terrain (the Dusk, then Nightfall). Every EMPTY rim tile goes dark; rim impressions lock and still score. **Occupied rim tiles are HELD** (the Held Ground law, all modes — **except the Solace's Unwritten, which the Dusk sweeps from the margin; the never-remembered are not kept**): the spirit stands, scores, may step away — the ground going dark behind it — and retaliates if struck, but it is held: its zone no longer intercepts, and its tile accepts **no new writing — no spirit may be Played or Overwritten onto it** (to play a card is to write it into the Memory; held ground is too thin to write on). *The Memory keeps what is loved, while it is loved.* **Readability is a law of its own:** the faded rim renders as night; a held tile renders **lamplit** — a small pool of light around the spirit that keeps it, visibly different from live board, with no reach-zone overlay (no zone, no overlay — what you don't see can't bite you), and the light goes out the moment the spirit leaves. One glance answers "why is that spirit out there and why didn't it intercept." The Dusk is telegraphed throughout round 8 (banner, darkening tiles, clock pips, one free "Dusk falls!" seal); rounds 9–12 are fought on the inner 3×3 plus whatever is still held, where the home rows no longer exist and the Margin Rule earns its keep. Round 12 is Nightfall; **round 11 is the last full round, and it is for saving** — the binding strip renders as a clock face. Turn structure — **Flow → Main → Fade**: **Flow** (draw; income = min(1+round, 6); at hand cap 8, **Release** — draw, then bottom one), **Main** — **no fixed action count**: **Play** and **Call** as far as your **Anima** reaches (Call still one living Kindred per caller); each **Mobile** spirit may **Move** once, *free* — that step is its arrival-engage — but never the turn it arrived (summoning sickness); **Glimpse** once — **burn a card of your choice from your hand as the cost, then see your top card and keep it or bottom it for +1 Anima** (the burn is what makes it a real decision, not a free every-turn action: you choose which hand card to spend, it leaves play, and you peek the top — then take the draw or bottom it for focus. Net: keep = −1 card for foresight; bottom = −2 cards for +1 Anima. Self-limiting — it costs a hand card and thins the 20-card deck, so it can't be every turn; you cannot Glimpse with an empty hand (nothing to burn) or an empty deck (nothing to peek)); and **Evolve** / **Devolve** (see below) — then **Fade** (at your turn-**END**, after Main): a base **banished in combat** that you did **not** evolve or devolve this Main dissolves now, laying the banisher's impression — the standing-Faded window closing (§0.5; a base banished on your OWN turn skips this Fade to the next full Main, gated by its `fade_deadline`). **The Dusk is not a Fade event** — it sweeps the rim **instantly** at the round-8 contraction (the empty rim darkens; the Unwritten on the dark rim dissolve at once, no window). Standing spirits still never *act* — they retaliate and intercept. Deck **20** (minimum 12 spirits; the deck-law caps of §20), mulligan once — **London-lite**: draw a fresh full hand, then **bottom one** (the bottomed card is the cost; deterministic from the seed; the opponent learns THAT you mulliganed — a public beat — never the cards; once per seat, in the opening window before turn 1; engine: `Command::Mulligan { seat }` → `Event::Mulliganed`, redacted in `PlayerView`). **Listener's Grace: removed** — the Listener keeps the name and the holstered tie-break lever. Scoring is Score: **one point per tile**, to whatever is last on it — the standing spirit if present, else the most-recent impression (a new banish overwrites the old; impressions don't stack and don't score under a covering spirit). Faded rim impressions lock and still score. The **Solace scores apart** — its Unwritten leave no marks; it banks an off-board **erasure tally** instead (§11). **Ties are draws** (ties-to-the-Listener stays a held lever; the Listener's last-action-each-round edge is documented and deliberate). The comeback channel sits near **~21%** and is invariant across the clock. 2v2 round count: **12** (same as 1v1), with the **full 2v2 shell** (TeamView HUD + hand), not board-only.


**Evolution — play the form card from hand onto its base (Main).** A spirit's
**Primal** and **Fabled** forms are **deck-playable cards** — they sit in the
deck and are drawn to hand like any spirit. You evolve by **playing the form card
from your hand onto its matching base during Main**: a **Primal** form onto one of
your **Fading** bases (its last becoming, the fade the fuel) — including a base
**banished** last turn that now stands Faded in your Main under the standing-Faded
window (§0.5), which is *the* Primal opportunity — a **Fabled** form
onto one of your **healthy** bases (donor-fueled, the turn after the base arrived).
**A base evolves to a Primal *or* a Fabled — a Primal cannot evolve to a Fabled.** Both
forms branch from the *base* (base→Primal and base→Fabled, never base→Primal→Fabled): a
Primal is a rescued, deepened body; a Fabled, a healthy body's ascension. The only path
from a Primal to a Fabled is to *recede* it to a base first (Devolution, below) and
evolve that base anew.
The form **costs the form's cost minus half the base's** Anima, arrives
at **full HP**, and leaves **no impression** — the spirit *became*, it did not die.
Evolution **fuels Parting** on the base it consumes, and follows the shared-Imprint
rule (RuleException carriers excepted). **You cannot evolve a base you have no form
card in hand for** — the becoming is a card you must hold and pay, not a free
in-place transform. **Deck-construction constraint — no orphan evolutions:** if a
**form** card is in a deck, **at least one of its base forms must be too** (you can
never draw a form you can never land). Deck generation — the five Quick Play styles
and the character decks alike — is **aware of the base↔form pairing** and never
produces an orphaned form.

**Devolution — recede a banished form to a base (the rescue).** A Primal or Fabled
form banished in combat enters its standing-Faded window (§0.5). Instead of letting it
dissolve — or evolving it further — its owner may **devolve** it: play a **base card
from hand** that lies in the form's line onto the faded form, during Main, before the
turn-end dissolve. The base **arrives at full HP**, its fade cleared — rescued, one tier
down. Devolution **costs half the banished form's Anima** (rounded down), and the
rescued base is **summoning-sick** until the owner's next turn. **Devolution IS an
arrival — symmetric with evolution** (the maintainer's ruling: *if evolutions are
arrivals, devolutions should be too*): the recede fires the same **arrival triggers** a
form's evolution fires — chiefly **the Throughline can complete on the devolve** (a base
that recedes directly into a standing 3-line re-completes on the spot: +10/+10 and a full
heal, at parity with a Primal-evolve into a line), and a queued **next-arrival** buff
(Kindle/Again!) lands on the rescued base. What stays **unchanged**: devolution still
**engages no one** (the recede carries no strike target — it never initiates combat) and
fires **no OnPlay**, and the base is still **summoning-sick** (no free Mobile step the
turn it devolves — exactly as an evolved form is). A spirit may cycle evolve↔devolve
without limit, bounded only by the forms and bases in hand: base → Primal → *(banished)*
→ recede to base → Fabled → *(banished)* → recede to base → … The Lorekeeper **reverts**;
the Solace **recedes** — one engine action, the faction's verb in text, UI, and log.

**The Solace deepens (Primal only).** The Solace's Unwritten carry **Primal forms —
never Fabled**. Where a Lorekeeper spirit evolves toward legend, an Unwritten
**deepens**: the erasure sharpens, but has no apex to climb to. A Solace line is two
stages (base → Primal); its cycle base ⇄ Primal recurs but never transcends — erasure
has no summit, only return. A Deepening is not always crueler: the Solace is *comfort
that devours*, so its forms span a **gentle-to-malign** range — some lean into mercy (a
softer oblivion, a sorrowful letting-go), some into appetite (a deliberate scouring).
The gentleness is the seduction, not a softening of the threat.

**Who opens the match.** First player is a **seeded coin-flip**, not a fixed seat — decided at genesis from the match seed (deterministic, replay-verified like everything else) and announced as a match-start beat. When a **bot character** is in the match, its **initiative** trait *weights* the flip toward opening — an **edge, not a guarantee** (the randomness stays); an all-human match is a pure 50/50. The opener is **Player one** (the home-rows first placement, §4); the other is **the Listener** (the last-action-each-round edge + the held tie-break lever). Applies to every mode — Quick Play, 1v1, and **2v2, where any of the four seats can open**: a 4-way seeded pick, with the `A1→B1→A2→B2` cycle rotating to begin at the chosen opener (still team-alternating), and each team's character initiative weighting its own seats' odds. Going-second compensation stays a **held lever**: measure the first-player win-rate in the sims, add it only if the data shows imbalance.

### Adopted rulings — the deck-playable effects
Four points the spec left underspecified, decided so the deck-playable effects work
(each gates a handful of cards):

1. **Fade reclaim (amount + agency).** **Reclaim** is the controller's *voluntary
   choice*: during your **Main**, you may **Reclaim** one of your own **standing**
   spirits — it leaves (no impression), its Parting fires, and you regain **⌊cost ⁄ 2⌋
   Anima**. A spirit *banished* by the foe is **not** reclaimed — it dissolves as
   ever. **Last-Light Koi** reclaims its **full** cost instead of half; **Ferrier of
   the Salt Road** adds **+1** to each of your reclaims (follow-on).
   *(Implementation note: this realizes the rule as a voluntary Main action on a
   STANDING spirit rather than a Fade-step choice on a Fading one — the engine never
   produces a non-banished fade, so the Fade-step framing had no eligible target; the
   "voluntary fade" is the controller choosing to cash a spirit in. The economy is
   unchanged: ⌊cost/2⌋ back, no impression, Koi full.)*
2. **Calm tiles.** An Unwriting may render a tile **calm** for a set number of rounds
   (round-scoped). While calm, a tile **accepts no new writing** (no Play or Overwrite
   onto it) but **still scores** and a standing spirit **keeps holding** it — distinct
   from a *faded* tile (permanent Dusk-dark). **The Quiet Spreads** calms two inner
   tiles the following round.
3. **"Look at" a Fabrication = private knowledge.** "Look at a face-down Fabrication"
   grants the looker **private knowledge** of that lie's identity: recorded per-player
   and surfaced only in **that** player's `PlayerView` (a `seen` mark on the face-down
   enemy terrain), never flipped face-up and never leaked to anyone else. This is
   distinct from a public **Reveal** (which turns it face-up for all). **Curio Fox**,
   **What's That?** read this way.
4. **Throughline completion.** A **Throughline completes** when a connected line of
   **3 or more of your spirits sharing one Imprint** forms (orthogonally adjacent;
   **Steadfast** anchors a link against displacement; **Unbreakable** bridges a single
   1-tile gap; a piece that "counts as every Imprint" joins any line). On completion the
   completing spirit gains **+10/+10 and a full HP restore**; "on Throughline complete"
   riders fire then (**Vale Eternal**: draw 2, gain 2 Anima). **Queen of the Quiet
   Garden** grants your Throughlines an extra +10/+10. The buff is **once per spirit
   body** — a spirit that has completed carries `throughline_done` and will not
   re-trigger. **The lifecycle of that flag across the becoming cycle (the maintainer's
   ruling):**
   - **Fading breaks it.** When a spirit becomes Fading (HP reaches 0 and it is banished
     into the standing-Faded window, §0.5), `throughline_done` **resets to false** — a
     rescued or re-formed body may earn the Throughline anew.
   - **A Primal form is re-completable.** Evolving to a **Primal** does **not** inherit
     `done`: the form arrives with `throughline_done = false` and may complete the
     Throughline afresh. (A Primal evolves from a *Fading* base, so the fade has already
     broken the flag — the form re-completing is the consistent outcome.)
   - **A Fabled form keeps it.** Evolving to a **Fabled** **inherits** the base's flag —
     if the healthy base had completed, the Fabled arrives already `done` and cannot
     re-trigger. (A Fabled evolves from a *healthy* base, a continuation of the same
     story: the buff carries forward, locked.)
   - **Devolution is re-completable, and completes ON THE DEVOLVE.** Receding a
     standing-Faded form to its base (§5 Devolution) resets `throughline_done = false`
     ("a fresh base earns its Throughline anew"). Devolution only happens while the form
     is Fading, so the fade already broke the flag; the reset is made explicit on the
     base for consistency. **And because devolution is an arrival (§5), the reset base
     re-completes immediately if it recedes into a standing 3-line** — the +10/+10 and
     full heal land on the devolve itself, at parity with a Primal-evolve into a line
     (the recede runs `check_throughline` exactly as evolution does).

   So the lifecycle is **asymmetric by tier, and the asymmetry is intended**: **Fabled
   keeps**; **Primal, devolution, and fading all reset** (re-completable).

## 6. Strays & the Foundlings

About **one match in seven** (seeded, published rate, soft pity cap of 12), the Memory stirs: a shimmer telegraphs **an empty inner tile** one round ahead — *location, never identity* — and at the start of either player's turn (seeded 50/50; the side-split is a tuning lever), a **Stray** surfaces in the round 2–6 window. **Surfacing is not an arrival** — the Memory is remembering, not attacking: no engage, no interception against the newcomer; player zones do not strike the appearing dog. **Wary Strays surface veiled** — the board shows a veiled stray that occupies, scores for no one, cannot intercept, and cannot be engaged; ending a turn adjacent unveils it (and that same adjacency counts as the first of its two courtship turns), or it unveils itself after two rounds of being almost-seen. A veiled Wary is **not face-up**, so it cannot be *fought* by an Overwrite either — but an Overwrite aimed at its tile **denies it entry**: the hidden thing leaves with no impression and no reveal, and the overwriter takes the cleared tile (§2 — the denial never names it, so the veil's redaction holds). Gentle and Feral surface open — the gentle want to be seen, and the dangerous *must* be (forecast integrity is non-negotiable; fog may hide identity, never incoming damage). **The wild keeps to the heart of the board:** Strays surface on **inner tiles only** — the Dusk never reaches them, so no wild thing is ever stranded, stepped, or confiscated by the clock. The telegraph names the tile; **if that tile is occupied when the surfacing turn arrives, the Stray does not come** — the clearing was filled, and the wild stays away. Occupying the shimmer is legal counterplay: deny your opponent a befriending, or leave the clearing open and welcome it — the choice is part of the game. **If no inner tile is empty at telegraph time, there is no surfacing** that window. A cancelled surfacing counts as a Stray-less match for pity purposes — denial never starves the collection long-term. A **befriended** Stray is yours thereafter and stands under the Held Ground law like any spirit. **The Midnight Stray:** in a match that already had a Stray, a published **10%** chance (seeded) that a second surfaces at the start of round 11 — the last full round — telegraphed at round 10 on an empty inner tile, **under the same law — occupied at round 11, and it does not come; no empty inner tile, no telegraph**; weighted toward the Gentle. About one match in seventy overall — rare enough to be a story, common enough to be believed. Some memories only surface at the very end. Strays are Resonance-less, score for no one while unclaimed, and offer the choice: **banish** (a normal banisher's impression — yes, even one at Echo; the Archive ledger remembers) or **befriend** — and a revealed Stray may also be **Overwritten** (§2: the shimmer is on the board, so an Overwrite reaches it; a defeated Stray lays the overwriter's impression beneath, exactly as a banish does). Temperaments: **Gentle** (end your turn adjacent with a shared Imprint), **Wary** (two consecutive turns — it's counting), **Feral** (it intercepts arrivals; befriendable only while it bears an Echo — below half, its fear cracks and it can recognize you). A first befriending adds the Foundling to your collection permanently — never craftable, never purchasable, and **every Foundling lists its haunt** (a story chapter or Daily rotation where it reliably appears): serendipity in PvP, certainty in solo. The 27 Foundlings — three temperaments × nine, three legends with printed rules (Home befriends the trailing Narrator; Hundredname needs nine turns of adjacency, at most three granted per encounter, persisting across matches as server state; Ashmane demands Echo plus two shared Imprints) — live in the cards doc §3.7/§5.9.

## 7. Keywords & rulings (the audit, consolidated)

- **Arcane** (−10) — pierces 20 Defense on every strike unless the target is Warded.
- **Warded** (−15) — immune to Arcane's pierce **and untargetable by enemy Rituals**.
- **Mobile** (−10) — move 1 tile, **free, once per turn** (but not the turn it arrived — summoning sickness); the step is an arrival: engage on arrival, eat interceptions. Re-aims projection, escapes the rim, courts Strays.
- **Steadfast** (−10) — cannot be forcibly displaced: pushes, pulls, and swaps fail. Anchors projection, interception zones, Throughline links, Bonds (pushes break Bonds), and your tile against round-7 rim shoves.
- **Parting** — triggers on every full dissolve, **including as evolution fuel**. Anti-Overwrite texture: the opponent picks when you die, and Parting bills them for it.
- **Lurk** (−10) — may be played face-down; **while face-down it is a Fabrication for all rules**: adjacency projection only, scores for no one, Overwrite-immune, never intercepts; at the Dusk a face-down Lurker on the rim is held like any occupant — something stands there, even unsaid. **Revealing is an arrival** (an action; may engage; may be intercepted).
- **Momentum / Relentless** — a defeat grants one bonus engagement at +10 Attack; Relentless chains while defeats continue. Embermane's +20 links and Sparkfather's first-engage bonus apply on top.
- **Unbreakable** — the pair counts as adjacent while Bonded and survives 1-tile separation: the only Throughline gap-bridge.
- Rulings batch: **Landmarks project adjacency** · **Calls are arrivals** (Kindred may engage on arrival and may be intercepted) · **Solace shifts are arrivals** (your zones bite the advancing erasure — the Memory resists) · **Attack buffs apply to all strikes** (so Round, Symphony, and Surprise Party reach interceptions and retaliations, not just arrival) · **Promise vs Overwrite**: redirected lethal means the defender survived — the overwriter dissolves, fully telegraphed. The interception mechanic's name is plain **Interception** (Vigil Owl is just an owl).

### Resolved rulings — the red-team's edges, now decided
Three edges the red-team surfaced where the **current engine behavior was settled and
tested**, but the *design intent* was the maintainer's to confirm. All three are now
ruled; recorded here so the choices stay explicit.

1. **Terrain alone on an empty rim tile at the Dusk — swept.** When the Dusk darkens
   the rim (§5), a tile holding **only a Landmark or a face-down Fabrication** (no
   spirit) is **swept** — the tile goes dark and the terrain dissolves with the ground
   it sat on (Held Ground holds a standing *spirit*'s tile, not bare infrastructure).
   *Ruled: stays swept (the maintainer's call); bare rim terrain is not held.*
2. **The Throughline-completion lifecycle — ruled, see §5.4.** Fabled keeps the flag;
   Primal evolution, devolution, and fading all reset it (re-completable). The
   formerly-flagged evolve/devolve asymmetry is folded into the full lifecycle.
3. **Overwrite onto an unclaimed Stray — Overwrite reaches it.** A Stray is on its
   tile, so the §2 prose stands: a **revealed** Stray resolves a full Overwrite exchange
   (defeat → the overwriter takes the cleared tile, the Stray's banisher-impression
   beneath; survive → the overwriter dissolves, the damage persisting); a **hidden** Stray
   (a veiled Wary, or any Stray not face-up to the overwriter) is **denied entry** — it
   leaves with no impression and no reveal, and the overwriter takes the cleared tile as an
   uncontested arrival (§2). The hidden-denial never names the Stray, so the veil's
   redaction holds. (Engine: `decide_overwrite` reads `state.stray`; guarded by
   `tests/suites/strays.rs`.)

## 8. Variance doctrine

**Seeded, visible, and merciful** — policy: every probability in the game is published in-client (Echo 20%/+20, Stray ~1/7, pity cap, befriend conditions). Echo's roll resolves at impact, never in the preview; the seed stays server-side until the post-match reveal, so the surprise is cryptographically real and the fairness is provable afterward. Randomness only ever activates for the side already losing. Purchases are 0% random, forever.

## 9. Modes & AI

**Solo** — six Resonance story arcs into the Solace finale; Skirmish vs any brain (choose your opponent faction — other Lorekeepers, or the Solace); the Daily Memory (and Foundling haunts). **1v1** — live and async; ranked at L10. **2v2** — 6×6, A1→B1→A2→B2, each turn Anima-gated like 1v1, cross-team Bonds, shared Throughlines and score, the co-op default of two humans against the Solace's pair; round count and the second-team grace are the open tuning items. **AI** — four difficulty tiers (Easy / Normal / Hard / Expert; Normal is the default), one agent scaled by softmax temperature + lookahead depth (calibrated monotonic ladder; see decisions/bot_and_ml_plan.md); the learned policy net swaps in behind the same `choose` seam later. The Solace plays its own faction deck — Unwritten that persist, Unwriting events that erase — and targets impressions: your points, and now your footholds.

## 10. Kindred

Six Uncommon callers, six Kindred (cards §3.3): *Call — action, 2 Anima, adjacent empty tile; one at a time; fades at the caller's departure.* Calls are arrivals. Kindred occupy and score while alive, dissolve to no impression, can't evolve, may be Bonded. Companionship as a mechanic; the tether line renders.

## 11. The Solace, the Unwritten, and the ill intent

The antagonist is three nested layers, each with its own motive (see docs/decisions/naming.md for the full lore).

**The Solace** (formal: *the Solace of the Lethe*; daily: *the Solace*) — the antagonist *faction*: an order of **people** who believe erasure is **mercy**. A fading Memory, to them, is suffering; the kindness is to let it go completely, into a forgetting that cannot ache. They are sympathetic — founded by former Lorekeepers who came to accept the mercy, and they take in both knowing converts and innocents who never knew otherwise. The Solace does not fight directly; it **calls the Unwritten** to do the releasing, and its **Unwriting** work (events that eat impressions and tiles) carries its sorrowful, gentle register.

**The Unwritten** — the Solace's *creatures*: things **made of everything that was never remembered**. Mostly moved by **envy/grief** (they were never remembered; an impression is the wound they never got to have) — wistful and unfinished, not cruel. Their defining rule: **the Unwritten leave nothing.** What an Unwritten banishes or erases is simply *gone* — no scar, no mark falls where your spirit or memory stood ("what is Unwritten never was"). They do not *claim* the Memory; they *empty* it. Yet erasure is their **win condition**, so the act itself scores: the Solace keeps a running **erasure tally** — each player spirit it banishes, each impression it unwrites, and each foothold it denies (a spirit dies in a Lacuna's reach and no mark forms) is **+1, banked and permanent** (you cannot un-erase it — you can only *prevent* it). They **persist until banished** and win by their *standing* Unwritten **plus** that tally, against the player's Score at Nightfall. (A player who banishes an Unwritten gets nothing — it dissolves leaving no mark of having been; combat is how you *remove* them, not score off them.)

**The ill intent** — a **sinister subset of the Unwritten** whose envy curdled into spite and hunger: not "I wish I'd been remembered" but "I will unmake you for having been." They keep the old menacing register (Redacted Bear, Page-Eater — "it's not empty, it's hungry"). The tragic irony: the merciful Solace cannot fully control what it calls — reaching for the Unwritten, it draws the ill intent too. PvE's real teeth live here.

PvE structure: six Resonance story arcs into the **Solace finale**; the antagonist set is **92 cards (41 Unwritten + 39 ill intent + 12 Unwriting events)** — ~20 distinct PvE characters; see §5.8. **Lacuna** denies new impressions and therefore footholds — and a foothold denied where a spirit has just died is itself an erasure, so it banks +1; **Page-Eater** eats projection with the impression. Their advances trigger your interceptions.

**The Solace as a live opponent — it plays to win through combat.** Every seat is uniform — `{faction, deck, controller}` — so the Solace is **just another faction**, bot-piloted: it draws a singleton Solace deck (Unwritten/IllIntent creatures + Unwriting events) into a hand, spends anima, and plays through the same decide/evolve + journal path as any player. Its Unwritten **persist until banished** (the orphan-sweep exempts them — combat is the point), and the Solace **scores asymmetrically**: standing Unwritten **plus an off-board erasure tally** — each player spirit it banishes and each impression it unwrites is **+1, banked and permanent**, while its Unwritten **leave nothing** on the board. The player's Score stays board-derived (occupied + impressions, one per tile), with the **Dusk** as the counter: contraction sweeps Unwritten from the fading margin (the never-remembered taken first) while the player's spirits hold their ground — clawing back the Solace's *standing*, though never its banked tally. The bot pilots it at the player's chosen **difficulty tier** (skill: softmax temperature + lookahead — the same agent that plays Lorekeeper), and **win-rate sims** keep the fight fair (`solace_winnability`: the player wins a fair share at Hard). The PvE-variety layer: **20 alignment-themed Solace characters** (IllIntent-heavy reads cruel; Unwritten-heavy, erasing — four per disposition: Cruelty, Erasure, Relentless, The Long Forgetting, Sorrow), the seeded **first-player coin-flip** (the character's initiative weights the toss), and the **2v2 Solace pair** on the 6×6 board, all wired into matchmaking. The Solace stays asymmetric — it unwrites and its own dissolutions leave nothing — but it is a faction that can *win*.

**The Lorekeeper characters.** The player's own house fields named opponents, allies, and mirrors too: **20 Lorekeeper characters**, four per Quick Play style (Embertide, The Long Watch, Mistwalk, The Choir, Drifter's Bindle), drawn from the house's disciplines — Remembrancers who keep the fierce memories, Archivists and wardens who hold the night watch, Dreamers who walk the half-charted Memories, the Choir who keep the shared griefs, and the wandering keepers of the road. The model is the exact twin of the Solace character (a lore name + an archetype lean + a seed-salted deck-bias over the 294-card Lorekeeper pool — the deck-playable spirits, callers, spellbook, and evolution forms), so a bot Lorekeeper seat — a 1v1 opponent, a 2v2 ally or rival, or the mirror across the table — fields a varied, on-theme deck rather than a bare Quick Play style. Each character's deck is **pure in (style, roster-index, match-seed)** and re-derived on replay like every other deck (determinism + redaction unchanged); the salt keeps even same-style characters meeting as distinct decks. They are faced, never collected.

## 12. Communication: Seals & pings

No free text anywhere in v1. Players speak in **Seals** — hanko ink-stamps pressed onto the shared board. Starter eight: *Well met · Well told · Thinking… · My mistake · Beautiful · Thank you · Good luck · Again soon.* Governing law: **every Seal must read kind even when spammed** — no taunts exist to corrupt; rate limit ~8s; one-tap **Quiet mode** mute, undetectable. More Seals earn through Attunement and the Chronicle, **including stamps of your befriended Foundlings**; every Seal carries a screen-reader label. 2v2 strangers get **board pings** (a brush circle: here / danger / mine) and the **intent-ghost** — your hovered placement shown faintly to your teammate. Matches end with both players stamping the **Souvenir**.

## 13. Components

The Card (cost seal, A/D/H triad, reach mini-grid, frame treatments: Primal splash / Fabled gilt / Kindred / ill-intent eaten / **Foundling: a frame with a worn collar-tag**); the Memory (paper-panel board, binding-strip round counter rendered as a clock face, the rim darkening at the Dusk, held tiles lamplit); **the projection washes** — two inks contesting the board, the overlap reading as the front line; **the arrival Forecast** — engage, interception, Echo branches, Promise redirections, Madrigal-class hidden auras shown as "−10 (unseen)", one panel, one tap; Anima motes; markers (Echo ripple, Throughline thread, Kindred tether, fading shimmer, **Stray shimmer**, team trims); the deckbuilder's **growth-shape preview**; the Souvenir with seed hash.

## 14. Progression & economy

Systems gate by Narrator Level (co-op at **L1**, deliberately; Rituals L2; Bonds & Landmarks L4; Fabrications L6; Kindred L8; ranked L10/L12); **cards never power-gate**. Attunement serializes each card's lore into earned chapters, alt-art, animated ink. Foundlings join only by befriending. Glints, weekly purchase caps, cosmetics-only Chronicle, losses take nothing, all odds published.

## 15. Bluff economics

A Fabrication must be at-or-above rate on both branches, and face-downs are **infrastructure** (adjacency projection), so a forward lie is a bridge until probed. The truest cost stands: a bluffed tile scores for no one.

## 16. Living Ink

The nine commitments — ink is the interface, paper is the world, motion is water, color is meaning, brush type, paper-and-water sound (the Solace is the absence of room tone — the Unwritten make no sound, because nothing that never was can), haptic grammar, diegetic-plus-numeral stats, one-hand portrait — with the projection washes and the Stray shimmer as first-class citizens of the visual language.

## 17. Levers (current)

The dials, held until evidence (mostly the policy-net milestone) moves them: Echo
odds/magnitude · interception caps · Stray rate, pity, and side-split ·
ties-to-Listener (the tie lever stays holstered) · Steadfast/Mobile/Warded taxes ·
deck size 20→22 · 2v2 round count and grace · the befriend-priority window ·
evolution full-HP arrival (held permanent). **Combat weight — the catalog stat
curve** is itself a lever: attack-forward / defense-light / meatier HP so net
`attack − defense ≈ HP/3` (~3 hits to fall an average spirit), applied via
`tools/restat.py` scales and a least-squares budget fit in `gen_catalog.py` that
derives evolution-form costs. Sim-tunable: re-stat + `cargo run -p recollect-bot
--bin rebalance`.

## 20. Quick Play (mode spec)

One tap from the title screen, against the Keeper's bot (always disclosed) or a friend. There are **five seeded deck styles** — *Embertide* (all teeth), *The Long Watch* (hold and outlast), *Mistwalk* (arrive sideways), *The Choir* (stand together), *Drifter's Bindle* (honest chaos) — of which the player is offered **three** each match (a seeded shuffle picks three of the five; `quickplay::offer`); she picks one, and the deck is **derived, not dealt**: a pure function of (style, seed, catalog) that always satisfies deck law (20 cards, copy cap, guaranteed opening curve of ≥8 cards at Cost ≤2). Because generation is deterministic, the server re-derives Quick Play decks during journal verification, offline Quick Play rides seed tickets like everything else, and the vs-AI opponent is the replayable policy from `docs/decisions/bot_and_ml_plan.md`. The generator lives in `recollect-core::quickplay`.

## 21. Async play: Standing Orders and preference lists

No mid-turn windows, for anyone, ever — the rule that keeps correspondence play and live play one game. **Interception choice = Standing Orders:** each spirit carries an order, set freely during your own turn — *Watch* (intercept; engine picks its best strike) or *Hold* (let arrivals pass) — applied during the opponent's turn. Defaults to Watch. Thematically: instructions left with the sentry. **Chain targeting = preference lists:** an arrival command may carry an ordered list of chain targets; each Momentum link takes the first legal entry. Deterministic, journal-clean, async-safe. Live matches may later add a real-time veneer that *edits* these mid-animation, but the journaled mechanism is orders and lists — the engine never waits on a human mid-resolution.

**Absence & abandonment.** v1 ships **live matches only** — no correspondence mode — but the engine never knows that: mode lives in deployment configuration, and what the engine sees is a **system-kind command**. The Connection machine detects continuous absence (heartbeats; reconnects and app-backgrounding reset cleanly); the present player sees a banner and countdown; at **120 seconds** the platform issues `MatchAbandoned { seat }`, and the engine resolves it like any command — journaled, so a spectator replaying the match sees how it ended. The result records as an **abandonment win**, distinct from a Score win in the event payload: the ledger never lies about how a match ended. Rating treats abandonment as a loss; repeat offenders earn queue cooldowns (platform concern, outside the journal). **Keeper takeover is rejected for ranked** — an agent playing a human's seat corrupts both the rating ledger and the AI-authority story (server-verified rewards depend on knowing which policy played); it may return someday as opt-in casual continuation. Round-aware grace is a recorded lever, deliberately unused in v1: one rule, one timer.

**Choices under the no-windows law.** Three shapes,
each matched to whose turn it is: **(1) Own-turn choices are pending phases**
— Glimpse and target-picks open a `PendingChoice` for the acting Narrator
(the PendingRelease pattern); no opponent window opens, so the async law is
untouched. Peeked cards are redacted from the opponent's view. **(2) Parting
choices resolve by doctrine** — the dying don't deliberate: restores find
the most wounded eligible ally; buffs find the highest Attack; pushes flee
the dissolving teller. Deterministic, async-safe, flavorful. **Restore is a
pure heal** — it raises HP only and never pulls a spirit back from fading; a
heal may LAND on a fading ally (raising its HP before it dissolves) but cannot
rescue it. The dissolving stay dissolving. **(3)
Interception is Standing Orders** — Watch by default; a free `SetOrders`
action marks spirits Held (they never intercept) and stands until changed.
Momentum chain preference lists ride the arrival command (the engine auto-chain is
deterministic and async-safe; preferences are agency, not correctness). If a
correspondence mode ever ships, the same command takes a deadline-based timer source —
the command is the seam, which is why this is cheap now and miserable later.

*The Memory keeps every match — and now it keeps the dog, too.*
