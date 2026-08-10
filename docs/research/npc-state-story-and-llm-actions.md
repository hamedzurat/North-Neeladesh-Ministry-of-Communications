# NPC state, authored story, and bounded LLM actions

## Conclusion

North Neeladesh does not need a full Nemesis System or an always-running LLM. The useful pattern is a small persistent state core: authored Subscriber Profiles, a few mechanically meaningful values and statuses, Relationship Notes, and a chronological event/action log. A seeded game engine advances this state and selects eligible authored Story Beats. The LLM is called during conversations—and optionally once between Shifts—to write dialogue and propose a catalogue action. It never writes world state or chooses an Ending directly.

## What shipped games suggest

### Nemesis System: remember events, then let engine rules react

Warner Bros.' patent describes stable per-NPC records with static and mutable parameters stored in a relational database or another data structure. Examples include identity, group, appearance, traits, power, rank, relationships, and achievements. Game events can change the involved NPC and related NPCs; the engine can determine offscreen encounter outcomes from character parameters, mission type, and randomness, then queue changes until a later transition. Prior event/action records can select authored dialogue on a later encounter. A patent describes possible embodiments, so this is evidence of the system's design, not proof that every described detail shipped. Sources: [WB patent application US20160279522A1](https://patents.google.com/patent/US20160279522A1/en) and Monolith's first-party GDC talks, [Embracing your Narrative Nemesis](https://www.gdcvault.com/play/1021955/Embracing-your-Narrative-Nemesis-Cinematic) and [Helping Players Hate (or Love) Their Nemesis](https://www.gdcvault.com/play/1025150/Helping-Players-Hate-%28or-Love%29).

**Useful here:** keep stable Subscriber identity and compact event history, but omit the large rank hierarchy. The engine can advance consequences between Shifts from records and seeded rules.

### The Sims 4: actions are authored data with constraints

Maxis describes Sims 4 interactions as largely data-driven. Each authored interaction declares constraints on the Sim's state; the engine tests constraints to decide whether interactions are compatible and whether, how, and where a Sim may perform them. Source: [Maxis, “Concurrent Interactions in The Sims 4”](https://gdcvault.com/play/1021210/Concurrent-Interactions-in-The-Sims).

**Useful here:** a Subscriber Action should be a named catalogue entry with typed parameters, eligibility checks, and engine-owned effects—not a request to set an arbitrary variable.

### Wildermyth: cast authored events from character state

Wildermyth's developer documentation says the engine finds stories that fit the current situation and then chooses among them. Authored story roles can be matched against personality traits, relationships, hooks/aspects, stats, party, and location; mandatory roles and thresholds can prevent an ill-fitting event from running. Its relationship quests are authored multi-event chains driven by character hooks and relationships. Sources: [Event Types](https://wildermyth.com/wiki/Event_Types), [Story Inputs and Outputs](https://wildermyth.com/wiki/Story_Inputs_and_Outputs), and [Event](https://wildermyth.com/wiki/Event).

**Useful here:** Story Threads remain authored. Persistent Subscriber state determines which Story Beat is eligible and who fills each role; the LLM improvises the lines inside that boundary.

### RimWorld: a normal engine can pace the story

Ludeon describes RimWorld's Storytellers as different engine policies over authored event types: one follows a tension curve, one is more random, and one suppresses danger. RimWorld's official overview likewise says its Storyteller controls the “random” events dealt into the story while colonists react mechanically to needs and surroundings. Sources: [Ludeon on Storyteller choices](https://ludeon.com/blog/2013/09/title-screen-update/) and the [official RimWorld overview](https://rimworldgame.com/).

**Useful here:** between Shifts, seeded code—not an LLM—can resolve pending actions, change availability and pressure, and choose the next eligible authored Story Beat.

## Recommended state shape for North Neeladesh

This is a design inference from the systems above, constrained by the project's existing [domain model](../../CONTEXT.md).

Do **not** give every Subscriber a large universal bag of numbers. Store only values that game rules actually read:

- Immutable authored Subscriber Profile: voice, goals, baseline personality, capabilities.
- Small common live state: availability and a coarse pressure level, such as `0..4`.
- Typed role-specific state only where needed: exact Account Balance, medicine supply days, official rank, investigation status, or exposure level.
- Discrete statuses/tags: `under_surveillance`, `ration_suspended`, `owes_operator_favor`.
- Relationship Notes and Subscriber Knowledge as already defined, rather than a universal affinity score.
- Append-only Story Event and Action Record history.

Money is a good numeric value because rules perform exact comparisons and arithmetic on it. “Loyalty 63” is poor unless an authored rule clearly explains what 63 permits that 62 does not.

A minimal Action Record can stay simple:

```text
id | shift | actor | action_type | arguments | result
```

The existence of an authoritative Action Record means the validated action happened. Rejected proposals remain diagnostic evidence rather than Action Records; later consequences or reversals are separate Story Events or actions.

## Where the LLM should and should not decide

The LLM may:

- write in-character dialogue from the bounded Response Context;
- choose whether to propose zero or one currently offered Subscriber Action;
- fill that action's typed arguments;
- write a short Subscriber Memory from facts supplied by the engine;
- optionally render a between-Shift letter, notice, or vignette after the engine has already chosen its facts and outcome.

The engine must:

- decide which actions are offered and validate every proposal;
- apply money, knowledge, status, schedule, and relationship changes;
- run seeded offscreen progression;
- choose eligible Story Beats and their authoritative outcomes;
- decide Endings and Canonical Facts.

This boundary keeps the LLM expressive where variation is valuable and excludes it where a plausible-sounding invention could break the story.

## Output format

XML-like tags are a reasonable envelope, but prompting alone is not a guarantee. `llama.cpp` can constrain generation with a custom GBNF grammar and can convert a supported subset of JSON Schema into a grammar; its documentation also warns that the schema must still be described in the prompt because the model does not see a response schema automatically. Source: [llama.cpp grammar documentation](https://github.com/ggml-org/llama.cpp/blob/master/grammars/README.md).

Qwen's official tool template uses an especially practical hybrid: XML-style `<tool_call>` boundaries containing a JSON object with the tool name and arguments. It recommends letting the tokenizer or Qwen-Agent apply the model's native format. Source: [Qwen tool-calling concepts](https://qwen.readthedocs.io/en/latest/getting_started/concepts.html#tool-calling).

For this game, the equivalent can remain small:

```xml
<response>
  <dialogue>I can send two hundred, but you did not hear that from me.</dialogue>
  <actions>
    <action>{"type":"transfer_funds","recipient":"operator","amount":200}</action>
  </actions>
</response>
```

The core extracts `dialogue` for TTS, parses the action JSON, checks it against the catalogue and current state, and only then writes an Action Record. If no action is proposed, `<actions></actions>` is empty. Begin with at most one action per response; expand only if playtesting finds a real need.

## End-to-end example

1. During a call, the engine offers the factory manager only `transfer_funds(recipient, amount)` with a current maximum of 200.
2. The LLM returns the XML-like response above.
3. The parser accepts the envelope; the engine verifies the actor is allowed to transfer funds, the recipient exists, and the amount is within the offered bound.
4. The engine validates the proposal. If valid, it applies the immediate effect and writes the Action Record to SQLite; if invalid, it records only diagnostic evidence. The LLM cannot alter the Account Balance itself.
5. At Shift end, the seeded engine reads the successful payment plus current authored prerequisites. It may make a bribery-related Story Beat eligible, select it, and update authoritative state.
6. On the next call, the LLM receives only the relevant facts and writes dialogue that remembers the payment. It cannot invent that the manager was arrested unless the engine supplies that event.

This is the useful middle ground: Nemesis-like persistence, Wildermyth-like authored event fitting, RimWorld-like engine pacing, and LLM dialogue without LLM authorship of world truth.
