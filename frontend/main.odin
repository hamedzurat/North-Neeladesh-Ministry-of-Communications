// Cabinet Frontend: responsive device mechanics and rendering for Rust authority output.
package switchboard_prototype

import "core:fmt"
import "core:math"
import "core:encoding/json"
import "core:net"
import "core:strings"
import "core:time"
import rl "vendor:raylib"

WINDOW_W :: 1440
WINDOW_H :: 900
LINE_COUNT :: 16
ENDPOINT_COUNT :: 22
CORD_COUNT :: 8
ACTION_COUNT :: 6

INK :: rl.Color{235, 235, 229, 255}
MUTED :: rl.Color{151, 157, 165, 255}
BACKGROUND :: rl.Color{15, 17, 20, 255}
PANEL :: rl.Color{28, 31, 36, 255}
CONTROL :: rl.Color{39, 44, 51, 255}
BORDER :: rl.Color{70, 77, 87, 255}
AMBER :: rl.Color{244, 188, 67, 255}
GREEN :: rl.Color{82, 202, 132, 255}
BLUE :: rl.Color{92, 174, 239, 255}
BLACK :: rl.Color{6, 8, 10, 255}
PAPER :: rl.Color{240, 239, 228, 255}
PAPER_INK :: rl.Color{28, 37, 30, 255}

FONT: rl.Font
CAMERA: rl.Camera2D

LINE_LABELS := [LINE_COUNT]cstring {
	"RAIL DISPATCH",
	"KHARAD CLINIC",
	"RATION OFFICE",
	"FIRE STATION",
	"FOUNDRY APTS",
	"BORDER POST",
	"LABOUR OFFICE",
	"MINISTRY DESK",
	"RIVER MARKET",
	"GRAND HOTEL",
	"POLICE POST",
	"STEEL WORKS",
	"PHARMACY",
	"FREIGHT YARD",
	"POST OFFICE",
	"EXCHANGE ANNEX",
}

DIGIT_TEXT := [10]cstring{"0", "1", "2", "3", "4", "5", "6", "7", "8", "9"}

CORD_COLORS := [CORD_COUNT]rl.Color {
	{220, 105, 80, 255},
	{224, 179, 67, 255},
	{96, 174, 224, 255},
	{102, 190, 126, 255},
	{164, 127, 205, 255},
	{213, 124, 165, 255},
	{99, 194, 179, 255},
	{202, 148, 91, 255},
}

ACTION_LABELS := [ACTION_COUNT]cstring {
	"OPERATOR",
	"POLICE",
	"EMS",
	"FIRE",
	"TAP 1 LISTEN",
	"TAP 2 LISTEN",
}

Cord :: struct {
	a, b:   int,
	active: bool,
}

Backend_Output :: struct {
	sequence:       u64,
	phase:          string,
	health:         string,
	reset_status:   string,
	routing_status: string,
	line_lamps:     [LINE_COUNT]bool,
	directory:      string,
	printer:        string,
	monitor_active: bool,
	speaker_active: bool,
}

App_State :: struct {
	cords:            [CORD_COUNT]Cord,
	dragging_cord:    bool,
	drag_start:       int,
	active_action:    int,
	digits:           [4]int,
	lookup_delay:     f32,
	tuning:           [2]f32,
	crank_fill:       f32,
	crank_flash:      f32,
	work_minutes:     f32,
	speaker_playing:  bool,
	speaker_phase:    f32,
	receipt_revealed: f32,
	receipt_scroll:   f32,
	receipt_dragging: bool,
	line_lamps:       [LINE_COUNT]bool,
	backend_sequence: u64,
	backend_timer:    f32,
	backend_online:   bool,
	backend_phase:    string,
	backend_health:   string,
	reset_status:     string,
	routing_status:   string,
	directory_page:   string,
	printer_feed:     string,
	reset_requested:  bool,
	speaker_enabled:  bool,
}

rect :: proc(x, y, width, height: f32) -> rl.Rectangle {
	return rl.Rectangle{x, y, width, height}
}

point :: proc(x, y: f32) -> rl.Vector2 {
	return rl.Vector2{x, y}
}

contains :: proc(area: rl.Rectangle, position: rl.Vector2) -> bool {
	return rl.CheckCollisionPointRec(position, area)
}

