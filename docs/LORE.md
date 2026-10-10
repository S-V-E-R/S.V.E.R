# Lore: the S.V.E.R Faction Bible

Approved by Joe on October 6, 2026. This is the canon behind Myria, Aetheron and Glint. Names, titles, creeds, values and home turf stay as in [FACTIONS.md](FACTIONS.md) and `apps/web/lib/factions.ts`; this file adds the world. The site's lore text (`lib/factions.ts`: `lore`, `line`, `relic`, `homeland`, `founders` and the rest, plus `WORLD`) comes from here. The working draft is the "S.V.E.R Faction Bible" doc on claude.ai.

## Rules for writing lore

- **Inspiration, not borrowing.** The lore takes techniques from MAG, League of Legends, StarCraft, WoW and Night Angel, and none of their names, places or plots.
- **Tone:** grey at the core, heroic in its faces.
  - Every faction is right about something and wrong about something.
  - No faction is the villain, and nothing suggests one faction's members are worse people.
  - PG-13: loss and ruin, never gore.
- **Three lengths:**
  - Line: one sentence, for crest cards, tooltips and the signup step.
  - Paragraph: three to five sentences, for the Factions page and the welcome ceremony.
  - Full: a page, for faction hubs and a future Lore page.
- **Rivalry lines** in chat and events can trade jabs, never contempt.

## The world

**Line.** When the Beacon burned out, three shards survived, and three peoples rose to carry them.

**Paragraph.** Once, a great light called the Beacon let every voice in the world be seen. Over generations its light narrowed onto a famous few until it cracked, and the Ashfall left everyone else in the dark. Three shards survived: the Phoenix Flame, the Pale Moon and the Crown. Myria, Aetheron and Glint formed around them, and they have fought ever since over who should relight the Beacon. The Grey Wolf, who belongs to none of them, keeps the Accord that limits their war.

**The full telling**

- **The Age of Signal.** The Beacon was not a gift from gods. The first broadcasters built it together. Anyone who spoke beneath it could be seen and heard across the world.
- **The Narrowing.** Light follows attention, and attention follows light. Over centuries the Beacon's light gathered on the voices that already had the most. The rest were still speaking, but nobody could see them. The dimness at the edges is **the Static**.
- **The Ashfall (year 0).**
  - The strain cracked the Beacon. It burned for one night and went dark, and ash fell for a season.
  - Years are counted from that night: **AF**, After the Fall.
- **The three shards:**
  - The **Phoenix Flame** fell burning into the crater and would not go out.
  - The **Pale Moon** rose instead of falling, and hangs in the sky as a second moon.
  - The **Crown** was the Beacon's capstone, the piece that held its light together.
- **The Dim Years (0 to 41 AF).** Each people believed the Beacon must be rebuilt around its own shard. They fought for forty years, and the Static spread wherever they fought.
- **The Grey Wolf.**
  - The Dim Years ended because of the people the Beacon had never lit: drifters, exiles and mercenaries from the edges. They wore grey because they belonged to no colour.
  - They were good at war and refused to choose a side. In 41 AF they forced the three to a table and wrote the Accord.
  - Nobody knows what the Wolf took from the Beacon's ruins. That stays a mystery for a future season.
- **The Accord.** Every faction swears to its five terms:
  1. Ground may be taken. A voice may never be silenced.
  2. Every light gets its turn.
  3. No coin buys the light.
  4. Only the living are counted.
  5. When the season ends, the map is redrawn.
- **The present (300 AF).**
  - The war goes on as a contest instead of a slaughter.
  - Legend says whoever holds enough of the world can relight the Beacon.
  - The Wolf believes a relit Beacon would only narrow again, so it keeps the contest fair and never lets it end.
  - Each season adds a year. Season 1 is 301 AF.

## The factions

| | Myria, the Blazing Vanguard | Aetheron, Masters of the Arcane | Glint, Sovereigns of Fortune |
| --- | --- | --- | --- |
| Line | The ones who stayed in the fire, and rose from its ashes. | The ones who looked up, and learned to read the second moon. | The ones who lit fires in the dark and called everyone in. |
| Relic | The Phoenix Flame | The Pale Moon | The Crown |
| What it gives | Endurance | Moonsight: patterns, odds, what comes next | Gathering: strangers become a people |
| What it costs | It feeds on rest; Myrians burn out | The more you see, the less you feel; the Moonstruck | It answers to the crowd, even when the crowd is wrong |
| Homeland | The Kiln, the crater where the Beacon fell, a forge-city | Selenne, an observatory city on the cliffs | Aurel, a hall-city at the crossroads |
| Shadow | Contempt for anyone who stops | Cold pride; the unmeasured doesn't matter | Comfort; the crowd's will mistaken for right |
| Battle cry | From ashes, we rise. | Knowledge ascends. Power follows. | Every hall starts with one open seat. |
| Bot | PYRE | ECHO | FAVOR |

