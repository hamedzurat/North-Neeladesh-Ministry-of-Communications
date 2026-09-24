# Shared game mechanics

See [`gameplay.md`](gameplay.md) for the complete mechanic and story guide,
including directory records, call phases, scoring, story transitions, and
current vetting notes.

The backend owns these rules. Odin, the Cabinet Frontend, and the text test
frontend only send physical input snapshots.

## Physical actions

| Player action                                                  | Backend mechanic                                                                                           |
| -------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| Connect a subscriber to `OPERATOR`                             | Opens an operator session for that caller.                                                                 |
| Set the directory digits to a subscriber ID                    | Selects a destination by its configured directory ID. Hardware line numbers are not public IDs.            |
| Connect the requested subscriber to `RING GENERATOR`           | Starts ringing when the caller remains on `OPERATOR`.                                                      |
| Crank with the ring generator connected                        | Arms the ring after the backend delay and lights the destination lamp.                                     |
| Replace the ring cord with a caller-to-destination cord        | Completes direct routing after ringing is ready.                                                           |
| Connect both subscribers to `TAP 1` and `TAP 2`, then hold TAP | Monitors an active connected call without changing its route.                                              |
| Hold `PTT / OPERATOR`                                          | Captures one operator turn for the connected caller.                                                       |
| Hold `EMS` or `POLICE` while speaking                          | Classifies the operator's request using the selected service rules.                                        |
| Release the active control                                     | Ends the voice turn. Disconnecting an active caller cancels the turn and applies the story's failure rule. |

## Story boundary

Stories define beats, participants, dialogue, classifiers, rewards, and valid
outcomes in Rust. They do not define another version of ringing, directory
selection, direct routing, TAP monitoring, patience, or scoring.

Each story declares the shared mechanics it uses through its `MECHANICS` list.
The debug surface exposes those lists so the story code and the visible tools
can be checked against the same vocabulary.

## Directory identity

`exchange.toml` owns the mapping between a subscriber's public directory ID and
its physical line. Story code refers to directory IDs. The backend resolves
those IDs to lines only when applying hardware rules.

For example, directory `1032` identifies Bela Bose's cat entry. The physical
line assigned to that entry may change in configuration without changing the
story's identity.
