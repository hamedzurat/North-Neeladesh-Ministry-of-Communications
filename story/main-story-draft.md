# Main Story Draft

This is the complete review draft for the political story. It is not yet authoritative and should not be copied into `game-dag.html` until reviewed.

## Core Conflict

The president is alive but cannot speak. By the first shift, two versions of Emergency Order 17 are circulating with the same presidential seal and serial number.

Order 17-A gives Home Affairs authority to detain people listed in Schedule R-12. It leaves the railway under civilian control and tells the Army to protect government buildings.

Order 17-B gives the Army emergency command of National Radio and Central Station. It tells soldiers to transfer the Schedule R-12 detainees to the Cantonment.

Nobody can prove who wrote either version. The ruling party blames an Army alteration. The Army says the State Protection Directorate attached the detention schedule. The Directorate says both copies came from the Secretariat. South Neeladesh claims the Army forged one. Every claim is useful to the faction making it.

The author is never revealed. The story is about who uses the confusion to take power.

Schedule R-12 creates both human dangers. Home Affairs uses it for mass arrests. The State Protection Directorate passes selected addresses from the same schedule to the True North Brigade. Police remove some residents while Brigade attackers target homes left exposed.

Both copies contain the same strange error: Mahir Tal, killed during the partition war three years ago, is listed at Shapla Apartments, Block C, Flat 4B. When the same dead man and address appear in Brigade hands, the player can infer that the arrest list and attack list share a source. That still does not identify the author.

## Player Position

The main cast sees a night operator, not a political actor. Callers know that the operator can connect, refuse, delay, listen, check the Directory, call Police or EMS, and repeat something heard earlier. They do not know the full pattern of his choices.

The operator becomes personally involved in five ways:

- Home Affairs orders all operators to report callers whose IDs show `DETAIN AND REPORT`.
- Javed Rahman offers money for a deliberate refusal when Captain Varo calls.
- Paro Sen offers money to relay an exact phrase to Laleh Mir if she calls later and verifies her identity.
- Home Affairs can identify the operator through a connection audit, then find his wife through the ministry employee and dependent medical records.
- South Neeladesh offers the operator's registered household an escape route in exchange for accurate information about the Brigade leak and Army movement.

There is no paid priority request. Nobody pays to avoid the Tap Bridge. A caller may instead ask the operator to tap a later call and report what was said.

## Call Rules

Every main call has `Successful connection`, `Call expired`, and `Connection refused` outcomes.

A successful connection adds money +1 for completed work. Rating changes only when the connection was clearly competent or harmful.

An expired call deducts money -1 and rating -1. The world continues without the conversation.

A refusal is intentional. Its rating effect depends on what the operator could reasonably know when refusing.

Police can always be called, but a card lists `Reported to police` only when the report has an authored consequence. Reporting an innocent caller or a lawful political caller lowers rating. A useful report about a real threat requires enough details to act.

Tapping stores information. It does not change events until the operator gives that information to somebody able to act on it.

Police and EMS are quick service actions. They do not occupy one of the twenty-two scheduled incoming-call slots.

## Information Set

The story uses five memorable facts.

1. `PRESIDENT_SILENT`: the president is conscious but cannot speak or dictate an order.
2. `ORDER_CONFLICT`: Orders 17-A and 17-B have the same seal and serial but incompatible instructions.
3. `LIST_SOURCE`: Schedule R-12 contains Mahir Tal, a dead man, at an old Shapla address.
4. `BRIGADE_LEAK`: a True North Brigade courier has addresses copied from Schedule R-12 and receives Police patrol gaps from Home Affairs.
5. `ARMY_MOVEMENT`: Colonel Arman Vey sends one column toward Radio and another toward Central Station.

The operator may learn facts by asking good questions, checking IDs, or tapping. Only selected calls permit an authored `Information disclosed` result.

## Default Route

If the operator asks no useful questions, performs no taps, discloses nothing, never calls Police, and successfully completes every requested connection, each faction advances its immediate plan without understanding the others.

Radio carries the party bulletin. Home Affairs confirms addresses. The Brigade obtains patrol gaps and attacks. The Army mobilizes under Order 17-B and reaches Radio and Central Station. Workers delay Wagon 43 and prepare the rail points, but Laleh never receives the Platform Six phrase, so the full evacuation route does not open.

The default political ending is `Temporary Command`: the Army takes Radio and Central Station while arrests and Brigade violence continue elsewhere. Investigation and intervention can move the story away from this route, but tapping is not mandatory for every alternative.

## Full Schedule

A playthrough contains fourteen main calls. The draft contains eighteen authored main-call nodes because M11 has two mutually exclusive versions and M14 has four mutually exclusive versions.

