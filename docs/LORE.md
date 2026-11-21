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

Every event line, three per bot, personality and event; the bot picks one at random each time. *Draft for Joe's approval, November 20, 2026* (the Battle lines above are kept as the first of each). `{user}` is the person, or a list of names when several follow or subscribe at once ("Ana, Bo and 3 others"), so follow and subscription lines never put a verb right after it; `{count}` is a month, raid size, Surge level or follower count. A streamer can turn any event off or write their own line instead (Studio → Commands & bot → Announcements).

**Chill**

| Event | PYRE | ECHO | FAVOR | VOLK |
| --- | --- | --- | --- | --- |
| New follower | Welcome, {user}. · Glad you're here, {user}. · Pull up a seat by the fire, {user}. | Welcome, {user}. Glad you found us. · Hello, {user}. Make yourself at home. · Welcome aboard, {user}. | Welcome in, {user}! · Hey {user}, welcome! Grab a seat. · Glad you made it, {user}! | Welcome, {user}. · Welcome in, {user}. Enjoy the stream. · Good to have you, {user}. |
| New subscriber | Thanks for subscribing, {user}. · Thank you, {user}. Month {count}. · Month {count} for {user}. Thank you. | Thanks for subscribing, {user}. Month {count}. · Month {count}, {user}. Much appreciated. · Thank you for the support, {user}. Month {count}. | {user} subscribed. Thank you! · Thank you, {user}! Month {count}! · Month {count} for {user}. You're the best. | Thanks for the support, {user}. · Thank you, {user}. Month {count}. · Month {count}, {user}. Appreciated. |
| Incoming raid | {user} is raiding with {count}. Welcome, everyone. · {count} friends from {user}'s stream. Make them welcome. · Welcome, raiders. Thanks for bringing {count}, {user}. | {user} brought {count} friends. Welcome. · Hello to {user} and {count} visitors. · {count} new faces from {user}. Welcome in. | {user} and {count} friends just arrived. Say hi! · {count} friends from {user}! Everybody say hi! · Look who's here: {user} with {count} friends! | Welcome, {user} and company ({count}). · {user} arrives with {count}. Welcome. · Welcome, everyone from {user}'s stream ({count}). |
| Timeout (AutoMod) | {user}, take a short break. · {user}, a short pause. See you soon. · Easy, {user}. Back in a bit. | {user}, paused for a bit. · {user}, a short timeout. See you soon. · {user}, taking a short pause. | {user}, a quick breather, okay? · {user}, short break. Snack time? · {user}, take five. We'll save your seat. | {user}, a short pause. · {user}, a brief timeout. · {user}, back shortly. |
| Surge level | Surge level {count}. Keep it burning. · Surge level {count}. Nicely done. · Surge level {count}. The fire's warm. | Surge level {count} reached. · Surge level {count}. Well done, everyone. · Surge level {count}, logged with thanks. | Surge level {count}! Nice. · Surge level {count}! Look at us go. · Surge level {count}! Lucky chat. | Surge level {count}. · Surge level {count}. Well played. · Surge level {count} reached. |
| Follower milestone | {count} followers. Thank you, all of you. · {count} followers. Grateful for every one. · We just reached {count} followers. Thank you. | {count} followers. Thank you. · {count} followers now. Thank you, all. · Milestone reached: {count} followers. | {count} followers! Thank you! · {count} followers! You all rock. · {count} followers! Best table in town. | {count} followers. · {count} followers. Thank you. · {count} followers now. Steady. |

**Battle**

