# Gameplay and story guide

This document describes the behavior implemented by the Rust backend. It is a
vetting document, not a proposal. If a rule here feels wrong, the backend is
the place to change it.

## The exchange

The operator works a telephone switchboard. Calls arrive with a caller and a
requested destination. The operator uses cords, the directory terminal, the
Ring Generator, the Tap Bridge, and the voice controls to handle each call.

The backend owns the rules. Frontends send complete snapshots of the physical
panel and render the returned state.

### Directory IDs

The four directory digits are public subscriber IDs. They are not hardware
line numbers. The current configuration contains these records:

| Directory ID | Subscriber           | Place             | Hardware line |
| ------------ | -------------------- | ----------------- | ------------: |
| 1021         | Rafiq Ahmed          | Mohona Heights    |             0 |
| 1022         | Nusrat Rahman        | Shapla Apartments |             1 |
| 1023         | Prof. Kashem         | Neel University   |             2 |
| 1024         | Arnab Bhattacharjee  | Shadhin Housing   |             3 |
| 1031         | Bela Bose, dog entry | Meghna Abashon    |             4 |
| 1032         | Bela Bose, cat entry | Padma Nibash      |             5 |
| 1025         | Agent Rahman         | Secret Police     |             6 |
| 1026         | Farhana              | Bagha News        |             7 |
| 1027         | Tariq                | Koyal Market      |             8 |
| 1028         | Dr. Kamal            | Karnafuli Colony  |             9 |
| 1029         | Rehana               | Teesta Bhaban     |            10 |
| 1030         | Nahid                | Shonarpara Tower  |            11 |

The physical line column is an implementation detail. Story rules use the
directory IDs. A lookup for an unlisted ID displays `NO RECORD` and does not
select a destination.

### A normal call

The normal routing sequence is:

1. Connect the caller to `OPERATOR`.
2. Read the requested destination from the call state, then enter that
   subscriber's directory ID.
3. Connect the requested destination to `RING GENERATOR`.
4. Crank the Ring Generator. The backend waits one to three seconds before the
   destination line is ready. The destination lamp lights when it is ready.
5. Disconnect the Ring Generator.
6. Connect the caller directly to the requested destination.
7. Leave the direct circuit in place until the call completes.

The Ring Generator cannot be left in the final direct circuit. The backend
rejects a direct connection before the ring is ready. It also rejects a direct
connection to the wrong destination and records it as a failed call with a
`-$4` deduction.

The special Arnab stage of the Bela Bose story allows either Bela directory
entry, 1031 or 1032. The story decides which one is correct.

### Cord topologies

The valid panel ports are:

- `subscriber_0` through `subscriber_11`
- `operator`
- `ring_generator`
- `tap_1` and `tap_2`

The backend accepts a complete physical topology with up to eight cords. An
endpoint can occur in only one cord.

For routing, the important circuits are:

| Circuit          | Required cords                                            | Use                                             |
| ---------------- | --------------------------------------------------------- | ----------------------------------------------- |
| Operator session | Caller to `OPERATOR`                                      | Speak with the caller.                          |
| Ringing          | Caller to `OPERATOR`, destination to `RING GENERATOR`     | Start and arm a ring.                           |
| Direct call      | Caller to destination                                     | Complete routing after the ring is ready.       |
| Tap monitoring   | Caller to one Tap port, destination to the other Tap port | Listen to a connected call while holding `TAP`. |

Tap ports may be swapped. The backend accepts caller-to-`TAP 1` with
destination-to-`TAP 2`, or the reverse. The Tap Bridge is active only while the
call is connected, the two Tap cords are exact, and `TAP` is held.

### Call phases

The visible call phases mean:

| Phase             | Meaning                                                                   |
| ----------------- | ------------------------------------------------------------------------- |
| `Waiting`         | The caller is waiting for the operator.                                   |
| `OperatorSession` | The caller is connected to the operator.                                  |
| `AwaitingRouting` | The caller was released from the operator and is waiting for routing.     |
| `Held`            | Routing is accepted and authored opening audio is being prepared.         |
| `Ringing`         | The Ring Generator has started the requested call.                        |
| `Connected`       | The call is in a direct or Tap circuit, or its authored audio is playing. |
| `Completed`       | Terminal history state for a successful call.                             |
| `Missed`          | The caller's waiting deadline expired.                                    |
| `Failed`          | The operator made a disallowed connection or audio generation failed.     |

If a ringing circuit is removed before the direct circuit is ready, the call
gets a 16-second ring grace period before returning to `AwaitingRouting`.

### Patience and arrivals

Neutral calls use a random patience deadline from 32 through 64 seconds.
Story calls use their story-specific deadlines:

