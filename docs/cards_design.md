# THE CARDS OF RECOLLECT — design & exemplars
**The card DESIGN companion: the template, the set architecture, the 38 fully-realized exemplars (the bar), and the vocabulary/naming law. Stats in Attack / Defense / HP · pairs with the rules law in `design.md`.**

> **The per-card DATA is no longer here.** Every card's stats, rules, keywords, effects,
> evolution lines, lore, and physical now live in **`app/crates/recollect-core/data/cards.toml`**
> — the single source of card truth, from which `tools/gen_catalog.py` (`make catalog`)
> regenerates `catalog.json` + the runtime side-data. This document keeps the design rationale:
> the template every card fills, the 419-set architecture, the exemplars that set the quality bar,
> and the naming/vocabulary law. To CHANGE a card, edit its `[[card]]` block in `cards.toml` and
> run `make catalog`; never hand-edit the generated JSON.

---
## 1. The Card Template

Every card ships with all of the following. Nothing optional.

| Field | Purpose |
|---|---|
| Name · Resonance · Rarity · Cost | Identity line |
| **A/D/H** + Reach + Imprints + Rules | The whole game state of the card |
| **Physical** | Written *for the artist*: silhouette first, one signature detail, how its ink behaves, a scale reference |
| **Lore** | Long, fun, voiced. Dialogue beats description. Serialized into 3 chapters unlocked by Attunement (play-XP) — chapter 1 here; chapters 2–3 in the content pipeline |

### Voice principles (the attachment engine)
1. **Every card is somebody.** Even a 1-cost common has a want, a habit, and a voice. Players bond with characters, not stat-lines.
2. **Dialogue over description.** Overheard conversations, field notes, the card talking to itself. Quotes are the fastest route to personality.
3. **Recurring narrators stitch the world together:** *Archivist Pell* (earnest junior keeper, easily moved), *Dreamer Juno* (margin-scribbling chaos), *the Ferrier* (solemn, kind), and anonymous children's rhymes. Meeting Pell on your fifth card makes the world feel inhabited.
4. **Concrete nouns. Humor welcome at every rarity.** Melancholy must be earned, never default.
5. **Names:** spirits mostly keep evocative species-style names (the "-ling" family is beloved); Uncommon and above may take personal names; Legendaries follow the *"Zenith, Who Asks the Sky"* pattern — Name, comma, a clause you want to read twice.

---

## 2. Set Architecture — the 419

| Type | Count | Composition |
|---|---|---|
| Spirits (base) | **114** | 16 per Resonance (slot 16 = Kindred-caller; Fear runs to 18) + 6 Remnants + 3 Paradox Legendaries + 6 curve-fill evolution bases |
| Evolution forms | **60** | 48 Lorekeeper (4 lines × 2 forms × 6 Resonances) + 12 Solace **Primal Deepenings** (8 seed + 4 gentle-or-malign menu partners; the Solace deepens, never ascends) |
| Kindred | **6** | One per Resonance, summoned by callers; not deck cards |
| Rituals | **42** | 6 per Resonance + 6 neutral |
| Bonds | **24** | 3 per Resonance + 6 neutral |
| Landmarks | **24** | 2 attuned per Resonance + 12 neutral |
| Fabrications | **30** | 4 per Resonance + 6 neutral |
| The Solace (PvE set) | **92** | 41 Unwritten + 39 ill intent (the sinister subset) + 12 Unwriting events; faced, not collected (the 12 Deepening forms above are counted under Evolution) |
| Foundlings (Strays) | **27** | 3 temperaments × 9 (5 C, 2 U, 1 R, 1 L); collectible only by befriending |
| **Total designed** | **419** | |

**Collectibles: 261** (419 − 60 evo forms − 6 Kindred − 92 Solace; the 12 Solace Deepenings raised the total and the evolutions together, so collectibles are unchanged — forms are never collectible). Rarity pyramid: **C 115 / U 86 / R 42 / L 18** — the tables are canon (regenerate via `make catalog`; the totals fall out of the catalog itself).
**The eighteen Legendaries:** Zenith · Madrigal · Vesper · Rondel · Ignis · Adamant · the three Paradox spirits (Vertigo, Saudade, Crucible) · The First Forgotten · The Library Remembers · The Old Friendship · The Confluence · The Perfect Lie · The Blank Page · and the three Foundling legends: Home, Who Was a Dog Once · Hundredname, Who Has Been Watching · Ashmane, Who Outran the Ill Intent.