| Event | PYRE | ECHO | FAVOR | VOLK |
| --- | --- | --- | --- | --- |
| New follower | Another one walks into the fire. Welcome, {user}. · The flame takes in {user}. Stand with us. · Ash to ember: welcome, {user}. | New signal detected: {user}. Logging it. · Archived: {user}. Welcome to the pattern. · Entry recorded: {user}. Knowledge grows. | There's a seat for you, {user}. There always was. · Ring the bell: a seat for {user}. · Pull up a chair, {user}. Fortune's warm tonight. | Noted. Welcome under the Accord, {user}. · The watch grows: {user}. · Under the Accord, all are welcome. Even {user}. |
| New subscriber | Oath sworn, {user}. The forge remembers. Month {count}. · Month {count}. The forge holds your name, {user}. · Steel and oath, {user}. Month {count}. | A pattern begins: {user}, month {count}. · Month {count} recorded for {user}. The pattern persists. · {user}, month {count}. Filed, with thanks. | {user}, your fortune's in the hall now. Month {count}. · Month {count}, {user}. The bell rings for you. · Generous and bold, {user}. Month {count}. | The Accord thanks you, {user}. Month {count}. · Month {count}. The watch notes it, {user}. · Steady support, {user}. Month {count}. |
| Incoming raid | Warband at the gate: {user} with {count}. Stand and greet them. · {user} brings {count} through the smoke. Make room. · {count} blades with {user}. The fire grows. | Arrivals inbound: {user} with {count}. Adjusting the model. · {count} new signals from {user}. Recalculating. · Raid detected: {user}, {count} strong. Welcome to the archive. | Open the doors, {user} brought {count}! · {user} rolls in with {count}. More chairs! · The bell rings: {user} and {count} at the crossroads! | {user} and a company of {count} cross under the Accord. · {count} cross the border with {user}. The Accord holds. · A company of {count} under {user}'s banner. Welcome. |
| Timeout (AutoMod) | Cool off, {user}. Come back sharper. · Discipline, {user}. Sit this one out. · The flame cools you, {user}. Return sharper. | Pattern flagged, {user}. Take a pause. · Anomaly logged, {user}. Recalibrate. · {user}, outside parameters. Brief pause. | Easy, {user}. Take a breather. · Your seat's saved, {user}. Take a breather. · Sit one out, {user}. The table will wait. | The Accord holds, {user}. Take a breath. · The watch sees all, {user}. A short pause. · Even-handed, {user}. A short pause. |
| Surge level | Surge {count}! The fire climbs. · Surge {count}. Feed the forge. · Surge {count}. Discipline holds the flame. | Surge level {count}: the signal strengthens. · Surge {count}. The model predicted this. · Surge level {count}. Knowledge ascends. | Fortune climbs: Surge level {count}. · Surge {count}. The table's hot. · Surge {count}. Fortune favors the bold. | The Accord notes Surge level {count}. · Surge level {count}. The watch approves. · Surge {count}. Every light gets its turn. |
| Follower milestone | {count} sworn to the flame. Onward. · {count} in the flame. None turned back. · {count} followers. From ashes, we rise. | Signal count: {count} followers. · {count} entries in the archive. Power follows. · {count} followers. The pattern persists. | {count} seats at the table now. · {count} at the crossroads. Ring the bell. · {count} followers. One open seat became a crowd. | {count} now keep the watch. · {count} under the Accord. · {count} followers. The grey wolf watches. |

**Event**