ui_mouse :: proc() -> rl.Vector2 {
	return rl.GetScreenToWorld2D(rl.GetMousePosition(), CAMERA)
}

draw_text :: proc(text: cstring, x, y: f32, size: f32 = 16, color: rl.Color = INK) {
	rl.DrawTextEx(FONT, text, point(x, y), size, 0.2, color)
}

measure_text :: proc(text: cstring, size: f32) -> f32 {
	return rl.MeasureTextEx(FONT, text, size, 0.2).x
}

draw_panel :: proc(area: rl.Rectangle) {
	rl.DrawRectangleRounded(area, 0.025, 4, PANEL)
	rl.DrawRectangleRoundedLinesEx(area, 0.025, 4, 1.2, BORDER)
}

draw_button :: proc(area: rl.Rectangle, label: cstring, size: f32 = 15, active := false) {
	fill := active ? rl.Color{91, 69, 27, 255} : CONTROL
	rl.DrawRectangleRounded(area, 0.1, 4, fill)
	rl.DrawRectangleRoundedLinesEx(area, 0.1, 4, 1.2, active ? AMBER : BORDER)
	width := measure_text(label, size)
	draw_text(label, area.x + (area.width - width) / 2, area.y + (area.height - size) / 2, size)
}

draw_jack :: proc(position: rl.Vector2, color := rl.Color{143, 151, 161, 255}) {
	rl.DrawCircleV(position, 18, color)
	rl.DrawCircleV(position, 12, BLACK)
	rl.DrawCircleV(position, 4, rl.Color{38, 43, 49, 255})
}

draw_lamp :: proc(position: rl.Vector2, lit := false) {
	rl.DrawCircleV(position, 9, lit ? AMBER : rl.Color{72, 65, 42, 255})
	if lit do rl.DrawCircleLines(i32(position.x), i32(position.y), 13, rl.Color{244, 188, 67, 120})
}

draw_subscriber_lines :: proc(state: ^App_State, jacks: ^[ENDPOINT_COUNT]rl.Vector2) {
	area := rect(20, 20, 900, 490)
	draw_panel(area)
	cell_width: f32 = 98
	cell_height: f32 = 220
	gap: f32 = 11
	start_x := area.x + 17
	start_y := area.y + 19

	for index in 0 ..< LINE_COUNT {
		column := index % 8
		row := index / 8
		cell := rect(
			start_x + f32(column) * (cell_width + gap),
			start_y + f32(row) * (cell_height + gap),
			cell_width,
			cell_height,
		)
		rl.DrawRectangleRounded(cell, 0.04, 3, CONTROL)
		rl.DrawRectangleRoundedLinesEx(cell, 0.04, 3, 1, BORDER)
		label_width := measure_text(LINE_LABELS[index], 10)
		draw_text(LINE_LABELS[index], cell.x + (cell.width - label_width) / 2, cell.y + 16, 10)
		lamp_position := point(cell.x + cell.width / 2, cell.y + 99)
		jack_position := point(cell.x + cell.width / 2, cell.y + 170)
		draw_lamp(lamp_position, state.line_lamps[index])
		draw_jack(jack_position)
		jacks[index] = jack_position
	}
}

draw_exchange_ports :: proc(jacks: ^[ENDPOINT_COUNT]rl.Vector2) {
	area := rect(20, 525, 900, 120)
	draw_panel(area)
	group_centers := [4]f32{135, 310, 530, 755}
	labels := [4]cstring{"OPERATOR", "RING GENERATOR", "TAP BRIDGE 1", "TAP BRIDGE 2"}
	for label, index in labels {
		center_x := area.x + group_centers[index]
		label_width := measure_text(label, 12)
		draw_text(label, center_x - label_width / 2, area.y + 20, 12, MUTED)
	}

	jacks[16] = point(area.x + group_centers[0], area.y + 77)
	jacks[17] = point(area.x + group_centers[1], area.y + 77)
	jacks[18] = point(area.x + group_centers[2] - 29, area.y + 77)
	jacks[19] = point(area.x + group_centers[2] + 29, area.y + 77)
	jacks[20] = point(area.x + group_centers[3] - 29, area.y + 77)
	jacks[21] = point(area.x + group_centers[3] + 29, area.y + 77)
	for index in 16 ..< ENDPOINT_COUNT do draw_jack(jacks[index])
}