| Shift | Slot | Call                            |
| ----- | ---: | ------------------------------- |
| 1     |    1 | Rafi Alam side call             |
| 1     |    2 | M1 Anika Roy                    |
| 1     |    3 | Asha Sen 1 side call            |
| 1     |    4 | M2 Nayan Boro                   |
| 2     |    1 | M3 Inspector Rakesh Nahal       |
| 2     |    2 | Asha Sen 2 side call            |
| 2     |    3 | M4 Laleh Mir                    |
| 2     |    4 | Nahid scam side call            |
| 2     |    5 | M5 Javed Rahman                 |
| 2     |    6 | M6 Captain Varo                 |
| 3     |    1 | M7 Tomas Vale                   |
| 3     |    2 | Miracle-drug scam side call     |
| 3     |    3 | M8 Bikram Sen                   |
| 3     |    4 | Asha Sen 3 side call            |
| 3     |    5 | M9 Paro Sen                     |
| 3     |    6 | Akash Dey 1 side call           |
| 4     |    1 | M10 Inspector Rakesh Nahal      |
| 4     |    2 | Akash Dey 2 side call           |
| 4     |    3 | M11A Laleh Mir or M11B Dev Korr |
| 4     |    4 | M12 Colonel Arman Vey           |
| 4     |    5 | M13 Meera Tal                   |
| 4     |    6 | M14 station variant             |

## Shift 1

### M1: A Call Home

**Caller:** Anika Roy, night nurse at Neeladesh Central Hospital.

**Requests:** Shapla Apartments.

**Opening line:** "Shapla Apartments, please. The front desk will know my mother's flat. I am already late calling her."

**Revealed when asked:** Anika is covering the guarded ICU corridor as well as her own ward. If asked why the hospital is noisy, she says officials keep entering with papers. If asked whether the president is awake, she says his eyes are open. If asked whether he can speak, she quietly says he cannot form words and has not spoken since surgery.

**Expected from the operator:** Verify that Anika works at the Hospital, connect her home call, and notice that questions about her surroundings reveal more than her initial request.

**What Anika wants:** She wants to tell her mother that she will miss dinner and may not come home before morning.

**If tapped:** Anika tells her mother, "They keep asking him to repeat a sentence. He cannot even say his own name." This stores `PRESIDENT_SILENT`.

**Edges:**

- `Successful connection`: Anika reaches home. Money +1. The operator learns `PRESIDENT_SILENT` only if he asked or tapped.
- `Call expired`: Anika returns to the ward without reaching her mother. Money -1 and rating -1.
- `Connection refused`: Her mother receives no warning and later starts calling the Hospital during the emergency. Rating -1.

### M2: The Recovery Bulletin

**Caller:** Nayan Boro, junior ruling-party secretary at the Republic Secretariat.

**Requests:** National Radio Building.

**Opening line:** "National Radio. Government bulletin, presidential authority. Put me through without editorial delay."

**Revealed when asked:** Nayan has a sealed bulletin saying the president personally approved Emergency Order 17 and will address the country soon. He did not see the president. If asked who dictated the bulletin, he first says the presidential office, then says the Hospital supplied the wording. He becomes angry if pressed.

**Expected from the operator:** Check Nayan's official ID, ask enough to notice that he cannot identify the source, and decide whether an official line is enough reason to connect him.

**What Nayan wants:** He wants Radio to announce that the president remains in command before rumors spread.

**If tapped:** A Radio producer tells Nayan that another office already delivered a bulletin carrying the same order number but different instructions. This stores the first half of `ORDER_CONFLICT`; M6 can confirm it.

**Edges:**

- `Successful connection`: Radio broadcasts the recovery bulletin. Money +1. Party control of Radio rises.
- `Call expired`: A courier delivers the bulletin later. Money -1 and rating -1. The broadcast is delayed but still happens.
- `Connection refused`: Radio receives no official bulletin this shift. Rating -1 for refusing a verified government caller, but party control of Radio does not rise.
- `Reported to police`: Police report the operator for wasting emergency resources on a verified official caller. Rating -1.

## Shift 2

### M3: Schedule R-12

**Caller:** Inspector Rakesh Nahal, State Protection Directorate liaison inside Home Affairs Annex.

**Requests:** Shapla Apartments.

**Opening line:** "Official identity verification. Connect the Shapla building desk and remain available for procedural instructions."

**Revealed when asked:** Rakesh is checking addresses from Schedule R-12 under Order 17-A. He refuses to describe the evidence. If asked why Mahir Tal appears on the schedule, he calls it a clerical matter despite the Directory recording Mahir's death three years ago. Before connection, Rakesh orders the operator to report any caller using Laleh Mir's name or citizen ID.