**Stat budget:** combat BITES — the curve is **attack-forward, defense-light, over a meatier HP pool**, so net `attack − defense ≈ HP/3` and an average spirit falls in ~3 interactions. The per-card A/D/H triads in `cards.toml` are the catalog's source of truth; `gen_catalog.py` fits the budget (`sum ≈ K·Cost + B`) to price the evolution forms. Trait taxes scale with the curve (Arcane −, Warded −, Mobile −, strong trait −…). Reach taxes (the reach does triple duty — arrival targeting, standing interception, placement projection): Burst −30, Spire −10, Legendary signatures −20. Resonance edge = **+10 Attack**. Arcane ignores **20 Defense**. Momentum = **+10 Attack per chain link**. Echo = **20% chance of +20 damage** while at or below half HP, odds always shown. Throughline = **+10/+10** and full HP restore.

---

## 3. Fully Realized Cards (38) — the bar

the full set's lore + physical live in `cards.toml`.

### 3.1 WONDER — the complete sixteen

**Dawnling** · Wonder · C · 1 — A15 / D10 / H40 · Cross · Storm
*Physical:* A palm-sized chick of pale-gold wash, perpetually mid-yawn. Its downy edges never fully dry, so it leaves little sunrise smudges where it waddles. Eyes: two brave dots. Scale: fits in a cupped hand, barely.
*Lore:*
"Is it morning?"
"Not yet."
"Is it morning *now*?"
"…Yes. Fine. It's morning."
— the complete transcript of every conversation ever held with a Dawnling. (Archivist Pell has stopped logging them. The sun has not stopped being argued up.)

**Moth of Small Hours** · Wonder · C · 1 — A15 / D10 / H25 · Cross · Wanderer · Mobile
*Physical:* A grey-violet moth whose wings are two torn calendar pages, dates illegible. It sheds a fine dust of 3 a.m. quiet. Antennae droop like someone politely not asking why you're still awake.
*Lore:* It only visits the lonely-awake, and it never stays. "It landed on my lamp at 3:14," writes Dreamer Juno, "and we agreed not to mention it. Best conversation I've had all year."

**Wrong-Way Skylark** · Wonder · C · 2 — A45 / D10 / H40 · Wide · Wanderer
*Physical:* A streak of confident teal with its head turned the opposite direction of travel. Trails a long, optimistic contrail of unmixed ink. Always rendered mid-bank, always banking incorrectly.
*Lore:*
"South is *that* way."
"Wonderful! And what's THIS way?"
"…Nobody knows."
"*Wonderful.*"
— it has discovered eleven places that were not previously anywhere. The keepers have given up correcting it and started following it.

**Cloudling** · Wonder · C · 2 — A30 / D10 / H50 · Cross · Storm · *Glimpse — on arrival: look at your top 2 cards, take 1.* ◆ Evolution line D
*Physical:* A child-sized cumulus with stubby arms, carried everywhere by a wind only it can feel. Its underside darkens when it concentrates. Holds exactly one (1) raindrop, saved for a special occasion.
*Lore:*
"When I'm big, I'm going to rain on EVERYTHING."
"Even me?"
"*Especially* you. Lovingly."
— it practices thundering at night, very quietly, so as not to wake the sky.

**Strider of the Far Blue** · Wonder · C · 3 — A45 / D20 / H65 · Cross · Wanderer
*Physical:* A long-legged wading bird, neck like a question that keeps extending. Its legs vanish into watercolor below the knee, as if always standing in distance itself. One feather is a folded map nobody has unfolded.
*Lore:* Postcards arrive from it, unsigned, unstamped, from places with no postal service: *"Weather here. Wish you were."* — *"Found the horizon. There's another one behind it."* — *"Not lost. Thorough."* Pell keeps them in a drawer labeled EVIDENCE OF ELSEWHERE.

**Stargazer Heron** · Wonder · C · 3 — A60 / D10 / H50 · Cross · Storm · Arcane
*Physical:* A tall ink heron, plumage flecked with constellation-white that no brush placed. Stands canted back, beak skyward, one leg tucked, reading. The wash around its head stays night-colored even at noon.
*Lore:* "Subject declines to fish during meteor showers," reports Pell. "Subject was observed holding a fish, gently, while both watched the Perseids. Subject's relationship to dinner remains unclear. Subject's relationship to the sky does not."

**Aurora Elk** · Wonder · C · 4 — A45 / D25 / H90 · Cross · Beast, Wanderer
*Physical:* A broad, calm elk whose antlers hold a slow ribbon of last night's sky — green-violet, faintly moving. Hooves leave prints that glow for one breath. Built like patience.
*Lore:* The children count the colors in its antlers and never agree. Four. Six. *Eleven.* The elk stands very still for the counting, because it is polite, and because once, long ago, somebody counted for it, and it has never forgotten how that felt.

**Curio Fox** · Wonder · U · 2 — A45 / D10 / H40 · Slant · Wanderer, Trickster · *On arrival: look at one face-down Fabrication.*
*Physical:* A slip of copper-teal fox, all angles and lean. Ears too big, on purpose. Its tail ends in an inkbrush tip that twitches toward anything closed, locked, wrapped, or folded.
*Lore:*
"What's in the box?"
"Nothing."
"What KIND of nothing?"
— the box, eventually, opened itself, citing exhaustion. The fox was already three secrets away.

