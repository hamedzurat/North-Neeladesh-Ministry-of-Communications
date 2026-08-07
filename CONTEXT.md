# North Neeladesh Telephone Exchange

The game models a manual telephone exchange in which the player operates the switchboard and makes consequential routing decisions.

## Language

**Exchange Operator**:
The player character responsible for speaking with subscribers and routing calls through the manual exchange.
_Avoid_: Player operator, telephone agent

**Subscriber**:
An NPC whose telephone line can originate or receive calls through the exchange.
_Avoid_: User, customer, phone

**Caller**:
The subscriber who initiates a particular call.
_Avoid_: Sender

**Callee**:
The subscriber whom the caller asks the Exchange Operator to reach.
_Avoid_: Receiver, destination NPC

**Subscriber Line**:
A telephone endpoint assigned to a Subscriber and represented on the switchboard by a Line Jack and lamp.
_Avoid_: Phone port, NPC port

**Line Lamp**:
The binary physical indicator of whether a Subscriber is holding their receiver off-hook. A lit lamp means off-hook and a dark lamp means on-hook; it does not encode call state through colour.
_Avoid_: LED, coloured status light

**Line Jack**:
The switchboard socket through which the Exchange Operator connects a Subscriber Line to a circuit.
_Avoid_: Female port, phone hole

**Cord**:
A two-ended patch cable used to connect two jacks into a circuit.
_Avoid_: Wire, cable

**Circuit**:
A call path created by a valid Cord arrangement between participating Subscriber Lines or exchange facilities. A Circuit may continue without the Exchange Operator, but must be cleared once either participant's Line Lamp goes dark.
_Avoid_: Connection, route

**Routing**:
The Exchange Operator's act of completing the requested Circuit between a Caller and Callee. A completed Routing earns the ordinary per-call portion of the Shift's earnings.
_Avoid_: Match, hookup

**Routing Rate**:
The amount paid for each completed Routing during the current Shift. It is set and disclosed at the start of the Shift from current world conditions and prior performance.
_Avoid_: Call score, fixed wage

**Tap Bridge**:
A paired set of jacks that completes a caller–callee Circuit and has its own momentary listen control. The Exchange Operator monitors that Circuit only while holding the control.
_Avoid_: Wiretap pair, tap ports

**Directory Terminal**:
The eight-button, four-digit selection device that automatically presents authored information for the selected Subscriber ID on paged e-paper screens. Each digit has an up and down control; there is no separate lookup action.
_Avoid_: NPC display, thumbwheel input, lookup button

**Cabinet**:
The physical arcade installation through which the Exchange Operator controls the exchange.
_Avoid_: Hardware mode, controller

**Shift**:
One configurable workday at the telephone exchange, during which the Exchange Operator handles calls under the rules currently in force.
_Avoid_: Round, level, day

**Run**:
A sequence of Shifts whose accumulated choices and consequences lead to an ending.
_Avoid_: Campaign, playthrough

**Scenario**:
A top-level authored configuration that composes the content, pacing, difficulty, and seed policy for a Run.
_Avoid_: Mode, preset, master config

**Story Event**:
A discrete occurrence in the world that records or causes a consequential change and may contribute to an ending.
_Avoid_: Score change, plot point, LLM action

**Faction**:
An organized political interest competing for influence over North Neeladesh, such as the government, a foreign power, an opposition party, or a rebel network.
_Avoid_: Side, team

**Canonical Fact**:
An authored truth about the setting, a character, or an unfolding situation that improvised conversation cannot contradict.
_Avoid_: Lore prompt, generated fact

**Subscriber Memory**:
A validated recollection belonging to a Subscriber that persists between Shifts within the current Run and may be shared only through permitted story actions.
_Avoid_: Chat history, transcript, cross-run memory

**Call Premise**:
An authored reason for a call that defines eligible participants, required knowledge, prerequisites, and possible Story Events while leaving the conversation itself open to improvisation.
_Avoid_: Script, random prompt, conversation file

**Call Attempt**:
One Subscriber's attempt to place a call, from the first incoming signal until that Subscriber hangs up or the call drops. A later retry is a new Call Attempt and may react to what happened previously.
_Avoid_: Call instance, phone session

**Held Caller**:
A Caller who remains off-hook after being disconnected from the Operator before Routing is complete. The same Call Attempt continues without consuming a Cord, but the Caller's patience falls faster than while connected to the Operator.
_Avoid_: Parked call, queued NPC

**Caller Patience**:
The Subscriber- and Call-Premise-specific time for which a Caller remains off-hook awaiting service. It falls more slowly while the Caller is connected to the Operator and varies reproducibly with the Run seed and current world state.
_Avoid_: Queue timeout, caller timer

**Callee Patience**:
The Subscriber-specific time for which a Callee remains available while the Exchange Operator completes a Circuit after they answer. It varies reproducibly with the Run seed and current world state.
_Avoid_: Handoff timeout, answer window

**Ringing Attempt**:
A continuous period in which a valid Ring Generator Circuit and sufficient cranking alert one Subscriber. It lasts only while the Exchange Operator keeps cranking and ends when the Callee answers, the cranking stops, or the Circuit is removed.
_Avoid_: Ring command, automatic ring

**Misroute**:
A Call Attempt connected to a Callee other than the one requested by the Caller. A Misroute is a recoverable operator mistake that may provoke both Subscribers, trigger a retry, and reduce the Shift's earnings.
_Avoid_: Wrong route, invalid connection

**Service Error**:
A recorded breach of the exchange rules, such as a Misroute or an unjustified premature disconnection. A physically similar action taken for a permitted story reason is a consequential choice rather than a Service Error.
_Avoid_: Player mistake, invalid move

**Shift Earnings**:
Money accumulated during a Shift from completed Routings at the current dynamic rate and authored payments from Subscribers or Factions, less any deductions for Service Errors.
_Avoid_: Score, wage

**Account Balance**:
Money carried between Shifts within the current Run. Its transaction history records Routing income, outside payments, and deductions without assigning them a moral category.
_Avoid_: Total score, wallet

**Service Record**:
The history of the Exchange Operator's logged service performance during the current Run. It affects earnings and may trigger disciplinary Story Events, including dismissal for excessive Service Errors.
_Avoid_: Mistake counter, penalty score

**Disciplinary Status**:
The Exchange Operator's standing under the government rules currently in force, progressing through declared warnings or probation toward possible dismissal. Dismissal ends the Run unless a Scenario explicitly defines another outcome.
_Avoid_: Lives, failure meter

**Service Call**:
An outbound Call Attempt that the Exchange Operator places directly to police, emergency medical services, or the fire service using its dedicated control. It uses no Cord but occupies the single Operator conversation.
_Avoid_: Emergency button, service shortcut

**Operator Session**:
The single audio session through which the Exchange Operator speaks with a Subscriber or direct service using push-to-talk. It is mutually exclusive with listening through a Tap Bridge.
_Avoid_: Voice channel, player chat