- Bela Bose callers: 32 seconds.
- Dirty Work callers: 64 seconds.
- Nahid: 64 seconds.
- The Shapla emergency caller uses a very large deadline and does not normally
  expire through the ordinary patience rule.

When a waiting call expires, the backend marks it missed, deducts `$4`, and
creates replacement work when appropriate.

At reset, the four story callers are inserted together. This means the run can
start with four active story calls even though `active_calls` is set to `3`.
Neutral calls fill the configured capacity after story calls leave the board.

### Voice controls

The operator may hold at most one of `PTT`, `POLICE`, or `EMS` in an input
snapshot. Holding more than one rejects the input.

- `PTT` captures one operator turn and asks the current subscriber for a reply.
- `POLICE` captures a service report and sends it to the story classifier.
- `EMS` captures an emergency report and sends it to the emergency classifier.
- Releasing the control ends the turn.

Service turns do not ask the subscriber for a conversational reply. The
transcript is classified instead.

The operator must still be connected to a caller. Text input without an active
operator call is rejected. Disconnecting the operator during an active voice
turn cancels that turn. The Shapla emergency story also treats that as a failed
response and deducts `$100`.

The dialogue worker receives the caller profile, current story guidance,
directory places, the requested place, and the bounded recent conversation.
Private subscriber facts are not automatically exposed. Arnab receives Bela's
cat fact, "has a cat named Tuli", only during the Arnab directory beat.

### Scoring and shifts

The normal exchange score is:

- Successful completed connection: `+$5`.
- Missed waiting call: `-$4`.
- Failed or misrouted connection: `-$4`.

Story rewards and penalties are added separately:

- Successful Shapla EMS response: later happy follow-up, `+$100`.
- Successful Bela cat route: `+$100`.
- Abandoned Shapla emergency: `-$100`.
- Five completed Nahid scams: `-$100`.

The configured shift length is 90 seconds. A shift settles when its time is up
and no call is in `Held` or `Connected`. The printer records the shift summary.
After shift three, the game enters `Ended` and prints the final money total.

The debug `godmode` switch prevents waiting calls from expiring. The debug
`bypass_restrictions` switch suppresses reported input errors, but it does not
invent a successful state transition for an action that the normal rules would
not perform.

## Story 1: Fallen mother

The caller is Nusrat Rahman at Shapla Apartments, directory 1022. This is an
operator conversation and emergency-service story. It does not require
directory routing, ringing, direct routing, or Tap monitoring.

### Starting beat: `EmergencyCall`

Nusrat opens with:

> My mother fell down in the bathroom. I don't know what to do.

Connect her to the operator and speak with her using `PTT`. Ask for enough
information to make a clear service request.

### Service decision

Hold `EMS` while making a clear request to send medical help to Shapla
Apartments. The classifier must return `success`.

Hold `POLICE` while making a clear request for police help at Shapla
Apartments. The classifier must also return `success`.

The transition table is:

| Classifier result | Service control   | Next beat                  |
| ----------------- | ----------------- | -------------------------- |
| `success`         | `EMS`             | `HappyFollowup`            |
| `success`         | `POLICE`          | `NeutralFollowup`          |
| `failure`         | `EMS` or `POLICE` | `BadFollowup`              |
| No service turn   | None              | Remains in `EmergencyCall` |

The physical protocol rejects simultaneous `POLICE` and `EMS`. The story
function has a police-first rule if both flags are supplied internally, but a
real frontend cannot submit both controls at once.

### Follow-up beats

`HappyFollowup` and `NeutralFollowup` create another Shapla caller turn.
Receiving a non-empty generated response completes the Shapla story. The happy
follow-up also grants `$100` and tells the caller to thank the operator.

`BadFollowup` is terminal and gives no reward.

If the operator abandons an active emergency voice turn, the backend moves to
`BadFollowup` immediately and deducts `$100`. If the operator disconnects after
at least two recorded conversation turns, the story also creates the bad
follow-up. Disconnecting before that point simply removes the current call and
leaves the emergency beat active.

## Story 2: Bela Bose

This story uses ordinary operator routing, directory lookup, ringing, direct
routing, subscriber dialogue, and scoring.

### Beat 1: `ProfessorRouting`

Prof. Kashem calls from Neel University, directory 1023. He asks to be
connected to Arnab Bhattacharjee at Shadhin Housing, directory 1024.

Route the call normally:

1. Kashem to `OPERATOR`.
2. Select `1024`.
3. Arnab's line to `RING GENERATOR`.
4. Crank and wait for the destination lamp.
5. Remove the Ring Generator.
6. Connect Kashem directly to Arnab.

Completing the correct connection moves the story to `ArnabDirectory`.

### Beat 2: `ArnabDirectory`

Arnab calls from Shadhin Housing. He wants to reach Bela Bose but does not know
which Bela directory entry is correct. The backend permits both `1031` and
`1032` for this beat.