**Star-Strewn Otter** · Wonder · U · 3 — A45 / D20 / H50 · Cross · Storm, Tide · *On arrival: your next Ritual costs 1 less.* ◆ Evolution line C
*Physical:* A river otter slicked in deep-blue wash, juggling five points of light in a lazy orbit. The lights reflect in water that isn't there. Expression: smug, earned.
*Lore:* "EXPLAIN THE LIGHTS," demanded Dreamer Juno, four hours in.
The otter added a sixth.
"…Fair," said Juno.

**Vigil Owl** · Wonder · U · 3 — A45 / D20 / H50 · Wide · Storm
*Physical:* A wide, soft owl rendered in three brushstrokes and an opinion. Eyes are two perfect unblinking moons. Sits at the exact center of its own silence.
*Lore:* It blinks once per question. The keepers brought it a list of forty-one. "Proceedings lasted six hours," notes Pell. "Answers received: forty-one blinks. Questions answered: arguably all of them. Arguably none. The owl seemed satisfied, which made one of us."

**Pathfinder Ibex** · Wonder · U · 4 — A45 / D20 / H65 · Cross · Wanderer · Mobile · *Adjacent allies' Reach extends 1 forward (arrival targeting only).*
*Physical:* A wiry mountain ibex of slate-teal, horns spiraling like a route only it remembers. Stands at impossible angles comfortably. Around its neck, a frayed cord with one bead per place it has led someone home.
*Lore:* "Follow," it doesn't say, because it doesn't talk. It just waits at the place where you'd give up, looking back, until you don't. There are seventy-three beads. Pell asked about the cord. The ibex looked at the seventy-fourth, which is you, and started walking.

**Skywhale Calf** · Wonder · U · 5 — A60 / D25 / H105 · Cross · Storm, Beast
*Physical:* A young whale of cloud-grey and gold, swimming breast-high through open air with unhurried flukes. Barnacled with tiny stars. Casts a shadow shaped like wonder, which is to say: everyone looks up.
*Lore:* It is too young to know the sky ended. So, for it, it hasn't. It breaches through nothing, sounding through nowhere, singing a long note to a pod that — listen — *answers*. Somewhere. The keepers cannot find the source of the answer and have, unusually, voted not to look.

**The Asking Light** · Wonder · R · 4 — A60 / D20 / H65 · Cross · Storm · Arcane · *Glimpse — on arrival: look at your top 3, take 1.* ◆ Evolution line A
*Physical:* A lantern-warm sphere of gold with a fringe of small reaching rays, hovering at storyteller height. It brightens at cliffhangers. No lantern: just the light, asking.
*Lore:*
"…and they lived happily ever after."
"*And then?*"
"That's the end."
"And THEN?"
— every bedtime in the Long Gallery runs ninety minutes over. Nobody minds. The endings keep going somewhere, and the light goes with them.

**Tempestrider Roc** · Wonder · R · 5 — A60 / D20 / H80 · Wide · Storm, Beast · *On arrival: allied Reach +1 this round (targeting only).* ◆ Evolution line B
*Physical:* An immense raptor whose wingspan is mostly weather. Feathers shade from teal to thunderhead; the trailing edge of each wing is already rain. It banks, and the painting's horizon tilts with it.
*Lore:* Where it banks, weather follows — eager, loyal, slightly behind, like a dog that is also a monsoon. Farmers post requests on high poles: *NORTH FIELD, PLEASE. GENTLE.* The roc reads. The roc is, mostly, gentle.

**Saffi, Who Lends the Sky** · Wonder · U · 3 — A40 / D20 / H50 · Cross · Storm, Wanderer · *Call — action, 2 Anima: manifest **Twinkle, a Borrowed Star** on an adjacent empty tile. One at a time; Twinkle fades if Saffi leaves play.*
*Physical:* A kite-tailed child-shaped memory in teal scarves, pockets inside-out and full of string. One hand always raised, palm open, as if returning something to a shelf very high up.
*Lore:* She runs a lending library with a single item. "Return it by morning," she says, stamping nothing onto nothing, very officially. "It gets homesick." The star has been borrowed four thousand times. It has been returned four thousand times. This is, Saffi will tell you, an *excellent* record.

**Zenith, Who Asks the Sky** · Wonder · L · 6 — A80 / D20 / H70 · ✶Halo · Storm, Wanderer · Warded · *On arrival: reveal all Fabrications. Your Glimpses take +1 card.*
*Physical:* A vast crane-like being of teal and dawn-gold, tall as weather. Its wingbeats dissolve into question-mark eddies; a slow halo of small glints orbits its head like punctuation looking for a sentence. Eyes like open parentheses. When it lands, the painting gets quieter to hear.
*Lore:*
"What is the sky *for*?"
The sky did not answer.
"WHAT IS THE SKY FOR?"
The sky, embarrassed, made a small wind.
"I'll wait," said Zenith.
— it has waited nine hundred years, posture perfect, patience radiant, volume rising roughly once a century. Fabrications reveal themselves in its presence. Out of respect or terror, opinions differ. (Pell: "Respect." Juno: "TERROR. Lovely though.")