action_rectangles :: proc() -> [ACTION_COUNT]rl.Rectangle {
	area := rect(20, 660, 900, 70)
	gap: f32 = 7
	button_width := (area.width - 28 - gap * 5) / 6
	result: [ACTION_COUNT]rl.Rectangle
	for index in 0 ..< ACTION_COUNT {
		result[index] = rect(
			area.x + 14 + f32(index) * (button_width + gap),
			area.y + 14,
			button_width,
			42,
		)
	}
	return result
}

draw_action_row :: proc(state: ^App_State) {
	area := rect(20, 660, 900, 70)
	draw_panel(area)
	buttons := action_rectangles()
	mouse := ui_mouse()

	if rl.IsMouseButtonPressed(.LEFT) {
		for button_area, index in buttons {
			if contains(button_area, mouse) {
				state.active_action = index
				break
			}
		}
	}
	if rl.IsMouseButtonReleased(.LEFT) || !rl.IsMouseButtonDown(.LEFT) do state.active_action = -1

	for button_area, index in buttons {
		draw_button(button_area, ACTION_LABELS[index], 11, state.active_action == index)
	}
}

draw_slider :: proc(value: ^f32, area: rl.Rectangle, label: cstring) {
	draw_text(label, area.x, area.y, 13, MUTED)
	track_y := area.y + 32
	rl.DrawRectangleRounded(rect(area.x, track_y - 3, area.width, 6), 0.8, 4, BORDER)
	knob_x := area.x + area.width * value^
	rl.DrawCircleV(point(knob_x, track_y), 12, AMBER)
	if rl.IsMouseButtonDown(.LEFT) &&
	   contains(rect(area.x, area.y + 13, area.width, 38), ui_mouse()) {
		value^ = clamp((ui_mouse().x - area.x) / area.width, 0, 1)
	}
}

draw_crank :: proc(state: ^App_State, area: rl.Rectangle, delta: f32) {
	mouse := ui_mouse()
	wheel := rl.GetMouseWheelMove()
	if contains(area, mouse) && wheel != 0 {
		movement := wheel
		if movement < 0 do movement = -movement
		state.crank_fill += movement * 0.11
		if state.crank_fill >= 1 {
			state.crank_fill = 0
			state.crank_flash = 0.35
		}
	} else {
		state.crank_fill = max(0, state.crank_fill - delta * 0.055)
	}
	state.crank_flash = max(0, state.crank_flash - delta)

	fill := state.crank_flash > 0 ? rl.Color{97, 177, 232, 255} : CONTROL
	rl.DrawRectangleRounded(area, 0.08, 4, fill)
	inner := rect(area.x + 5, area.y + 5, area.width - 10, area.height - 10)
	water_height := inner.height * state.crank_fill
	rl.DrawRectangleRec(
		rect(inner.x, inner.y + inner.height - water_height, inner.width, water_height),
		rl.Color{70, 154, 218, 210},
	)
	rl.DrawRectangleRoundedLinesEx(area, 0.08, 4, 1.2, state.crank_flash > 0 ? BLUE : BORDER)
	label_width := measure_text("SCROLL TO CRANK", 13)
	draw_text(
		"SCROLL TO CRANK",
		area.x + (area.width - label_width) / 2,
		area.y + area.height / 2 - 7,
		13,
	)
}

draw_manual_controls :: proc(state: ^App_State, delta: f32) {
	area := rect(20, 745, 900, 135)
	draw_panel(area)
	draw_crank(state, rect(area.x + 16, area.y + 21, 160, 93), delta)
	draw_slider(&state.tuning[0], rect(area.x + 210, area.y + 20, 658, 36), "TUNING 1")
	draw_slider(&state.tuning[1], rect(area.x + 210, area.y + 77, 658, 36), "TUNING 2")
}

SEGMENTS := [10][7]bool {
	{true, true, true, true, true, true, false},
	{false, true, true, false, false, false, false},
	{true, true, false, true, true, false, true},
	{true, true, true, true, false, false, true},
	{false, true, true, false, false, true, true},
	{true, false, true, true, false, true, true},
	{true, false, true, true, true, true, true},
	{true, true, true, false, false, false, false},
	{true, true, true, true, true, true, true},
	{true, true, true, true, false, true, true},
}