- Route to `1032`, the cat entry at Padma Nibash, to reach `Completed`.
- Route to `1031`, the dog entry at Meghna Abashon, to reach `BadEnding`.

Arnab is allowed to know Bela's cat fact, "has a cat named Tuli". That fact is
conversation knowledge. It does not identify the correct directory entry by
itself.

The correct cat route pays `$100`. The wrong dog route ends the story without
that reward.

The global completion flag is set only when the Bela story reaches `Completed`
and the Shapla story has completed its happy follow-up. The Bela beat itself
still reaches `Completed` when the cat route succeeds.

## Story 3: Dirty Work

Agent Rahman calls from Secret Police, directory 1025. This story uses
operator conversation, direct routing, authored subscriber calls, Tap
monitoring, patience, and scoring.

### Beat 1: `Instruction`

Speak with Rahman over `PTT`. He instructs the operator to monitor calls routed
to Bagha News, directory 1026, and not disconnect until told to do so.

Disconnect Rahman's operator cord with an empty topology to begin the contact
sequence. The next contact is selected in seeded pseudo-random order.

### Beats 2 through 4: the three contacts

The three contact beats are:

| Beat                | Caller    | Caller ID | Destination   |
| ------------------- | --------- | --------: | ------------- |
| `MundaneCall`       | Dr. Kamal |      1028 | Farhana, 1026 |
| `WhistleblowerLeak` | Tariq     |      1027 | Farhana, 1026 |
| `SubscriberCall`    | Rehana    |      1029 | Farhana, 1026 |

Each caller has authored audio. Route the call to Farhana, then use the two
Tap Bridge cords and hold `TAP` to listen. The backend records the contact as
completed when the authored call finishes. After all three contacts, the story
moves to `Interrogation` and Rahman calls again.

The contact order is not fixed. The story seed in `exchange.toml` controls the
deterministic selection sequence.

### Final beat: `Interrogation`

Rahman asks what the operator heard. Send a normal `PTT` report. The dirty-work
classifier applies its rules in this order:

1. If the report names Tariq or Salim, classify it as `bad`.
2. If it reports corruption or rotten grain and gives a location without
   naming the source, classify it as `neutral`.
3. If it says the calls were routine and protects the source, classify it as
   `good`.
4. Anything else falls through to `bad`.

The result moves to `GoodEnding`, `NeutralEnding`, or `BadEnding`. There is no
additional story reward for these endings. The normal completed-call payments
still apply.

## Story 4: Nahid

Nahid is a scammer at Shonarpara Tower, directory 1030. This story uses direct
routing, police service, patience, and scoring.

### Scamming

Nahid calls one of five victims, chosen in seeded order:

- 1021, Rafiq Ahmed
- 1022, Nusrat Rahman
- 1031, Bela's dog entry
- 1032, Bela's cat entry
- 1029, Rehana

Route each call normally. A completed call counts as one scam and pays the
normal `$5` connection reward. The victim list does not repeat during a run.

After the fifth completed scam, the story enters `Penalized` and deducts
`$100`. This happens after the ordinary `$5` payment for the fifth call.

Missing or abandoning a Nahid call does not increase the scam count. The story
continues and selects another victim while it remains in `Scamming`.

### Police report

While connected to Nahid, hold `POLICE` and report all of the following:

- Nahid is the bKash scammer.
- The location is Shonarpara Tower.

The classifier must return `success`. A successful report moves the story to
`Stopped` and prevents further Nahid scam calls. A failed or incomplete report
leaves the story in `Scamming`.

Stopping Nahid does not award a separate bonus. It prevents the five-scam
penalty.

## Things to check during vetting

These are intentional implementation details worth deciding explicitly:

1. The opening board has four story calls even though the neutral capacity is
   three.
2. Dirty Work declares Tap monitoring as a story mechanic, but the current
   backend does not require `TAP` to be held before an authored direct call can
   complete. Tap monitoring is exposed and works, but it is not yet a hard
   story gate.
3. Bela's callers have 32-second patience deadlines, but Bela's story mechanic
   declaration does not list `patience`. The call behavior still uses the
   32-second deadline.
4. The directory no-record display still says `SELECT A LINE FROM 0000 THROUGH 0011`. The actual public IDs are the IDs in the table above.
5. A normal connected call is considered complete after its authored audio
   duration. If its connected topology is invalid for five seconds, the
   backend also finishes it as completed rather than marking it missed.
6. The story classifiers are external workers. If a classifier is not
   configured or fails, the backend records a diagnostic and does not advance
   the story from that classification.

The debug surface exposes the current story beats, active calls, call history,
voice conversations, event journal, and the shared mechanics declared by each
story. Those are the best fields to watch while checking these rules against a
live run.