### 3.2 The Charm Squad — proving attachment at Common, one per Resonance

**Cinderling** · Fury · C · 1 — A30 / D10 / H25 · Cross · Flame
*Physical:* A thumb-high flame with arms crossed. Burns vermilion at rest, white at the tips when contradicted. Leaves tiny scorch-prints shaped like exclamation points.
*Lore:*
"You're quite small," observed the candle.
"I am EARLY," said the Cinderling, "in my CAREER."
— the candle has since been promoted to bonfire by association. The Cinderling takes full credit. The Cinderling takes full credit for most things. The Cinderling is, infuriatingly, sometimes right.

**Greyfin Seal** · Sorrow · C · 2 — A30 / D20 / H50 · Cross · Tide
*Physical:* A round grey seal in soft blue wash, settled on a dock post worn smooth in exactly its shape. Whiskers beaded with mist. Eyes on the horizon, calm as schedule-keeping.
*Lore:* "The 4:15 has not run in sixty years," writes the Ferrier. "The seal is not wrong to wait. The seal is simply early. We are all of us simply early for something." Every evening, the Ferrier shares the catch with it anyway. The seal accepts. Vigils need supper too.

**Roundest Frog** · Harmony · C · 1 — A15 / D20 / H25 · Cross · Bloom
*Physical:* A frog that is, geometrically and spiritually, a sphere. Moss-and-rose wash. Sits in the exact center of any lily pad, which then floats level for the first time in its life.
*Lore:* MEASUREMENT LOG, Dreamer Juno: "Attempt 1: circumference exceeds tape. Attempt 2: tape now belongs to frog. Attempt 3: sat with frog instead. Conclusion: it has achieved the shape of contentment, and possibly the contentment of shape. Further study unnecessary. Further sitting scheduled."

**Creep-Toad** · Fear · C · 1 — A30 / D5 / H25 · Slant · Trickster · Mobile
*Physical:* A flat, wide toad of violet-black, drawn slightly out of focus on purpose. Always rendered one hop closer than the previous illustration. Smiling? Unconfirmed.
*Lore:* "Hop. Hop. …hop?" — Pell's field notes end here. They resume three pages later in different handwriting that careful analysis confirms is still Pell's, just *aware*. The toad means no harm. The toad means, as far as anyone can tell, nothing whatsoever. That's the unsettling part.

**Watchful Marmot** · Resolve · C · 1 — A15 / D20 / H25 · Cross · Guardian
*Physical:* A stout marmot at attention atop a pebble it has designated The Post. Slate-bronze wash, chest out, whiskers regulation-straight. Squints at the middle distance with the full authority of none.
*Lore:* INCIDENT REPORT, filed in triplicate, author: self. "Incidents: zero. Suspicious clouds: four (dispersed). Perimeter: held. Commendation: requested." — it has filed nine hundred of these. The keepers approve every one. Somewhere along the line, it stopped being a joke. Nothing has ever snuck past. Nothing has tried. *Vigilance.*

### 3.3 The Six Kindred — small, summoned, irreplaceable

Kindred are small spirits manifested by their caller (design §10). They occupy and score while alive, dissolve to **no impression**, cannot evolve, may be Bonded (yes, people Bond their pets; we knew this would happen; it is encouraged), and fade if their caller leaves play.

**Twinkle, a Borrowed Star** (called by Saffi) — A15 / D0 / H25 · Slant · Storm · Arcane
*Physical:* A marble-sized prick of white-gold light with a paper tag on a string. Casts shadows upward, faintly. Hums at the frequency of a held breath.
*Lore:* The tag reads, in careful handwriting: *PROPERTY OF THE NIGHT SKY (TEMPORARILY).* On the back, smaller: *if found, look up.*

**Cinder-Pup** (called by Embermother Ash) — A30 / D0 / H25 · Lance · Flame · *Frenzy — +10 Attack while damaged.*
*Physical:* A puppy assembled from one ember and boundless intent. Tail wags shed sparks. Practices its big-dog bark against furniture.
*Lore:* "Woof," it said. A curtain caught fire, politely, out of encouragement. "WOOF," it amended. The Embermother beamed. The fire brigade has a standing appointment and, by now, a favorite.