The paragraph, the faction's own telling of the Ashfall, the relic's full description and the founders are in `lib/factions.ts`, word for word from the approved bible.

**Founders**

| Faction | Founder | Who they were |
| --- | --- | --- |
| Myria | Ondra the Unbowed | First Flamekeeper. She carried the Flame out of the crater for nine days without sleep. "Nine days" means a task finished the hard way. |
| Myria | Hale Corrow | The Quartermaster. He built the Kiln's forges: every tool is earned by learning to make it. Makers and crafters claim him. |
| Myria | Ryn Fastmarch | The Courier. She was the fastest runner of the war's messages; her records stood for a century. Speedrunners claim her. |
| Aetheron | Ilen Marrow | The First Reader. She wrote the Codex of Turns, added to each season. Theorycrafters and educators claim her. |
| Aetheron | Sefa Lumen | The Painter. She painted what the Moon showed so everyone could read it. Artists claim her. |
| Aetheron | Tavik the Gamesman | He built war tables to test battles before fighting them. Strategy, card and board gamers claim him. |
| Glint | Maren of the Open Table | The first sovereign. She was chosen by the crowd after she gave her last coin so a stranger could sit at the fire. |
| Glint | Dallo Reed | The Minstrel. His songs told scattered camps they weren't alone. Musicians claim him. |
| Glint | The Hearthway Twins | Innkeepers who never turned a traveller away, even in the Dim Years. Co-op, MMO and cozy players claim them. |

**Rivalries**

| | On Myria | On Aetheron | On Glint |
| --- | --- | --- | --- |
| Myria says | | "They study the fire. We walk through it." | "They crown the loudest." |
| Aetheron says | "Brave, and wasteful." | | "A crowd is not an argument." |
| Glint says | "They'd rather win alone." | "Clever, and lonely." | |

Under the grudges, each side owes the others something:
- Aetheron's maps have saved Myrian lives.
- Myria held the line while Aurel's halls were being built.
- Glint's fires fed both of them in the Dim Years.

## How the lore maps to the platform

| In the story | On S.V.E.R |
| --- | --- |
| The Narrowing | The problem S.V.E.R fixes: nothing is ordered by size |
| The Static | Obscurity: streaming to nobody |
| The Accord | Fair rotation, no paid boosts, viewer integrity, the guidelines, seasons |
| The Turning | MAGNet's rotation |
| The Grey Wolf | S.V.E.R itself, neutral and keeper of the rules; VOLK is its voice |
| The ravens, the Wolf's messengers | Raven's Eye (Phase 2 analytics) |
| Homelands and ground | Home turf and territories on the war map |
| A season | One year AF |
| Relighting the Beacon | Winning a season ("holding the light") |
| Sparks of the Beacon | Beacons, the short videos |
| Warbands crossing | Raids |
| Keeping a friend's fire | Hosting |
| Free companies | Guilds |
| Valor | What the Accord pays for showing up |

Event idea, not a spec: "The Static rises". This is a cross-faction week where all three sides gain ground together by raiding and hosting streams that haven't had their turn yet. It must never rank anyone by size.

## The faction bots

| Bot | Faction | Who it is | Tagline |
| --- | --- | --- | --- |
| PYRE | Myria | The ember-spirit of the Kiln's first forge. It speaks rarely, in short, hard lines. | Discipline is the flame that never dies. |
| ECHO | Aetheron | The archive of Selenne. It repeats what it learns until it becomes a pattern. | The pattern persists. |
| FAVOR | Glint | The voice of Aurel's crossroads bell, which rings when a stranger is given a seat. Warm and lucky, and the only bot that jokes. | Fortune favors the bold. And the generous. |
| VOLK | Neutral | The Grey Wolf's watchman, keeper of the Accord. Even-handed, dry, and the same voice to every faction. | The grey wolf watches. |