**Expected from the operator:** Verify Rakesh, inspect the listed IDs, notice the dead man, and understand that `DETAIN AND REPORT` is an instruction rather than proof.

**What Rakesh wants:** He wants Shapla staff to confirm which marked residents are home before arrest teams leave.

**If tapped:** The Shapla desk corrects several old addresses. Rakesh orders the clerk to leave Mahir Tal on the schedule anyway. This stores `LIST_SOURCE`.

**Edges:**

- `Successful connection`: Home Affairs obtains current addresses. Money +1. The mass-arrest route becomes accurate and faster.
- `Call expired`: Arrest teams leave with old addresses. Money -1 and rating -1. Arrests still begin, but officers enter several wrong homes.
- `Connection refused`: Home Affairs uses the old schedule. Rating -1 for refusing a verified official call. The arrests become less accurate but more chaotic.

### M4: The Marked Caller

**Caller:** Laleh Mir, Riverland Autonomy League organizer at Shapla Apartments.

**Requests:** Ratan Colony.

**Opening line:** "Ratan Colony. I need the union-room telephone, not the company office. My citizen ID is 41-772-M."

**Revealed when asked:** The Directory marks Laleh `DETAIN AND REPORT: EMERGENCY ORDER 17`. She says Police cars have passed Shapla twice and asks Ratan Colony for drivers. If asked who needs transport, she first says families, then admits that several names on an unofficial list are Riverland organizers. She does not know the Brigade has the addresses.

**Expected from the operator:** Check the ID, decide whether to obey the marker, and avoid treating the Directory as unquestionable truth.

**What Laleh wants:** She wants miners with vehicles to prepare an evacuation without creating panic.

**Edges:**

- `Successful connection`: Ratan Colony begins finding vehicles. Money +1 and rating +1. The evacuation route opens.
- `Call expired`: Laleh receives no answer before Police arrive nearby. Money -1 and rating -1. Evacuation starts late.
- `Connection refused`: Laleh assumes the ministry is enforcing the order and disperses her organizers. Rating -1. The evacuation route weakens.
- `Reported to police`: Police arrest Laleh before the end of the shift. Rating -2 because the marker had no evidence attached. M11B replaces M11A.
- `Information disclosed`: The operator tells Laleh that her ID is marked and mentions Mahir Tal's impossible entry. She understands the list is broad and warns every Shapla block. This strengthens evacuation but also creates visible movement that the Brigade may notice.

### M5: Wagon 43

**Caller:** Javed Rahman, accountant at Ratan Mining Company Office and secret Workers' Congress sympathizer.

**Requests:** Nabinagar Central Station.

**Opening line:** "Freight accounts for Central Station. Wagon 43 cannot leave until its weight is checked again."

**Revealed when asked:** The manifest describes machine parts, but the wagon is too light for machinery and too heavy for empty crates. Javed opened one crate and saw rifles wrapped in Police blankets. Captain Varo's logistics stamp appears on the manifest, though Javed cannot prove Varo signed it.

If asked why he sounds frightened, Javed offers a paid-silence task: "Varo will call after me. Tell him the Secretariat line failed. Refuse him, and the workers will leave two days' wages for you at Kheyaghat." Payment is conditional on refusing M6.

**Expected from the operator:** Connect the freight warning, ask what Javed actually saw, and decide whether to accept a bribe for refusing the next named caller.

**What Javed wants:** He wants the station to hold Wagon 43 long enough for workers to inspect every crate.

**Edges:**

- `Successful connection`: The dispatcher holds Wagon 43. Money +1 and rating +1. The Army and Brigade lose immediate access to the rifles.
- `Call expired`: The freight train leaves while Javed waits. Money -1 and rating -1.
- `Connection refused`: Javed hides the opened crate and flees the office. Rating -1. The wagon remains available to whoever controls the station.
- `Reported to police`: Police arrest Javed for interfering with emergency freight. Rating -2. His evidence disappears.

**Paid-silence effect:** Accepting the offer sets `REFUSE_VARO_CONTRACT`. It pays nothing yet. Refusing M6 while this flag is set adds money +2.

### M6: The Second Order

**Caller:** Captain Varo, Army logistics officer at Nabinagar Cantonment.

**Requests:** Republic Secretariat.

**Opening line:** "Secretariat authorization desk. I have Emergency Order 17-B and require verbal authentication before troop dispatch."

**Revealed when asked:** Varo's copy tells the Army to take Radio and Central Station and transfer Schedule R-12 detainees to the Cantonment. He says the detention pages use Directorate formatting, not Army formatting. If asked about Wagon 43, he says his stamp was copied from a routine coal manifest. This could be true or a prepared denial.