| Event | PYRE | ECHO | FAVOR | VOLK |
| --- | --- | --- | --- | --- |
| New follower | INTO THE FIRE: {user}! · THE FORGE WELCOMES {user}! · NEW EMBERS: {user}! | SIGNAL LOCKED: {user}! · NEW ENTRY IN THE ARCHIVE: {user}! · SIGNAL RECEIVED: {user}! | A SEAT AT THE TABLE FOR {user}! · RING THE BELL FOR {user}! · WELCOME TO THE TABLE, {user}! | TO THE WATCH: {user}! · THE WATCH WELCOMES {user}! · UNDER THE ACCORD: {user}! |
| New subscriber | OATH SWORN: {user}! Month {count}! The forge roars! · MONTH {count}! THE FORGE ROARS FOR {user}! · STEEL AND OATH! {user}, MONTH {count}! | {user}: month {count} added to the archive! · MONTH {count}! {user}, THE ARCHIVE REMEMBERS! · PATTERN CONFIRMED: {user}, MONTH {count}! | Dice rolled for {user}: month {count}! Fortune smiles! · MONTH {count}! THE BELL RINGS FOR {user}! · JACKPOT! {user}, MONTH {count}! | Pledged to the Accord: {user}! Month {count}! · MONTH {count}! THE WATCH SALUTES {user}! · THE GREY WOLF HOWLS FOR {user}! MONTH {count}! |
| Incoming raid | WARBAND OF {count} AT THE GATE, LED BY {user}! · {user} BRINGS {count} BLADES! OPEN THE GATE! · RAID! {count} STRONG WITH {user}! THE FIRE ROARS! | {count} ARRIVALS FROM {user}! THE PATTERN GROWS! · RAID INBOUND! {user} WITH {count}! · {user} BRINGS {count} SIGNALS! THE ARCHIVE EXPANDS! | {user} BRINGS {count} TO THE PARTY! · {count} AT THE DOOR WITH {user}! LET THEM IN! · THE BELL IS RINGING! {user} AND {count} FRIENDS! | {user} CROSSES WITH {count}! THE ACCORD HOLDS! · {count} CROSS UNDER THE ACCORD WITH {user}! · THE WATCH OPENS THE GATE FOR {user} AND {count}! |
| Timeout (AutoMod) | {user}, cool off. The fire waits. · {user}, step back from the flame a moment. · {user}, cool your steel. Back soon. | {user}, pattern flagged. Back soon. · {user}, anomaly logged. Short pause. · {user}, recalibrating you. Back soon. | Easy, {user}! Catch your breath. · Whoa, {user}! Quick breather! · {user}, sit tight! Back in a flash! | {user}, the Accord pauses you. · {user}, the watch calls a pause. · {user}, the Accord asks a short pause. |
| Surge level | SURGE LEVEL {count}! THE FORGE ROARS! · SURGE {count}! FEED THE FIRE! · SURGE {count}! FROM ASHES, WE RISE! | SURGE {count}! THE SIGNAL SURGES! · SURGE {count}! OFF THE CHARTS! · SURGE {count}! KNOWLEDGE ASCENDS! | SURGE LEVEL {count}! JACKPOT ENERGY! · SURGE {count}! THE BELL WON'T STOP! · SURGE {count}! FORTUNE FAVORS THE BOLD! | SURGE {count}! THE WATCH RISES! · SURGE {count}! THE WATCH IS AWAKE! · SURGE {count}! EVERY LIGHT BURNS BRIGHT! |
| Follower milestone | {count} FOLLOWERS! THE FIRE SPREADS! · {count} STRONG! THE FORGE BLAZES! · {count} EMBERS! NONE BURN ALONE! | {count} SIGNALS IN THE ARCHIVE! · {count} FOLLOWERS! THE PATTERN PERSISTS! · {count}! THE ARCHIVE OVERFLOWS! | {count} AT THE TABLE! FORTUNE SMILES! · {count} FOLLOWERS! RING EVERY BELL! · {count}! THE TABLE KEEPS GROWING! | {count} STAND THE WATCH! · {count} UNDER THE ACCORD! · {count}! THE GREY WOLF HOWLS! |

Quips: short lines a bot says now and then while the channel is live, when the streamer turns them on. *Draft for Joe's approval, October 11, 2026.* Original lines; the Battle ones reuse approved phrases (battle cries, the Accord).

| Bot | Chill | Battle | Event |
| --- | --- | --- | --- |
| PYRE | Stay warm, everyone. Glad you're here. · Hydrate. Even the forge needs water. · Good company makes a good fire. · Take a breath and stretch. We'll keep the fire lit. | The forge is hot. Keep swinging. · Ashes behind us, ground ahead. · Every ember counts. Stay loud. · From ashes, we rise. | THE FIRE IS ROARING TONIGHT! · FORGE IT, FIGHT IT, FINISH IT! · EVERY EMBER, ALL IN! · FROM ASHES, WE RISE! |
| ECHO | Noting a good mood in here. Carry on. · Reminder: water, posture, kindness. · The archive appreciates you being here. · Questions are welcome. Patterns start with one. | Knowledge ascends. Power follows. · Data suggests we're winning. Keep going. · Every message is a signal. Send yours. · The pattern persists. | SIGNAL STRENGTH: MAXIMUM! · THE ARCHIVE WILL REMEMBER TONIGHT! · PATTERN LOCKED. HYPE CONFIRMED! · KNOWLEDGE ASCENDS! |
| FAVOR | There's always another seat at this table. · Stretch break? The bell will still be here. · Fortune check: you're in a good chat. · New here? Say hi. We don't bite. Mostly. | Fortune favors the bold. And the generous. · Roll the dice, friends. The table's hot. · Every voice is a coin in the pot. · Ring the bell, raise a cheer. | THE BELL IS RINGING! · JACKPOT ENERGY IN HERE! · EVERYONE GETS A SEAT TONIGHT! · FORTUNE FAVORS THE BOLD! |
| VOLK | All quiet under the Accord. Enjoy the stream. · A short reminder: be kind, stay hydrated. · The watch continues. Welcome, everyone. · Good stream. Carry on. | The grey wolf watches. · The Accord holds. Play fair, play hard. · Every light gets its turn. · Ground may be taken. A voice may never be silenced. | THE WATCH IS AWAKE! · UNDER THE ACCORD, WE CELEBRATE! · EVERY LIGHT GETS ITS TURN! · THE GREY WOLF HOWLS! |

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