**Droplet** (called by the Ferrier's Daughter) — A0 / D5 / H40 · Cross · Tide · *Parting: restore 10 HP to an adjacent ally.*
*Physical:* A calf-sized raincloud at ankle height, trailing a hem of drizzle. Two pale eyes in the grey. Rains harder when happy, which confuses absolutely everyone.
*Lore:* "Is it sad?"
"It's *delighted.* That's the problem."
— it once attended a wedding. The wedding is remembered, fondly, as The Flood.

**Hum** (called by Choirmother Lark) — A15 / D5 / H25 · Cross · Song · *Chorus — +10 Attack per adjacent ally (max +10).*
*Physical:* A perfectly round bird-blob of rose-green wash. No visible wings; travels by bouncing, on the beat. Mouth permanently open on one (1) note.
*Lore:* It knows a single note. It has committed. The Choirmother built four cantatas, two laments, and a national anthem around it. "Talent," she says, "is mostly *attendance.*" Hum has never missed a rehearsal. Hum *is* a rehearsal.

**Jitters** (called by Marionettist Grey) — A15 / D0 / H25 · Slant · Shade · Mobile
*Physical:* A shadow-mouse stitched from nerves and apology, ears like radar. Moves in saccades. Carries a tiny day-planner in which one entry recurs.
*Lore:* The entry reads: *BE BRAVE — 3 PM.* At 3 PM precisely, Jitters stands its ground for four full seconds, trembling magnificently, and then files bravery as *done* and goes back to flinching with a clear conscience. The keepers consider this an excellent system. Honestly, so do we.

**Chip** (called by Cairnwright Odo) — A0 / D10 / H40 · Cross · Stone · Steadfast
*Physical:* A fist-sized pebble with two chisel-dot eyes and gravel posture. Stands where placed. Stands where placed. Stands where placed.
*Lore:* "What do you want to be when you grow up?"
"A wall."
"Walls are many stones, Chip."
"Then I'll go FIRST."
— Odo says he's load-bearing in spirit. Odo says it without smiling, which is how you know he means it.

### 3.4 The Three Paradox Legendaries — the opposing pairs, held in one being

Each unites a neutral pair from the wheel. They give and receive **no Resonance edges** (their halves cancel), carry dual signature Imprints, and exist to make deckbuilders stare at the ceiling, happily.

**Vertigo, Who Loves the Long Fall** · Wonder + Fear · L · 6 — A95 / D20 / H70 · ✶Plummet (fwd 1, fwd 2, both rear diagonals) · Storm, Shade · Arcane · *After Vertigo engages, you may push either survivor 1 tile.*
*Physical:* A diver-shaped being caught forever in the moment before: top half radiant upward spirals of gold awe, bottom half violet streaks falling away. Under one foot, the floor is always missing. Expression: rapture, mid-gasp.
*Lore:*
"Don't look down."
"Why not?"
"…That's a fair question."
— last exchange recorded at the Edge of the Map. Both parties reportedly delighted. One of them is still falling, on purpose, taking notes.

**Saudade, Who Sets a Place for the Missing** · Sorrow + Harmony · L · 6 — A60 / D25 / H80 · ✶Embrace (4 orthogonal + both forward diagonals) · Tide, Song · *Mourner. When any spirit fully dissolves, restore 10 HP to all your spirits.*
*Physical:* A grandmother-shaped warmth in rose-grey wash, sleeves rolled, always carrying one extra chair. Her shadow holds hands with nobody, gently. The table beside her is set for one more than were invited.
*Lore:*
"Who's that place for?"
"Whoever you're missing."
"…Can I sit there?"
"Sweetheart. That's exactly who it's for."
— supper at Saudade's runs long. Nobody leaves hungry. Nobody leaves *entirely*, ever. That's the comfort. It's also the ache, and she sets the chair anyway.

**Crucible, Who Holds the Fire Still** · Fury + Resolve · L · 6 — A80 / D30 / H65 · ✶Anvil (Cross + fwd 2) · Flame, Stone · Steadfast · *Enemies that engage Crucible take +10 retaliation.*
*Physical:* An ox-built kiln of slate stone, seams glowing banked vermilion. Moves like a verdict. Heat-shimmer rises off it in the shape of held breath. Never hurries. Never cools.
*Lore:* The fire asked to be let out.
"Not yet," said Crucible.
The fire raged, as fire does. Crucible held, as Crucible does. And the fire — for the first time in its bright, brief, furious life — felt not caged, but *held*. There is a difference. Crucible is the difference.

### 3.5 The Solace — three from the PvE set (the Unwritten)

Unwritten spirits share the rule: ***Unwritten** — when it banishes a spirit, the memory is erased entirely: nothing falls where the spirit stood, and the Solace banks the erasure off-board, so its forgetting **scores**. The Unwritten itself, when it dissolves, also leaves nothing.* They are erasure walking, and they **persist until banished** — combat is how you answer them. Players face them in solo and co-op; they are never collected.

**Unwritten Wolf** · Unwritten · — · cost 3 — A60 / D10 / H50 · Lance · *Unwritten.*
*Physical:* A wolf-shaped absence. The paper shows through where it stands; its outline is whatever you almost remember a wolf being, which is worse than any particular wolf. Eyes: two places the ink refuses to go.
*Lore:* "Do not describe it in your notes," writes Pell, describing it. "Description feeds it nothing. But the silence after — the silence is louder. Cross out this entry. I can't. Crossing out is how it got here."

**Sentence Fragment** · Unwritten · — · cost 2 — A45 / D5 / H40 · Slant · *Unwritten. When this defeats a spirit, its controller reveals their top card; Sentence Fragment steals one word of its name (cosmetic, permanent, unsettling).*
*Physical:* A scuttling clause of broken letterforms, missing its end, dragging a comma like a snapped leash. Moves in stops and
*Lore:* "The dog ran—"
"The dog ran—"
"*Please,*" it says, in stolen type, "how does it end?"
— the Unwritten are not all hungry. Some are only unfinished, which turns out to be the same thing from the inside; the hungry ones we call the ill intent.

**White-Out** · Unwriting · — *Unwriting: erase 2 impressions anywhere; one random rim tile fades a round early.*
*Lore:* FORECAST, posted in the Long Gallery in a hand nobody recognizes: *"Nothing, drifting in from the margins. Visibility: total. Accumulation: everything."* The keepers have filed it under WEATHER because the alternative is filing it under TRUE.

### 3.6 Type Exemplars — one Legendary per non-spirit type, plus one more spirit voice

**The Library Remembers** · Ritual · L · 5 — *Put any spirit that fully dissolved this match into your hand; it costs 2 less.*
*Lore:* The request slip came back stamped **FOUND**, in ink still warm. Attached, a note in a librarian's hand no one alive has met: *"Nothing is ever lost. Misfiled, often. Loved things double-shelve themselves. Try under your name."* Pell read it four times, then went and stood in the stacks for a while, for no reason he wrote down.

**The Old Friendship** · Bond · L · 4 — *Pair: +10/+10 each; if one fully dissolves, the other may evolve, ignoring the Imprint rule.*
*Lore:*
"Remember when—"
"Yes."
"I hadn't finished."
"Still yes."
— some bonds outlive their reasons, their owners, and on the evidence of this card, their endings. The Archive lists no origin for it. The Archive lists no origin for most true things.

**The Confluence** · Landmark · L · 3 — *Spirits here +10/+10 and count as adjacent to all four diagonals.*
*Physical (for the board artist):* Two rivers of different-colored ink braiding through one tile without ever mixing — they take turns. Standing stones at the bank, worn smooth on both sides, as if argued against politely for centuries.
*Lore:* Where tellings meet, neither yields and neither breaks. The water has worked out what the Narrators are still fighting about: you can share a bed without sharing a color. Spirits who stand here come away larger, and slightly more reasonable.

**The Blank Page** · Fabrication · L · 3 — *Trap — the engager loses all printed Traits and Keywords permanently.*
*Physical:* Face-down, it is indistinguishable from any Fabrication. Revealed, it is a sheet of perfect, breathing white. The longer it's looked at, the less the looker has to say.
*Lore:* "It's not empty," said Dreamer Juno, packing to leave the exhibit early. "It's *hungry.*" Juno declined to elaborate. Juno, who annotates everything, left the margin blank. The margin has stayed blank. Margins around it tend to.

**Ignis Brightmaw, the Unfinished Argument** · Fury · L · 5 — A95 / D10 / H50 · ✶Comet · Flame, Beast · *Frenzy. Relentless.*
*Physical:* A lion-drake of vermilion dry-brush, mane an explosion of scribbled counterpoints, smoke off its shoulders rising in the shape of rebuttals. Stands like the last word, repeatedly.
*Lore:*
"You're wrong," said the mountain.
"PROVE IT," said Ignis.
"No," said the mountain.
— Ignis circled the mountain for a year, presenting evidence. The mountain remains unmoved. Ignis remains unfinished. Privately — and this is the only private thing about Ignis — it respects the mountain enormously. What's the point of a fire nothing can hold? (See: Crucible. Ignis refuses to.)

### 3.7 The Foundlings — twenty-seven Strays, three philosophies of trust

Foundlings surface as **Strays** (design §6): rarely (~1 in 7 matches, seeded, soft pity cap, telegraphed one round ahead), at the start of either player's turn, on neutral ground. They are Resonance-less (no wheel edges), occupy but **score for no one while unclaimed**, can be **banished** (a normal banisher's impression) or **befriended** — and a first befriending adds the card to your collection permanently. Never craftable, never purchasable. Every Foundling also has a known *haunt* — a story chapter or Daily Memory where it reliably appears — so the collection path is play, never luck.