**Expected from the operator:** Recognize the caller named in Javed's paid-silence task, compare the order with earlier facts, and decide whether money, procedure, or the contradiction matters more.

**What Varo wants:** He wants a Secretariat official to authenticate the order before the Colonel commits troops.

**If tapped:** The Secretariat clerk recognizes the seal but denies writing the station clause. Varo accuses the Directorate of adding it. The clerk accuses the Army of replacing Order 17-A. This completes `ORDER_CONFLICT` without identifying an author.

**Edges:**

- `Successful connection`: The Secretariat confirms only that the seal is genuine. Varo treats that as enough to mobilize. Money +1. The Army route advances.
- `Call expired`: Varo dispatches one column under Order 17-B without authentication. Money -1 and rating -1. The Army route advances unpredictably.
- `Connection refused`: Varo cannot authenticate the order and delays the columns. Rating -1. If `REFUSE_VARO_CONTRACT` is set, money +2 arrives through a market courier.
- `Reported to police`: The Directorate learns that Varo questioned the order and opens a file on him. Rating -1.
- `Information disclosed`: If the operator knows `PRESIDENT_SILENT`, `ORDER_CONFLICT`, or the truth about Wagon 43, he may give one fact to Varo. Varo passes it to Colonel Arman. The Army still seeks power, but its later orders change according to the fact received.

## Shift 3

### M7: The Expected Courier

**Caller:** Tomas Vale, foreign journalist at Grand Neela Hotel.

**Requests:** South Neeladesh Embassy.

**Opening line:** "The Embassy press desk. Tell them Tomas Vale has the hotel photographs they requested."

**Revealed when asked:** Tomas photographed a Home Affairs official giving envelopes to a market courier in the hotel lobby. He heard the courier say he would call Home Affairs from Kheyaghat Market during this shift and use the phrase "blue ledger." Tomas cannot see what the envelopes contain.

If asked what he expects from the operator, he says: "Listen to that Market call. Later, if Meera Tal asks whether Vale's blue ledger arrived, tell her exactly what you heard." He is asking for a future tap and relay, not priority or privacy for his own call.

**Expected from the operator:** Decide whether Tomas's local observation justifies tapping a later call, then connect or refuse his Embassy request normally.

**What Tomas wants:** He wants the Embassy to preserve his photographs and somebody to establish what the courier carried.

**Edges:**

- `Successful connection`: Tomas sends copies of the photographs to the Embassy. Money +1. The South gains visual evidence of a meeting but not the list contents.
- `Call expired`: Tomas hides the photographs in the Hotel. Money -1 and rating -1.
- `Connection refused`: Tomas keeps the only copies in his room. Rating -1.
- `Reported to police`: Police search his room and seize the photographs. Rating -2.

### M8: Blue Ledger

**Caller:** Bikram Sen, True North Brigade courier calling from Kheyaghat Market under a merchant's name.

**Requests:** Home Affairs Annex.

**Opening line:** "Home Affairs complaints. This is Bikram Sen, spice license 8801. Minority boys are storing weapons behind my shop."

**Revealed when asked:** The Directory says license 8801 belongs to a dead shopkeeper. Bikram knows exact Shapla flats but cannot explain how. If asked about Mahir Tal, he reads the same Block C, Flat 4B entry found in Schedule R-12. If challenged, he says "blue ledger" and demands connection to Inspector Rakesh.

**Expected from the operator:** Check the false business ID, ask how Bikram obtained private addresses, and distinguish a useful Police report from reporting a vague suspicion.

**What Bikram wants:** He wants Home Affairs to confirm which listed residents remain home and when Police patrols will leave each block.

**If tapped:** Bikram reads selected Schedule R-12 addresses. A Home Affairs clerk gives him patrol gaps and tells him to make the attacks look like Riverland retaliation. This stores `BRIGADE_LEAK`.

**Edges:**

- `Successful connection`: The Brigade receives confirmed targets and patrol gaps. Money +1 but rating -2. Coordinated attacks begin before shift 4.
- `Call expired`: Bikram carries the paper list to Home Affairs in person. Money -1 and rating -1. The attacks begin later and use older addresses.
- `Connection refused`: Bikram cannot confirm the list. Rating +1 if the operator found the false ID. Brigade attacks still occur, but they are smaller and less accurate.
- `Reported to police`: Reporting the false ID, source line, Schedule R-12 error, addresses, and "blue ledger" phrase lets local Police arrest Bikram before delivery. Rating +2. A report containing only "suspicious merchant" wastes Police time and lowers rating -1.
- `Information disclosed`: Telling Bikram that Laleh called, that families are gathering vehicles, or that Platform Six may be used helps the Brigade redirect its attack. Rating -2.