draw_seven_digit :: proc(digit: int, origin: rl.Vector2, scale: f32) {
	t := 6 * scale
	w := 34 * scale
	h := 34 * scale
	areas := [7]rl.Rectangle {
		rect(origin.x + t, origin.y, w, t),
		rect(origin.x + t + w, origin.y + t, t, h),
		rect(origin.x + t + w, origin.y + h + t * 2, t, h),
		rect(origin.x + t, origin.y + h * 2 + t * 2, w, t),
		rect(origin.x, origin.y + h + t * 2, t, h),
		rect(origin.x, origin.y + t, t, h),
		rect(origin.x + t, origin.y + h + t, w, t),
	}
	for area, index in areas {
		color := SEGMENTS[digit][index] ? AMBER : rl.Color{63, 53, 31, 255}
		rl.DrawRectangleRounded(area, 0.35, 3, color)
	}
}

draw_shift_time :: proc(state: ^App_State, delta: f32) {
	area := rect(940, 20, 480, 145)
	draw_panel(area)
	state.work_minutes = min(17 * 60, state.work_minutes + delta)
	total_minutes := int(state.work_minutes)
	hour := total_minutes / 60
	minute := total_minutes % 60
	digits := [4]int{hour / 10, hour % 10, minute / 10, minute % 10}
	start_x := area.x + 84
	for digit, index in digits {
		x := start_x + f32(index) * 76
		if index >= 2 do x += 18
		draw_seven_digit(digit, point(x, area.y + 25), 1)
	}
	rl.DrawCircleV(point(area.x + 239, area.y + 57), 5, AMBER)
	rl.DrawCircleV(point(area.x + 239, area.y + 86), 5, AMBER)
	status_color := state.backend_online ? GREEN : AMBER
	draw_text(fmt.ctprintf("%s", state.backend_health), area.x + 16, area.y + 116, 11, status_color)
	draw_text(fmt.ctprintf("%s", state.reset_status), area.x + 286, area.y + 116, 11, MUTED)
	draw_text(fmt.ctprintf("%s", state.routing_status), area.x + 16, area.y + 137, 11, AMBER)
}

draw_directory :: proc(state: ^App_State, delta: f32) {
	area := rect(940, 180, 480, 255)
	draw_panel(area)
	left := rect(area.x + 14, area.y + 18, 218, 219)
	right := rect(area.x + 248, area.y + 18, 218, 219)
	rl.DrawRectangleRounded(left, 0.04, 3, CONTROL)
	rl.DrawRectangleRec(right, PAPER)

	state.lookup_delay = max(0, state.lookup_delay - delta)
	for digit, index in state.digits {
		x := left.x + 12 + f32(index) * 51
		up := rect(x, left.y + 10, 42, 28)
		down := rect(x, left.y + 106, 42, 28)
		if rl.IsMouseButtonPressed(.LEFT) && contains(up, ui_mouse()) {
			state.digits[index] = (digit + 1) % 10
			state.lookup_delay = 0.5
		}
		if rl.IsMouseButtonPressed(.LEFT) && contains(down, ui_mouse()) {
			state.digits[index] = (state.digits[index] + 9) % 10
			state.lookup_delay = 0.5
		}
		draw_button(up, "+")
		rl.DrawRectangleRec(rect(x, left.y + 48, 42, 48), BLACK)
		text := DIGIT_TEXT[state.digits[index]]
		width := measure_text(text, 25)
		draw_text(text, x + (42 - width) / 2, left.y + 58, 25, GREEN)
		draw_button(down, "-")
	}

	if state.lookup_delay > 0 {
		draw_text("SEARCHING", right.x + 14, right.y + 18, 17, PAPER_INK)
		return
	}
	directory_lines := strings.split(state.directory_page, "|", context.temp_allocator)
	for line, index in directory_lines {
		if index >= 4 do break
		draw_text(fmt.ctprintf("%s", line), right.x + 14, right.y + 18 + f32(index) * 31, index == 0 ? 15 : 11, PAPER_INK)
	}
}