**Temperaments:**
- **Gentle** — end your turn with a spirit adjacent that shares an Imprint: it joins you.
- **Wary** — as Gentle, but two consecutive turns of adjacency. It's counting. **Wary Strays surface veiled**: identity hidden, untouchable, never intercepting, until a first adjacency (which counts as courtship turn one) or two rounds pass.
- **Feral** — it intercepts arrivals within its Reach. Befriendable only while it bears an Echo: below half HP, its fear cracks, and it can finally recognize you.

The Archive keeps your ledger — Foundlings befriended, Foundlings banished — without comment.

**Dog Who Knows the Way Home** · Gentle · C — A30 / D5 / H40 · Cross · Beast, Guardian
*Physical:* A road-colored mutt of confident brushwork, burrs in the coat, tail at half-mast like a flag of a small calm country. Always rendered mid-glance-backward.
*Lore:* It is not lost. Let's be clear about that. It knows exactly where home is — it checks on you, glances back, trots three steps, checks again. The keepers followed it once, to see. It led them, by an unhurried route, to each of their own front doors. Then it left. It has somewhere to be: *behind whoever's furthest from home.*

**Dog Who Was Left** · Feral · C — A45 / D5 / H40 · Cross · Beast, Shade
*Physical:* The same road-colored mutt, drawn with a harder brush — coat hackled into dry-brush spikes, a frayed rope-end at the collar, eyes that have done the math on people. It stands sideways to everyone.
*Lore:* It waited. That part it did perfectly. The keepers will tell you the two dogs are not the same dog, and the keepers are almost certainly right, and nobody has ever once believed them. It bites first now. It bites *first* now. But below half — when its fear cracks open and the old waiting shows through — it has been known to let a hand come all the way down to its head. Archivist Pell has befriended it forty times across forty matches. He says it gets easier. He is lying, and does it anyway.