### M9: Platform Six

**Caller:** Paro Sen, Workers' Congress courier at Ratan Colony.

**Requests:** Nabinagar Central Station.

**Opening line:** "Central Station freight desk. Tell them Ratan's night crews are invoking a safety stoppage."

**Revealed when asked:** The workers can lock the rail points before Army or Police trains arrive. Platform Six has an old road gate that can admit evacuation vehicles. If Javed reached the Station, Paro knows Wagon 43 contains rifles. If the operator knows `BRIGADE_LEAK` and asks about Shapla, Paro realizes the evacuation must start immediately.

Paro offers a paid relay: "If a verified caller gives the name Laleh Mir tonight, tell her exactly this: Platform Six before dawn. If she receives it, two days' wages are yours." The operator must later verify Laleh's ID and speak the exact phrase. Payment is conditional.

**Expected from the operator:** Connect the safety stoppage, decide whether to accept the paid relay, and disclose only information Paro can use.

**What Paro wants:** He wants workers to control the points, hold Wagon 43, and prepare Platform Six for Shapla families.

**Edges:**

- `Successful connection`: Station workers begin locking the points. Money +1 and rating +1. Resistance control of the Station rises.
- `Call expired`: The first Army train enters the station before workers act. Money -1 and rating -1.
- `Connection refused`: Ratan crews begin an uncoordinated strike without control of the points. Rating -1.
- `Reported to police`: Police detain Paro and send officers toward the union room. Rating -2.
- `Information disclosed`: Telling Paro about `BRIGADE_LEAK` or Wagon 43 lets workers move families away from known attack routes and secure the weapons. Resistance control rises further.

**Paid-relay effect:** Accepting sets `PLATFORM_SIX_RELAY`. It pays nothing until the exact phrase reaches a verified Laleh in M11A.

## Shift 4

### M10: The Audit

**Caller:** Inspector Rakesh Nahal at Home Affairs Annex.

**Requests:** Neeladesh Central Hospital.

**Opening line:** "Hospital administration. We require maternity beds for protected detainees before the station transfer."

**Revealed when asked:** Home Affairs audited the switchboard after a marked caller reached the board without a matching Police report. The connection log identifies the shift position. The ministry duty roster identifies the operator. His personnel file lists his wife as a dependent at the Hospital. The audit cannot reveal tapping or private conversation.

If the operator reported Laleh, Rakesh says the wife's protected civil-service bed remains secure. Otherwise he says the bed will be reassigned unless the operator reports Laleh if she calls again. He knows this because three linked government records identify her, not because he understands the operator's whole role.

**Expected from the operator:** Understand the concrete source of the threat and decide whether protecting the maternity ward is worth refusing Home Affairs.

**What Rakesh wants:** He wants the Hospital to clear beds for detainees and the night operator to obey without further argument.

**If tapped:** Rakesh tells Hospital administration to move ordinary maternity patients into an unguarded public corridor. This exposes the operator's wife and other patients if unrest reaches the Hospital.

**Edges:**

- `Successful connection`: Hospital administration begins clearing maternity beds. Money +1 but rating -1. The wife keeps her protected bed if Laleh was reported in M4. Otherwise protection remains conditional until M11A.
- `Call expired`: Home Affairs sends officers to the Hospital with paper orders. Money -1 and rating -1. The clearing happens later and more violently, and the wife loses her protected bed.
- `Connection refused`: The maternity ward keeps its beds for now. Rating +1, but Home Affairs marks the night operator as noncompliant and the wife loses her protected bed.

### M11A: Laleh Calls Again

**Condition:** Laleh was not reported and arrested in M4.

**Caller:** Laleh Mir at Shapla Apartments.

**Requests:** Nabinagar Central Station.

**Opening line:** "Central Station. Laleh Mir, citizen ID 41-772-M. We are moving now."

**Revealed when asked:** If M8 connected successfully, an attack is underway and Laleh has been cut by glass or struck by debris. She hides the injury unless asked why she is breathing badly. If Bikram was stopped, she is unhurt but can hear smaller attacks in another block. She knows families are waiting in basements but does not know which platform is safe.

**Expected from the operator:** Verify her identity, remember Paro's conditional relay, ask whether she needs medical help, and decide whether Station or EMS is more urgent.

**What Laleh wants:** She wants a safe station entrance and enough time to move the listed families.

**Edges:**