draw_speaker :: proc(state: ^App_State, delta: f32) {
	area := rect(940, 450, 480, 70)
	draw_panel(area)
	if rl.IsMouseButtonPressed(.LEFT) && contains(area, ui_mouse()) do state.speaker_enabled = !state.speaker_enabled
	if state.speaker_playing do state.speaker_phase += delta * 7
	bar_width: f32 = 12
	for index in 0 ..< 24 {
		height: f32 = 5
		if state.speaker_playing {
			wave := math.sin_f32(state.speaker_phase + f32(index) * 0.73)
			if wave < 0 do wave = -wave
			height = 7 + wave * 26
		}
		x := area.x + 18 + f32(index) * 18
		rl.DrawRectangleRounded(
			rect(x, area.y + 57 - height, bar_width, height),
			0.3,
			3,
			state.speaker_playing ? BLUE : BORDER,
		)
	}
}

draw_printer :: proc(state: ^App_State, delta: f32) {
	area := rect(940, 535, 480, 345)
	draw_panel(area)
	paper := rect(area.x + 24, area.y + 18, area.width - 48, area.height - 36)
	rl.DrawRectangleRec(paper, PAPER)

	printer_lines := strings.split(state.printer_feed, "|", context.temp_allocator)
	if len(printer_lines) == 0 do return
	state.receipt_revealed = min(f32(len(printer_lines)), state.receipt_revealed + delta * 2.6)
	revealed := int(state.receipt_revealed)
	visible_lines := 13
	max_scroll := max(0, revealed - visible_lines)
	printing := revealed < len(printer_lines)
	if printing {
		state.receipt_scroll = f32(max_scroll)
	} else if contains(paper, ui_mouse()) {
		state.receipt_scroll = clamp(
			state.receipt_scroll - rl.GetMouseWheelMove() * 2,
			0,
			f32(max_scroll),
		)
	}

	track := rect(paper.x + paper.width - 12, paper.y + 8, 6, paper.height - 16)
	thumb := track
	if revealed > visible_lines {
		thumb.height = max(34, track.height * f32(visible_lines) / f32(revealed))
		travel := track.height - thumb.height
		thumb.y = track.y + travel * state.receipt_scroll / f32(max_scroll)
		if rl.IsMouseButtonPressed(.LEFT) && contains(thumb, ui_mouse()) do state.receipt_dragging = true
		if rl.IsMouseButtonReleased(.LEFT) do state.receipt_dragging = false
		if state.receipt_dragging && rl.IsMouseButtonDown(.LEFT) {
			ratio := clamp((ui_mouse().y - track.y - thumb.height / 2) / max(1, travel), 0, 1)
			state.receipt_scroll = ratio * f32(max_scroll)
			thumb.y = track.y + travel * ratio
		}
		rl.DrawRectangleRounded(track, 0.8, 3, rl.Color{190, 190, 180, 255})
		rl.DrawRectangleRounded(thumb, 0.8, 3, rl.Color{105, 109, 104, 255})
	}

	start_line := int(state.receipt_scroll)
	end_line := min(revealed, start_line + visible_lines)
	y := paper.y + 14
	for index in start_line ..< end_line {
		draw_text(fmt.ctprintf("%s", printer_lines[index]), paper.x + 16, y, 12, PAPER_INK)
		y += 21
	}
}

snapshot_json :: proc(state: ^App_State) -> string {
	builder := strings.builder_make(context.temp_allocator)
	fmt.sbprintf(&builder, `{"sequence":%d,"cords":[`, state.backend_sequence)
	first := true
	for cord in state.cords {
		if !cord.active do continue
		if !first do strings.write_string(&builder, ",")
		fmt.sbprintf(&builder, "[%d,%d]", cord.a, cord.b)
		first = false
	}
	directory_id := state.digits[0] * 1000 + state.digits[1] * 100 + state.digits[2] * 10 + state.digits[3]
	fmt.sbprintf(
		&builder,
		`],"active_action":%d,"crank_complete":%t,"directory_id":%d,"speaker_enabled":%t,"reset":%t}\n`,
		state.active_action,
		state.crank_flash > 0,
		directory_id,
		state.speaker_enabled,
		state.reset_requested,
	)
	return strings.to_string(builder)
}

