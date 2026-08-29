# Replay structures in inspection and bureaucracy games

## Conclusion

*Papers, Please* does not regenerate its Story campaign between Runs. Its authored 31-day spine is largely deterministic; small story branches and player decisions lead to different endings, while routine entrants vary within parameters. Its procedural play is separated into Endless mode. The closest comparable games likewise advertise authored branching, scarce time or resources, different judgements, and multiple endings more often than generated campaigns.

For North Neeladesh, this supports one logical graph of authored Story Events whose availability changes with prior outcomes and completed Shifts. It does **not** support enumerating every possible conversation as a branch. The LLM can let the player speak freely, but authoritative game state still needs a small vocabulary of validated outcomes that can open or close graph paths.

## Factual findings

### Papers, Please

Lucas Pope describes Story mode as mostly deterministic: it has small branches for some story threads, while most ordinary immigrants receive random properties. His earlier summary is even sharper: low-level daily material varies within parameters, but the story is scripted. The traveler generator could fill unspecified details for routine entrants, while an authored story entrant could force their face, name, nationality, document errors, dialogue, and behaviour. ([November 2012 development log](https://dukope.com/devlogs/papers-please/tig-00/), [March 2013 development log](https://dukope.com/devlogs/papers-please/tig-04/))

The campaign was arranged deliberately rather than shuffled into a new narrative on each Run. Pope first used a spreadsheet for each day's settings, traveler placement, news, and rules. He later described assembling encounters, characters, story threads, document ideas, and attacks into a day grid: hook material near the beginning, longer stories in the middle, new elements to maintain interest, and a climax at the end. The post-release layout tool represented days as columns and mechanics, rules, news, travelers, and bulletins as boxes with dependencies; this made crowded or empty days visible. ([March 2013 development log](https://dukope.com/devlogs/papers-please/tig-04/), [May 2013 development log](https://dukope.com/devlogs/papers-please/tig-06/), [postmortem](https://dukope.com/devlogs/papers-please/tig-10/))

This authored spine still accommodates routine variation. Story-critical entrants were placed on particular days so they matched bulletins and news, while generated details supplied the ordinary inspection workload between them. ([February 2013 development log](https://dukope.com/devlogs/papers-please/tig-03/))

The released Story mode lasts at most 31 days. It has 20 endings: 12 can occur early and 8 are available after day 31, depending on decisions made during days and nights. Its branching save system lets a player return to earlier days and try alternatives. ([August 2013 development log](https://dukope.com/devlogs/papers-please/tig-08/))

Endless mode is separate from Story mode. At release it offered Timed, Perfection, and Endurance game types across four active rule sets, with score leaderboards. Pope described its days, entrants, rules, and events as randomly generated with basic progression. ([April 2013 development log](https://dukope.com/devlogs/papers-please/tig-05/), [August 2013 development log](https://dukope.com/devlogs/papers-please/tig-08/))

The replay structure is therefore:

- a stable inspection loop whose rules and workload grow over time;
- scripted story anchors mixed with parametrically varied routine entrants;
- consequences, early exits, and endings driven by player decisions;
- branching saves that reduce the cost of exploring another decision;
- a distinct procedural score mode for players who want the job without the fixed story.

It is **not** a fresh selection or reordering of major authored arcs for every Story Run.

### Closest comparables

The sources below are developer- or publisher-controlled store descriptions, except where a developer explicitly confirms a community explanation. They establish advertised structures, not the games' internal data models.

| Game | First-party facts relevant to variation |
| --- | --- |
| [*Not Tonight*](https://store.steampowered.com/app/733790/Not_Tonight/) | A time-pressure campaign in which the player finds different jobs, improves their apartment and equipment, and chooses whether to help the resistance or keep their head down. The listing says decisions matter; it does not claim procedural stories. |
| [*Death and Taxes*](https://store.steampowered.com/app/1166290/Death_and_Taxes/) | Advertises meaningful choices, dialogue options, an upgrade shop, a branching storyline, and multiple endings. In a replayability discussion, a developer endorsed the explanation that ordinary profiles are random with story exceptions, while replay mainly comes from pursuing different world outcomes and endings; the developer also confirmed seven ending classes in that discussion. ([developer-confirmed discussion](https://steamcommunity.com/app/1166290/discussions/0/2260187970834198892/)) |
| [*Lil' Guardsman*](https://store.steampowered.com/app/1924360/Lil_Guardsman/) | Uses more than 100 unique, fully voiced visitors. The player questions them, deploys limited tools, decides their fate, and influences an alliance and subsequent siege. Its Chronometer can rewind an encounter to explore another result. No generated campaign is advertised. |
| [*Mind Scanners*](https://store.steampowered.com/app/1389550/Mind_Scanners/) | Creates pressure through a long patient list, limited time and resources, equipment development, difficult treatment decisions, the regime's trust, the player's daughter, and the choice to report or join a resistance group. No procedural patient or story claim appears in the official description. |
| [*Booth: A Dystopian Adventure*](https://store.steampowered.com/app/761350/Booth_A_Dystopian_Adventure/) | Combines a series of food-inspection missions, wages and spending, about a dozen recurring people, and what its developer calls a carefully crafted branching story. It does not advertise procedural narrative variation. |

Across these games, the repeated job gains interest from escalating operational pressure and authored consequences. Between Runs, variation usually comes from trying different judgements, allegiances, resource allocations, branches, and endings. Parametric variation is most useful for routine cases around controlled story content.

## Design inference for North Neeladesh

Everything in this section is an inference from the factual patterns above, not a claim about the comparables' implementations.

### Use one logical event graph, not one enumerated conversation tree

The proposed global acyclic graph is a good simplification if its nodes are authoritative **Story Beats** or **Story Events**, not every line an NPC or player might say. It can express all stories in one connected causal space while allowing some branches to start later and some events to occur at fixed points.

A useful node needs only:

- prerequisites: prior event outcomes or Run state that must exist;
- exclusions: outcomes that permanently close it;
- timing: earliest Shift, optional deadline, or an exact fixed Shift;
- participants and one or more eligible Line Listings;
- the premise supplied to the LLM;
- a small set of authoritative outcomes and their state effects;
- successor unlocks, with Endings as terminal nodes.

At the start of a Run, only root events are eligible. Completing Shifts and resolving events makes more nodes eligible, so a longer Run naturally brings more stories forward. Events can join back onto shared later nodes, which keeps the structure acyclic without duplicating every downstream path.

### Preserve free conversation without giving the LLM authority over the graph

The player can say anything and the LLM can answer naturally. After the exchange, however, the model should propose zero or one outcome from those offered by the active Story Event—for example `operator_warned_caller`, `operator_promised_connection`, or `caller_learned_fact`. Deterministic code validates and records it. Only recorded outcomes and explicit Run state alter graph eligibility.

This boundary is what lets authored paths reliably open and close despite unconstrained language. If raw dialogue directly creates graph nodes, facts, or endings, the author can no longer know which prerequisites are true and the acyclic structure stops being authoritative.

### The graph does not replace people or places

The sixteen **Subscriber Lines** can keep stable place-based **Line Listings**. A mix of office, industrial, public-service, ordinary apartment, new apartment, and high-ranking residential listings provides both daily-life and high-stakes calls. Different Subscribers can use the same listing when a Story Event requires them.

Subscriber identity still needs a reusable Profile somewhere, even if the project stops using one text file per NPC. The graph should reference Subscribers and Line Listings; it should not duplicate their voice, relationships, and baseline knowledge in every event node. Whether the graph and profiles live in one database, one authoring document, or several generated data files is a storage decision independent of the narrative model.

### Keep replay variation modest and legible

The lowest-complexity replay plan is:

1. Keep the same broad graph and opening situation.
2. Let player interactions and validated outcomes open and close different paths.
3. Vary routine or ambient calls and non-authoritative details around graph events.
4. Allow event timing to float inside authored windows, while reserving exact Shifts for genuine fixed points.
5. End when an Ending node becomes authoritative, rather than promising a fixed number of Shifts.
6. Consider an Endless Scenario later if procedural switchboard work is enjoyable without the authored graph.

This follows the strongest lesson from *Papers, Please*: use randomness to refresh the work, authored state transitions to make choices matter, and progression to keep the current Run changing. It avoids the extra compatibility problem of selecting several independent Story Threads while retaining multiple paths and Endings.

### Main risk

A single logical graph can still become a production trap if every event is connected directly to every other event. Prefer shared typed Run state and reusable prerequisite queries over pairwise edges wherever possible. The graph should show causality; it should not pre-write a response to every action an LLM-capable player might attempt.