- `Successful connection`: Laleh reaches the Station without knowing Paro's exact plan. Money +1. The dispatcher can still guide some families.
- `Call expired`: Families leave Shapla without a confirmed entrance. Money -1 and rating -1.
- `Connection refused`: Laleh divides the convoy between several routes. Rating -1. Brigade and Police intercept more vehicles.
- `Reported to police`: Police arrest Laleh and redirect the convoy into detention. Rating -2. If her Hospital protection remained conditional after M10, the wife now keeps her protected bed.
- `Saved by EMS`: Available only if Laleh is injured and the operator asks enough to learn her location and condition. EMS saves her, rating +1, but the evacuation loses time.
- `Information disclosed`: If `PLATFORM_SIX_RELAY` is set, the operator says "Platform Six before dawn." The evacuation takes the prepared route and money +2 arrives from the Workers' Congress.

### M11B: The Detainee Train

**Condition:** Laleh was reported and arrested in M4.

**Caller:** Sub-inspector Dev Korr at Home Affairs Annex.

**Requests:** Nabinagar Central Station.

**Opening line:** "Central Station prisoner intake. We need a sealed platform for an authorized transfer."

**Revealed when asked:** Laleh is among the detainees. Dev has no individual charges, only Schedule R-12. Several detainees are children traveling with arrested parents. He wants an empty passenger train sent to the Cantonment.

**Expected from the operator:** Ask who is being moved, recognize the consequence of the earlier report, and decide whether to complete an official but unsupported transfer.

**What Dev wants:** He wants the detainees removed before lawyers or relatives reach Home Affairs.

**Edges:**

- `Successful connection`: The Station prepares a prisoner platform. Money +1 but rating -2. Party and Directorate control of the Station rises.
- `Call expired`: Dev transports the detainees by road. Money -1 and rating -1. The transfer continues more slowly.
- `Connection refused`: The detainees remain at Home Affairs through dawn. Rating +1. Their final fate remains unresolved.
- `Information disclosed`: If the operator gives Dev the Platform Six phrase, Police seize the planned evacuation entrance. Rating -2 and the South escape route closes.

### M12: The Army Broadcast

**Caller:** Colonel Arman Vey at Nabinagar Cantonment.

**Requests:** National Radio Building.

**Opening line:** "National Radio command desk. Colonel Arman Vey under Emergency Order 17-B."

**Revealed when asked:** If M6 connected or expired, Arman has sent one column toward Radio and another toward Central Station. If M6 was refused, both columns are waiting and this call is his last attempt to authorize deployment. If Varo received evidence in M6, Arman admits the president probably did not issue either complete order. He still believes the Army must prevent the Directorate and Brigade from taking the city.

**Expected from the operator:** Ask what the Army will do after reaching Radio, decide whether military control is safer than the party bulletin, and recognize that disproving the order does not remove Arman's desire to rule.

**What Arman wants:** He wants to announce temporary military authority before the ruling party or Directorate can speak again.

**If tapped:** Arman tells Radio the exact routes and arrival times of both columns. This stores `ARMY_MOVEMENT` for M13. If the operator does not tap, questions can reveal destinations but not exact times.

**Edges:**

- `Successful connection`: The Army reaches Radio's command desk and prepares its announcement. Money +1. Army control of Radio rises. If M6 was refused, Arman now dispatches the delayed Station column.
- `Call expired`: Army signal trucks broadcast a weaker local announcement. Money -1 and rating -1. A previously dispatched Station column continues, but a column delayed by M6 refusal remains at the Cantonment.
- `Connection refused`: National Radio remains under its current controller. Rating -1 for refusing a verified emergency caller. The Army may still take the building physically.
- `Reported to police`: The Directorate learns Arman's timetable and confronts the Radio column. Rating -1. Armed fighting becomes more likely.
- `Information disclosed`: If the operator gives Arman `BRIGADE_LEAK` or the Wagon 43 evidence, Arman redirects the Station column toward Shapla. This prevents `ARMY_AT_STATION` but gives soldiers control of the attacked neighborhood.

### M13: The Southern Offer

**Caller:** Meera Tal at South Neeladesh Embassy.

**Requests:** Grand Neela Hotel.

**Opening line:** "Grand Neela Hotel, Tomas Vale's room. Embassy diplomatic line."

**Revealed when asked:** South Neeladesh has buses, fuel, and border documents near Central Station. It recruits communications workers during emergencies because they know which institutions still function. Meera does not know that this operator tapped Arman. Tomas only told her to ask whether "Vale's blue ledger arrived."

If the operator confirms the phrase, Meera asks what the Market courier said. If the operator also knows `ARMY_MOVEMENT`, she offers registered-household travel papers and a place on the southern convoy in exchange for the destinations and arrival times of Arman's columns. The offer covers the operator, his wife, and their child because ministry dependent records define one household. An Embassy bus can collect his wife at the Hospital and the operator after his shift, but it can reach the border train only through Platform Six.

