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

**Line Jack**:
The switchboard socket through which the Exchange Operator connects a Subscriber Line to a circuit.
_Avoid_: Female port, phone hole

**Cord**:
A two-ended patch cable used to connect two jacks into a circuit.
_Avoid_: Wire, cable

**Tap Bridge**:
A paired set of jacks that completes a caller–callee circuit while allowing the Exchange Operator to monitor it.
_Avoid_: Wiretap pair, tap ports

**Directory Terminal**:
The four-digit lookup device that displays authored Subscriber information relevant to verification and current rules.
_Avoid_: NPC display, number input

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