reset_cabinet_interactions :: proc(state: ^App_State) {
	for index in 0 ..< CORD_COUNT do state.cords[index] = Cord{}
	state.dragging_cord = false
	state.drag_start = -1
	state.active_action = -1
	state.crank_fill = 0
	state.crank_flash = 0
	state.speaker_enabled = true
}

sync_backend :: proc(state: ^App_State) {
	state.backend_sequence += 1
	socket, dial_error := net.dial_tcp_from_hostname_and_port_string("127.0.0.1:48129")
	if dial_error != nil {
		state.backend_online = false
		state.backend_health = "RUST CORE OFFLINE"
		return
	}
	defer net.close(socket)
	if net.set_option(socket, .Receive_Timeout, time.Millisecond * 250) != nil {
		state.backend_online = false
		state.backend_health = "RUST CORE SOCKET ERROR"
		return
	}
	request := snapshot_json(state)
	_, send_error := net.send_tcp(socket, transmute([]byte)request)
	if send_error != nil {
		state.backend_online = false
		state.backend_health = "RUST CORE SEND ERROR"
		return
	}
	buffer: [8192]byte
	read_count, receive_error := net.recv_tcp(socket, buffer[:])
	if receive_error != nil || read_count == 0 {
		state.backend_online = false
		state.backend_health = "RUST CORE NO RESPONSE"
		return
	}
	output: Backend_Output
	decode_error := json.unmarshal(buffer[:read_count], &output)
	if decode_error != nil {
		state.backend_online = false
		state.backend_health = "RUST CORE PROTOCOL ERROR"
		return
	}
	state.backend_online = true
	state.backend_phase = output.phase
	state.backend_health = output.health
	state.reset_status = output.reset_status
	state.routing_status = output.routing_status
	state.line_lamps = output.line_lamps
	state.directory_page = output.directory
	if output.reset_status == "RESET COMPLETE" {
		reset_cabinet_interactions(state)
		state.receipt_revealed = 0
		state.receipt_scroll = 0
	}
	state.printer_feed = output.printer
	state.speaker_playing = output.speaker_active
	state.reset_requested = false
}

endpoint_cord :: proc(state: ^App_State, endpoint: int) -> int {
	for cord, index in state.cords {
		if cord.active && (cord.a == endpoint || cord.b == endpoint) do return index
	}
	return -1
}

free_cord :: proc(state: ^App_State) -> int {
	for cord, index in state.cords {
		if !cord.active do return index
	}
	return -1
}

endpoint_at :: proc(jacks: ^[ENDPOINT_COUNT]rl.Vector2, position: rl.Vector2) -> int {
	for jack, index in jacks {
		if rl.CheckCollisionPointCircle(position, jack, 23) do return index
	}
	return -1
}

draw_cord :: proc(start, finish: rl.Vector2, color: rl.Color) {
	bend_y := min(650, max(start.y, finish.y) + 42)
	control_1 := point(start.x, bend_y)
	control_2 := point(finish.x, bend_y)
	rl.DrawSplineSegmentBezierCubic(
		start,
		control_1,
		control_2,
		finish,
		11,
		rl.Color{4, 5, 7, 230},
	)
	rl.DrawSplineSegmentBezierCubic(start, control_1, control_2, finish, 6, color)
}