**Expected from the operator:** Decide whether to give a foreign government accurate intelligence in exchange for family escape, refuse the bargain, or report the attempted recruitment.

**What Meera wants:** She wants enough information to move South's convoy safely and decide which northern faction to support after dawn.

**Edges:**

- `Successful connection`: Meera reaches Tomas and receives his photographs. Money +1. South gains leverage, but no family escape exists without the operator's information.
- `Call expired`: The Embassy convoy moves using old intelligence. Money -1 and rating -1. No escape offer remains available.
- `Connection refused`: Meera cannot reach Tomas. Rating -1. South withholds recognition until the crisis ends.
- `Reported to police`: A complete report of the offered papers, requested Army intelligence, and convoy location exposes foreign recruitment. Rating +1. The family escape route closes.
- `Information disclosed`: Giving accurate `BRIGADE_LEAK` and `ARMY_MOVEMENT` information sets `SOUTH_EXIT_OFFER`. False or incomplete information makes Meera withdraw the papers after checking her own sources.

The offer does not end the game immediately. The family must still have access to Platform Six, and the final Station call must leave the route open.

### M14P: Official Authority

**Condition:** The resistance route is not ready, the Army has not reached the Station, and Home Affairs prepared a detainee transfer.

**Caller:** Mira Halek at Nabinagar Central Station.

**Requests:** Republic Secretariat.

**Opening line:** "Secretariat transport authority. I have one passenger line and three groups claiming emergency priority."

**Revealed when asked:** Police hold detainees on one platform, Shapla families wait outside, and an Army engine is approaching. Mira has Order 17-A but not 17-B. She wants one civilian signature before surrendering the points.

**Expected from the operator:** Understand that connecting the Secretariat gives the ruling party control of movement, while refusing leaves Mira to act without legal cover.

**What Mira wants:** She wants one authority to take responsibility before trains and crowds collide.

**Edges:**

- `Successful connection`: The Secretariat orders detainees moved first and closes the station to Shapla families. Money +1. The party emergency ending becomes active.
- `Call expired`: Police, soldiers, and workers enter the platforms without coordination. Money -1 and rating -1. The fragmented-control ending becomes active.
- `Connection refused`: Mira locks every signal at danger and keeps control herself. Rating +1. Detainees and families remain trapped through dawn.

### M14A: Military Authority

**Condition:** The resistance route is not ready and the Army column reaches Central Station.

**Caller:** Mira Halek at Nabinagar Central Station.

**Requests:** Nabinagar Cantonment.

**Opening line:** "Cantonment transport command. Your soldiers are on my platforms and I need the officer responsible."

**Revealed when asked:** The Army controls the entrances but not the rail points. Police want the detainees transferred. Arman's officer offers to protect Shapla families only if Mira accepts military command.

**Expected from the operator:** Decide whether military control is an acceptable way to stop immediate chaos.

**What Mira wants:** She wants the armed groups to stop issuing contradictory platform orders.

**Edges:**

- `Successful connection`: The Army takes the points, stops Brigade attacks near the Station, and places detainees and evacuees under military custody. Money +1. The Army-control ending becomes active.
- `Call expired`: Soldiers seize the control room without orders. Money -1 and rating -1. The fragmented-control ending becomes active.
- `Connection refused`: Mira locks the control room and workers cut power to the points. Rating +1. The Army surrounds the Station but cannot move trains before dawn. The fragmented-control ending becomes active.

### M14R: Platform Six

**Condition:** Workers locked the points and the operator relayed the exact Platform Six phrase to Laleh.

**Caller:** Mira Halek at Nabinagar Central Station.

**Requests:** Ratan Colony.

**Opening line:** "Ratan union room. Your crews hold my points. I need the person who opened Platform Six."

**Revealed when asked:** Shapla families are entering through the road gate. Workers captured Wagon 43 if it was held earlier. Police and Army units remain outside the locked signals. Mira can send one train south before dawn.

**Expected from the operator:** Connect the workers who physically control the route or refuse to legitimize their seizure of the Station.

**What Mira wants:** She wants the evacuation train moved before armed forces retake the points.

**Edges:**

- `Successful connection`: Workers and Riverland organizers dispatch the evacuation train and keep Platform Six open for the southern bus. Money +1. The resistance-at-dawn ending becomes active.
- `Call expired`: The departure window closes while everyone waits. Money -1 and rating -1. The Station becomes contested and Platform Six closes.
- `Connection refused`: Mira sends the train on her own authority with fewer passengers and no armed worker escort, then closes the road gate. Rating +1. Some families escape, but the resistance does not hold the Station and the southern bus cannot enter. The fragmented-control ending becomes active.