FAVOR replaced JINX on October 6, 2026, because JINX is a well-known League of Legends champion.

Sample lines in the Battle personality:

| Event | PYRE | ECHO | FAVOR | VOLK |
| --- | --- | --- | --- | --- |
| New follower | Another one walks into the fire. Welcome. | New signal detected. Logging it. | There's a seat for you. There always was. | Noted. Welcome under the Accord. |
| New subscriber | Oath sworn. The forge remembers. | A pattern begins. Month one. | Your fortune's in the hall now. | The Accord thanks you. |
| Incoming raid | Warband at the gate. Stand and greet them. | Arrivals inbound. Adjusting the model. | Open the doors, we've got company! | A company crosses under the Accord. |
| Timeout (AutoMod) | Cool off. Come back sharper. | Pattern flagged. Ten minutes. | Easy, friend. Take a breather. | The Accord holds. Take a breath. |
| Season won | From ashes, we rise. | Knowledge ascends. | One open seat became a kingdom. | The map is redrawn. |

Sample lines in the Chill personality (the default; warm, little faction flavor). *Draft for Joe's approval, October 9, 2026.* `{user}` is the person, `{count}` a number.

| Event | PYRE | ECHO | FAVOR | VOLK |
| --- | --- | --- | --- | --- |
| New follower | Welcome, {user}. | Welcome, {user}. Glad you found us. | Welcome in, {user}! | Welcome, {user}. |
| New subscriber | Thanks for subscribing, {user}. | Thanks for subscribing, {user}. Month {count}. | {user} subscribed. Thank you! | Thanks for the support, {user}. |
| Incoming raid | {user} is raiding with {count}. Welcome, everyone. | {user} brought {count} friends. Welcome. | {user} and {count} friends just arrived. Say hi! | Welcome, {user} and company ({count}). |
| Timeout (AutoMod) | {user}, take a short break. | {user}, paused for a bit. | {user}, a quick breather, okay? | {user}, a short pause. |

Sample lines in the Event personality (maximum hype for tournaments, charity streams and milestones). *Draft for Joe's approval.*

| Event | PYRE | ECHO | FAVOR | VOLK |
| --- | --- | --- | --- | --- |
| New follower | {user} STEPS INTO THE FIRE! | SIGNAL LOCKED: {user}! | {user} TAKES A SEAT AT THE TABLE! | {user} JOINS THE WATCH! |
| New subscriber | {user} SWEARS THE OATH! Month {count}! The forge roars! | {user}: month {count} added to the archive! | {user} rolls the dice: month {count}! Fortune smiles! | {user} pledges to the Accord! Month {count}! |
| Incoming raid | WARBAND OF {count} AT THE GATE, LED BY {user}! | {count} ARRIVALS FROM {user}! THE PATTERN GROWS! | {user} BRINGS {count} TO THE PARTY! | {user} CROSSES WITH {count}! THE ACCORD HOLDS! |
| Timeout (AutoMod) | {user}, cool off. The fire waits. | {user}, pattern flagged. Back soon. | Easy, {user}! Catch your breath. | {user}, the Accord pauses you. |

## Where the lore appears

- **Factions page:**
  - Each card's story is the faction paragraph.
  - The "The Ashfall" section has the world paragraph and the Accord.
- **Welcome ceremony and the signup "Your side" step:** the faction paragraph.
- **Faction hubs:** "The story of …" has the line, the paragraph, the faction's own telling, the relic and its price, the homeland, the shadow, the battle cry and the founders.
- **War map:**
  - The RISK-style continent (`TerritoryMap`, PR #72) names each homeland in the sea beside it and marks each faction's capital.
  - Its side panel shows the Accord, the selected territory's founder and the battle cry.
  - Genre cards say which homeland a genre lies in.
  - `lib/lore.ts` maps genres to founders; everything else comes from `lib/factions.ts`.

## Legacy material

| Kept | Reworked | Left behind |
| --- | --- | --- |
| The three titles, the phoenix, moon and crown imagery, "From ashes, we rise", "Knowledge ascends. Power follows.", "Fortune favors the bold" | "Every empire starts with a single coin" became Maren's story and "one open seat" | Glint as a merchant empire. Faction perks (they imply bought or size-based advantages). Placeholder Ferocity, Resilience and Unity scores. The text cards and the ChatGPT sketch, which swapped or contradicted the live identities. |