**Duckling Following Anyone** · Gentle · C — A15 / D0 / H25 · Cross · Mobile · *(Imprinted on sight: ALL Imprints count as shared when befriending it.)*
*Physical:* A yellow comma with feet. Perpetually three steps behind the nearest moving object, at maximum effort.
*Lore:* It imprinted on the first thing it saw, which was, regrettably, *everything*. It has followed a cart, a cloud, a Kilnhorn Rhino (the rhino slowed down), and once, for an entire afternoon, its own previous footprints. The Dreamers call it the easiest catch in the Memory. The Dreamers are correct and have all, every one of them, kept it.

**Cat From Three Houses Down** · Gentle · C — A15 / D5 / H40 · Slant · Trickster, Wanderer
*Physical:* A self-satisfied loaf of grey wash with one white sock and the posture of a landlord. Renders herself into sunbeams that aren't otherwise depicted.
*Lore:* She has four names, three households, and a feeding schedule that would shame a railway. "OUR cat," say the children at the blue door. "Our cat," agree the newlyweds at the green one. The keepers attempted a census and were each, separately, convinced she lived with them. Befriending her does not mean she's yours. It means you made the rotation.

**Mule Who's Been Promised Things Before** · Wary · C — A15 / D20 / H50 · Cross · Steadfast · Stone, Beast
*Physical:* A slate-grey mule planted like furniture, ears at skeptical half-mast, one long sigh permanently in progress. Carries no load and intends to keep it that way.
*Lore:* "Good grass over here," they said. (It was adequate.) "Light work," they said. (It was a mountain.) "Last time," they said. (It was not.) The mule now operates on a strict evidence-only policy: show up twice, *then* we'll talk. The Ferrier respects this animal more than most people. "Trust with a ledger," he says, "is still trust."

**Wolverine Wearing a Trap** · Feral · R — A60 / D10 / H50 · Cross · Steadfast · Stone, Beast · *Retaliates +10. When befriended, the trap comes off: it loses Steadfast and gains Mobile, permanently.*
*Physical:* A low thundercloud of muscle and spite, dragging a rusted leg-trap and eighteen inches of snapped chain like a war trophy it hates. Moves anyway. Moves *anyway.*
*Lore:* The trap caught it years ago. The trap lost. It has carried the thing since — proof, warning, dare. It intercepts everything that comes close, because everything that ever came close had hands. Befriending it is not gentle work: you wear it down, you stand inside its reach while it's shaking, and then — if it lets you — you take the trap off. Pell has done it once. He says the sound the chain made hitting the ground is the best thing he's heard in the Archive, and Pell works in a building full of music.

**Home, Who Was a Dog Once** · Gentle · LEGENDARY — A45 / D20 / H80 · Cross · Beast, Guardian, Wanderer
*Signature rule:* **It befriends you.** At the end of each round while unclaimed, Home takes one step toward the trailing Narrator's nearest spirit. If it ends a round adjacent to that spirit, it joins that Narrator — no Imprint required.
*Physical:* A great old dog the size of a doorway, rendered in the soft gold of late windows. Its coat carries faint architecture — a suggestion of eaves, a chimney's curl in the tail. Where it lies down, the ground looks briefly like a hearth.
*Lore:* It was a dog so good at coming home that, somewhere along the years, the two ideas wore into one. Now it *is* the home, and it does what home does: it finds the one who's losing. You don't earn it. You can't. You just have a bad enough day, in front of everyone, and then there is a warmth against your leg with a heartbeat in it. — *"Subject located me," writes Pell, "on the afternoon of the flood, the funeral, and the failed exam. Subject's accuracy is frankly upsetting. Subject may stay."*