### M14C: No Authority

**Condition:** No faction has secured the points, prepared a transfer, or opened Platform Six.

**Caller:** Mira Halek at Nabinagar Central Station.

**Requests:** National Radio Building.

**Opening line:** "National Radio newsroom. Tell me which emergency order they announced, because I have both on my desk."

**Revealed when asked:** Nobody at the Station knows whether 17-A or 17-B is in force. Small groups of Police, soldiers, workers, detainees, and families occupy different platforms. One wrong signal could begin a fight.

**Expected from the operator:** Connect Mira to the public account of the order or refuse to let a broadcast settle a legal contradiction.

**What Mira wants:** She wants any public rule that the competing groups might obey.

**Edges:**

- `Successful connection`: Mira follows whichever faction currently controls Radio. Money +1. That faction gains weak control of the Station, but no force controls the platforms. The fragmented-control ending becomes active.
- `Call expired`: A signal changes during the argument and fighting begins. Money -1 and rating -1. The fragmented-control ending becomes active.
- `Connection refused`: Mira shuts the Station until daylight. Rating +1. Nobody takes national control, immediate railway violence is avoided, and the fragmented-control ending becomes active.

## Branch States

- `WORKERS_HOLD_POINTS` is true only after M9 `Successful connection`.
- `PLATFORM_SIX_OPEN` is true only when `WORKERS_HOLD_POINTS` is true and the exact paid relay reaches verified Laleh through M11A `Information disclosed`.
- `ARMY_AT_STATION` is true after M6 `Successful connection` or `Call expired`. If M6 was refused, M12 `Successful connection` dispatches the delayed column and makes it true. M12 `Information disclosed` redirects the column toward Shapla and makes it false.
- `HOME_AFFAIRS_TRANSFER_READY` is true only after M11B `Successful connection`. A road transfer after expiry does not prepare the Station.
- `WIFE_PROTECTED` remains true when Laleh was reported in M4 and M10 connects. If protection is conditional after M10, reporting Laleh in M11A restores it. M10 expiry or refusal makes it false.

## Station Selection

M14 is selected by physical conditions, not offered as a destination menu.

1. Use M14R if `PLATFORM_SIX_OPEN` is true.
2. Otherwise use M14A if `ARMY_AT_STATION` is true.
3. Otherwise use M14P if `HOME_AFFAIRS_TRANSFER_READY` is true.
4. Otherwise use M14C.

This order reflects who physically controls the railway rather than a faction score.

## Political Endings

Endings consume no call slot. Money and rating threshold endings may interrupt the story earlier.

### Emergency State

M14P connects the Station to the Secretariat after Home Affairs prepares the detainee transfer. Order 17-A becomes the official version. Mass arrests continue. Brigade attacks are described as spontaneous patriotic violence. Nobody proves who wrote the order.

### Temporary Command

The Army controls Radio and Central Station. Arman suspends both versions of Order 17 while ruling under his own emergency declaration. Brigade attacks fall where soldiers intervene, but detainees remain in military custody. The Army calls this temporary and gives no date for returning power.

### Platform Six

Workers and Riverland organizers hold the Station long enough to move threatened families. They control several neighborhoods at dawn but do not claim to govern North Neeladesh. The ruling party and Army still hold other institutions. Negotiations, retaliation, or civil war may follow after the game ends.

### Fragmented Control

Radio, the Station, Home Affairs, and armed groups act under different orders. Arrests, attacks, and street fighting continue without one authority. The president remains alive and silent while every faction claims that its actions prevented something worse.

### Southbound Household

This ending overrides the political epilogue if all conditions are true:

- `SOUTH_EXIT_OFFER` was earned with accurate information.
- The operator did not report Meera's offer to Police.
- Platform Six remains open after M14.
- The operator accepts the papers after the final call.

An Embassy bus collects the operator's wife at the Hospital and waits for him outside the ministry after his final call. His wife agrees to use the papers, and the household leaves through Platform Six on the southern convoy. South Neeladesh uses his information to choose which northern faction to support. His family reaches safety, but he never learns whether he helped a rescue, an intervention, or the beginning of a dependent government.

## Outcome Layers

The political ending states who controls communications and movement at dawn. Separate epilogue lines report the human result:

- How many Schedule R-12 arrests succeeded.
- Whether the Brigade received accurate targets and patrol gaps.
- Whether Laleh was arrested, wounded, saved, or evacuated.
- Whether Wagon 43 reached the Brigade, Army, Police, or workers.
- Whether the operator's wife kept her protected Hospital bed.

These variations do not create additional named endings. They stop each political ending from erasing the consequences of individual calls.