draw_cords_and_handle_input :: proc(state: ^App_State, jacks: ^[ENDPOINT_COUNT]rl.Vector2) {
	mouse := ui_mouse()
	hovered_endpoint := endpoint_at(jacks, mouse)

	if rl.IsMouseButtonPressed(.RIGHT) {
		if hovered_endpoint >= 0 {
			cord_index := endpoint_cord(state, hovered_endpoint)
			if cord_index >= 0 do state.cords[cord_index].active = false
		}
		state.dragging_cord = false
		state.drag_start = -1
	}

	if rl.IsMouseButtonPressed(.LEFT) &&
	   hovered_endpoint >= 0 &&
	   endpoint_cord(state, hovered_endpoint) < 0 &&
	   free_cord(state) >= 0 {
		state.dragging_cord = true
		state.drag_start = hovered_endpoint
	}

	if rl.IsMouseButtonReleased(.LEFT) && state.dragging_cord {
		if hovered_endpoint >= 0 &&
		   hovered_endpoint != state.drag_start &&
		   endpoint_cord(state, hovered_endpoint) < 0 {
			cord_index := free_cord(state)
			if cord_index >= 0 do state.cords[cord_index] = Cord{state.drag_start, hovered_endpoint, true}
		}
		state.dragging_cord = false
		state.drag_start = -1
	}

	for cord, index in state.cords {
		if cord.active do draw_cord(jacks[cord.a], jacks[cord.b], CORD_COLORS[index])
	}
	if state.dragging_cord && state.drag_start >= 0 {
		draw_cord(jacks[state.drag_start], mouse, CORD_COLORS[free_cord(state)])
	}

	for position, endpoint in jacks {
		cord_index := endpoint_cord(state, endpoint)
		color := rl.Color{143, 151, 161, 255}
		if cord_index >= 0 do color = CORD_COLORS[cord_index]
		if state.dragging_cord && state.drag_start == endpoint do color = AMBER
		draw_jack(position, color)
	}
}

update_camera :: proc() {
	width_scale := f32(rl.GetScreenWidth()) / f32(WINDOW_W)
	height_scale := f32(rl.GetScreenHeight()) / f32(WINDOW_H)
	scale := min(width_scale, height_scale)
	CAMERA = rl.Camera2D {
		offset = point(
			(f32(rl.GetScreenWidth()) - f32(WINDOW_W) * scale) / 2,
			(f32(rl.GetScreenHeight()) - f32(WINDOW_H) * scale) / 2,
		),
		target = point(0, 0),
		zoom   = scale,
	}
}

initial_state :: proc() -> App_State {
	state := App_State {
		drag_start      = -1,
		active_action   = -1,
		digits          = {4, 1, 0, 1},
		tuning          = {0.64, 0.38},
		work_minutes    = 9 * 60,
		backend_health  = "RUST CORE STARTING",
		reset_status    = "PRESS R TO RESET",
		routing_status  = "WAITING FOR RUST CORE",
		directory_page  = "SEARCHING",
		printer_feed    = "WAITING FOR RUST CORE",
		speaker_enabled = true,
	}
	return state
}

main :: proc() {
	rl.SetConfigFlags({.WINDOW_HIGHDPI, .BORDERLESS_WINDOWED_MODE})
	rl.InitWindow(WINDOW_W, WINDOW_H, "North Neeladesh")
	defer rl.CloseWindow()
	monitor := rl.GetCurrentMonitor()
	rl.SetWindowPosition(0, 0)
	rl.SetWindowSize(rl.GetMonitorWidth(monitor), rl.GetMonitorHeight(monitor))

	FONT = rl.LoadFontEx("/usr/share/fonts/TTF/IosevkaNerdFontMono-Regular.ttf", 32, nil, 0)
	if !rl.IsFontValid(FONT) do FONT = rl.GetFontDefault()
	defer if FONT.texture.id != rl.GetFontDefault().texture.id do rl.UnloadFont(FONT)
	rl.SetTextureFilter(FONT.texture, .BILINEAR)
	rl.SetTargetFPS(60)
	state := initial_state()

	for !rl.WindowShouldClose() {
		update_camera()
		delta := rl.GetFrameTime()
		jacks: [ENDPOINT_COUNT]rl.Vector2

		rl.BeginDrawing()
		rl.ClearBackground(BACKGROUND)
		rl.BeginMode2D(CAMERA)
		draw_subscriber_lines(&state, &jacks)
		draw_exchange_ports(&jacks)
		draw_action_row(&state)
		draw_manual_controls(&state, delta)
		draw_shift_time(&state, delta)
		draw_directory(&state, delta)
		draw_speaker(&state, delta)
		draw_printer(&state, delta)
		draw_cords_and_handle_input(&state, &jacks)
		if rl.IsKeyPressed(.R) {
			reset_cabinet_interactions(&state)
			state.reset_requested = true
		}
		state.backend_timer -= delta
		if state.backend_timer <= 0 {
			sync_backend(&state)
			state.backend_timer = 0.15
		}
		rl.EndMode2D()
		rl.EndDrawing()
		free_all(context.temp_allocator)
	}
}