**Hundredname, Who Has Been Watching** · Wary · LEGENDARY — A60 / D20 / H65 · Veil · Shade, Wanderer, Trickster
*Signature rule:* **Its haunt opens only after you've befriended all eight other Wary Foundlings. Befriending takes **nine turns of adjacency — and it will grant you at most three in any one encounter.** The rest must wait for the next meeting. Earned turns persist across your matches.**
*Physical:* A long cat of deep-violet wash with ninety-nine faint names written into the fur in ninety-nine hands, none of them finished. Its eyes are the exact color of being noticed.
*Lore:* Every Narrator who ever fed it gave it a name. It kept them all and answered to none. It has watched you, specifically, for some time — it knows which corner you favor, which bluffs you repeat, what you do when you're losing. The hundredth name, the one it will finally answer to, is the one *you'll* give it — and it is in no hurry whatsoever. Three visits. Maybe four. It has ninety-nine reasons to be sure.

**Ashmane, Who Outran the Ill Intent** · Feral · LEGENDARY — A80 / D20 / H80 · ✶Hunt (Lance + both forward diagonals) · Flame, Beast, Shade · *Befriendable only while it bears an Echo AND by a spirit sharing TWO Imprints. Its flank is partly unwritten: the first time it would be defeated each match, it survives at 10 HP instead.*
*Physical:* A maned beast — lion in the shoulders, horse in the run — of scorched vermilion dry-brush. Along one flank the ink simply stops: a margin of bare paper in the shape of teeth, where the ill intent almost had it. The mane streams backward even when it stands still. It has not stopped running. It is merely, at the moment, running in place.
*Lore:* It is the only thing on record that the ill intent chased and lost. What it learned in that chase is carved into every line of it: *being held and being erased feel identical from inside.* So it will not be held. It strikes whatever arrives. It survives what should have ended it, because it has practice. And yet — wounded, cracked open, Echo ringing through it — it has, three times in the Archive's history, let someone stay inside its reach. All three describe the same moment: the running stops. The paper-bare flank rises and falls. And the wild thing decides, against all its evidence, to find out what holding is for. — *"You don't tame Ashmane," says the Ferrier. "You testify."*

---

## 6. Vocabulary & naming

**The Banish law (all cards).** Recollect's rules never *kill, slay, destroy,* or *die*. A defeated spirit is **banished from the telling**; its impression falls in the **banisher's color**. Strays offer **banish or befriend**. The Unwritten **Unwrite**: where one banishes a spirit, the memory is erased entirely — nothing falls where it stood, and the Solace banks the erasure off-board (so forgetting *scores*); an Unwritten that *itself* dissolves also leaves nothing. *Forgetting* is the Solace's register and only the Solace's — a Fear card may misplace you, never unwrite you. (The full faction lore is in `decisions/naming.md`.)

**Naming principles.** The "-ling" family (Dawnling, Cinderling, Tearling, Sproutling, Hushling, Pebbling, Oathling, Cloudling, Duetling…) is protected vocabulary: it is how this game says *small and beloved*. Spirits mostly keep evocative species-style names; Uncommon and above may take personal names; Legendaries follow the *"Name, comma, a clause you want to read twice"* pattern.

## 7. Lore

Across the 419 designed cards, **chapter-1 lore and a physical** live in `cards.toml`, in the §1 voice (375 carry their own lore entry and 417 a physical; evolution forms rhyme with their base rather than restate it, and a couple of type-exemplars — The Library Remembers, The Old Friendship — stand on lore alone by design); the Solace speaks in one voice throughout. Chapters 2–3 per card are written at Attunement-system integration, alongside the localization pass.

---


## Card data, lore & physical — see `cards.toml`

The former §4 (the full per-Resonance index), §5 (Kindred-callers, the Paradox
Legendaries, the Solace PvE set, the Foundlings, the curve-fill evolution bases), and
§9 ("The Telling Completed" — every card's lore + physical) were the per-card DATA. That
data now lives in **`app/crates/recollect-core/data/cards.toml`**, one `[[card]]` block
per card, carrying identity + stats + reach + imprints + keywords + rules + the effect IR
+ evolution lineage + lore + physical. The architecture totals above (the 419, the rarity
pyramid, the per-type counts) are asserted against the generated catalog by
`recollect-core/tests/canon.rs`; the lore/physical completeness for the Solace Deepenings
is guarded by `tests/suites/solace_deepenings.rs`.
