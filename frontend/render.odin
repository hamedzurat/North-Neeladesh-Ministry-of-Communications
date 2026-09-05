package frontend

import "core:fmt"
import "core:math"
import "core:strings"
import rl "vendor:raylib"

// The reference cabinet is drawn on a fixed logical canvas and letterboxed on
// smaller or wider windows. Input is authored locally, but every visible value
// below comes from the latest complete backend output.
ref_rect :: proc(x, y, width, height: f32) -> rl.Rectangle { return rl.Rectangle{x, y, width, height} }
ref_point :: proc(x, y: f32) -> rl.Vector2 { return rl.Vector2{x, y} }
ref_contains :: proc(area: rl.Rectangle, position: rl.Vector2) -> bool { return rl.CheckCollisionPointRec(position, area) }
ref_mouse :: proc() -> rl.Vector2 { return rl.GetScreenToWorld2D(rl.GetMousePosition(), CAMERA) }

ref_text :: proc(value: string, x, y, size: f32, color: rl.Color = INK) {
	rendered := strings.clone_to_cstring(value, context.temp_allocator) or_else cstring("")
	rl.DrawTextEx(FONT, rendered, ref_point(x, y), size, 0.2, color)
}

ref_text_c :: proc(value: cstring, x, y, size: f32, color: rl.Color = INK) {
	rl.DrawTextEx(FONT, value, ref_point(x, y), size, 0.2, color)
}

ref_measure :: proc(value: cstring, size: f32) -> f32 { return rl.MeasureTextEx(FONT, value, size, 0.2).x }

ref_panel :: proc(area: rl.Rectangle) {
	rl.DrawRectangleRounded(area, 0.025, 4, PANEL)
	rl.DrawRectangleRoundedLinesEx(area, 0.025, 4, 1.2, BORDER)
}

ref_button :: proc(area: rl.Rectangle, title: cstring, size: f32 = 15, active := false) {
	fill := active ? rl.Color{91, 69, 27, 255} : CONTROL
	rl.DrawRectangleRounded(area, 0.1, 4, fill)
	rl.DrawRectangleRoundedLinesEx(area, 0.1, 4, 1.2, active ? AMBER : BORDER)
	width := ref_measure(title, size)
	ref_text_c(title, area.x + (area.width - width) / 2, area.y + (area.height - size) / 2, size)
}

ref_draw_jack :: proc(position: rl.Vector2, color := rl.Color{143, 151, 161, 255}) {
	rl.DrawCircleV(position, 18, color)
	rl.DrawCircleV(position, 12, BLACK)
	rl.DrawCircleV(position, 4, rl.Color{38, 43, 49, 255})
}

ref_draw_lamp :: proc(position: rl.Vector2, lit := false) {
	rl.DrawCircleV(position, 9, lit ? AMBER : rl.Color{72, 65, 42, 255})
	if lit do rl.DrawCircleLines(i32(position.x), i32(position.y), 13, rl.Color{244, 188, 67, 120})
}

ref_draw_subscriber_lines :: proc(app: ^Input_State, jacks: ^[ENDPOINT_COUNT]rl.Vector2) {
	area := ref_rect(20, 20, 900, 490)
	ref_panel(area)
	cell_width: f32 = 98
	cell_height: f32 = 220
	gap: f32 = 11
	start_x := area.x + 17
	start_y := area.y + 19

	for index in 0 ..< LINE_COUNT {
		column := index % 8
		row := index / 8
		cell := ref_rect(start_x + f32(column) * (cell_width + gap), start_y + f32(row) * (cell_height + gap), cell_width, cell_height)
		rl.DrawRectangleRounded(cell, 0.04, 3, CONTROL)
		rl.DrawRectangleRoundedLinesEx(cell, 0.04, 3, 1, BORDER)
		label_width := ref_measure(REF_LINE_LABELS[index], 10)
		ref_text_c(REF_LINE_LABELS[index], cell.x + (cell.width - label_width) / 2, cell.y + 16, 10)
		lamp_position := ref_point(cell.x + cell.width / 2, cell.y + 99)
		jack_position := ref_point(cell.x + cell.width / 2, cell.y + 170)
		ref_draw_lamp(lamp_position, app.backend_output.line_lamps[index])
		ref_draw_jack(jack_position)
		jacks[index] = jack_position
	}
}

ref_tap_position :: proc(index: int) -> rl.Vector2 {
	bridge := index / 2
	side := index % 2
	centers := [2]f32{400, 700}
	return ref_point(20 + centers[bridge] + (f32(side) * 2 - 1) * 20, 602)
}

ref_draw_exchange_ports :: proc(jacks: ^[ENDPOINT_COUNT]rl.Vector2) {
	area := ref_rect(20, 525, 900, 120)
	ref_panel(area)
	ref_text_c("OPERATOR", 85, area.y + 20, 12, MUTED)
	ref_text_c("RING GENERATOR", 210, area.y + 20, 12, MUTED)
	jacks[16] = ref_point(120, area.y + 77)
	jacks[17] = ref_point(270, area.y + 77)
	ref_draw_jack(jacks[16])
	ref_draw_jack(jacks[17])
	centers := [2]f32{400, 700}
	for index in 0 ..< 2 {
		center_x := 20 + centers[index]
		ref_text(fmt.tprintf("TAP BRIDGE %d", index + 1), center_x - 45, area.y + 20, 11, MUTED)
		first := ref_tap_position(index * 2)
		second := ref_tap_position(index * 2 + 1)
		jacks[18 + index * 2] = first
		jacks[19 + index * 2] = second
		rl.DrawLineEx(first, second, 3, BORDER)
		ref_draw_jack(first)
		ref_draw_jack(second)
	}
	for color, index in REF_CORD_COLORS {
		x := area.x + 342 + f32(index) * 64
		rl.DrawRectangleRounded(ref_rect(x, area.y + 105, 42, 5), 0.8, 3, color)
	}
}

ref_action_rectangles :: proc() -> [ACTION_COUNT]rl.Rectangle {
	area := ref_rect(20, 660, 900, 70)
	gap: f32 = 5
	button_width := (area.width - 28 - gap * f32(ACTION_COUNT - 1)) / f32(ACTION_COUNT)
	result: [ACTION_COUNT]rl.Rectangle
	for index in 0 ..< ACTION_COUNT {
		result[index] = ref_rect(area.x + 14 + f32(index) * (button_width + gap), area.y + 14, button_width, 42)
	}
	return result
}

ref_action_active :: proc(app: ^Input_State, index: int) -> bool {
	if slot := ref_action_slot(&app.intent.held_controls, index); slot != nil {
		return slot^
	}
	return false
}

ref_draw_action_row :: proc(app: ^Input_State) {
	area := ref_rect(20, 660, 900, 70)
	ref_panel(area)
	buttons := ref_action_rectangles()
	for button_area, index in buttons { ref_button(button_area, REF_ACTION_LABELS[index], 10, ref_action_active(app, index)) }
}

ref_draw_crank :: proc(app: ^Input_State, area: rl.Rectangle) {
	fill := app.crank_flash > 0 ? rl.Color{97, 177, 232, 255} : CONTROL
	rl.DrawRectangleRounded(area, 0.08, 4, fill)
	inner := ref_rect(area.x + 5, area.y + 5, area.width - 10, area.height - 10)
	water_height := inner.height * app.crank_fill
	rl.DrawRectangleRec(ref_rect(inner.x, inner.y + inner.height - water_height, inner.width, water_height), rl.Color{70, 154, 218, 210})
	rl.DrawRectangleRoundedLinesEx(area, 0.08, 4, 1.2, app.crank_flash > 0 ? BLUE : BORDER)
	ref_text_c("SCROLL TO CRANK", area.x + 21, area.y + area.height / 2 - 7, 13)
}

ref_draw_slider :: proc(value: u16, area: rl.Rectangle, title: cstring) {
	ref_text_c(title, area.x, area.y, 13, MUTED)
	track_y := area.y + 32
	rl.DrawRectangleRounded(ref_rect(area.x, track_y - 3, area.width, 6), 0.8, 4, BORDER)
	knob_x := area.x + area.width * f32(value) / 1023
	rl.DrawCircleV(ref_point(knob_x, track_y), 12, AMBER)
}

ref_draw_manual_controls :: proc(app: ^Input_State) {
	area := ref_rect(20, 745, 900, 135)
	ref_panel(area)
	crank_area := ref_rect(area.x + 16, area.y + 21, 160, 93)
	ref_draw_crank(app, crank_area)
	ref_draw_slider(app.backend_output.tuning.coarse, ref_rect(area.x + 210, area.y + 20, 658, 36), "TUNING 1")
	ref_draw_slider(app.backend_output.tuning.fine, ref_rect(area.x + 210, area.y + 77, 658, 36), "TUNING 2")
}

ref_draw_seven_digit :: proc(digit: int, origin: rl.Vector2, scale: f32) {
	t := 6 * scale
	w := 34 * scale
	h := 34 * scale
	areas := [7]rl.Rectangle {
		ref_rect(origin.x + t, origin.y, w, t), ref_rect(origin.x + t + w, origin.y + t, t, h),
		ref_rect(origin.x + t + w, origin.y + h + t * 2, t, h), ref_rect(origin.x + t, origin.y + h * 2 + t * 2, w, t),
		ref_rect(origin.x, origin.y + h + t * 2, t, h), ref_rect(origin.x + t, origin.y + t, t, h),
		ref_rect(origin.x + t, origin.y + h + t, w, t),
	}
	for area, index in areas {
		color := REF_SEGMENTS[digit][index] ? AMBER : rl.Color{63, 53, 31, 255}
		rl.DrawRectangleRounded(area, 0.35, 3, color)
	}
}

ref_draw_clock :: proc(app: ^Input_State) {
	area := ref_rect(940, 20, 480, 145)
	ref_panel(area)
	total_seconds := app.backend_output.elapsed_seconds
	minutes := int(total_seconds / 60) % 100
	seconds := int(total_seconds % 60)
	digits := [4]int{minutes / 10, minutes % 10, seconds / 10, seconds % 10}
	start_x := area.x + 84
	for digit, index in digits {
		x := start_x + f32(index) * 76
		if index >= 2 do x += 18
		ref_draw_seven_digit(digit % 10, ref_point(x, area.y + 25), 1)
	}
	rl.DrawCircleV(ref_point(area.x + 239, area.y + 57), 5, AMBER)
	rl.DrawCircleV(ref_point(area.x + 239, area.y + 86), 5, AMBER)
	ref_text(fmt.tprintf("SHIFT %d // %s", app.backend_output.clock_shift, app.backend_output.game_phase), area.x + 286, area.y + 116, 11, MUTED)
	if call, ok := app.backend_output.call.?; ok {
		ref_text(fmt.tprintf("CALL // %s", call.phase), area.x + 14, area.y + 98, 10, BLUE)
	}
	ref_text(app.status, area.x + 14, area.y + 116, 10, app.connected ? GREEN : AMBER)
}

ref_draw_directory :: proc(app: ^Input_State) {
	area := ref_rect(940, 180, 480, 255)
	ref_panel(area)
	left := ref_rect(area.x + 14, area.y + 18, 218, 219)
	right := ref_rect(area.x + 248, area.y + 18, 218, 219)
	rl.DrawRectangleRounded(left, 0.04, 3, CONTROL)
	rl.DrawRectangleRec(right, PAPER)
	for digit, index in app.displayed_directory_digits {
		x := left.x + 12 + f32(index) * 51
		rl.DrawRectangleRounded(ref_rect(x, left.y + 10, 42, 28), 0.1, 4, CONTROL)
		rl.DrawRectangleRounded(ref_rect(x, left.y + 106, 42, 28), 0.1, 4, CONTROL)
		ref_text(fmt.tprintf("%d", digit), x + 15, left.y + 55, 25, GREEN)
		ref_text_c("+", x + 16, left.y + 14, 15)
		ref_text_c("-", x + 17, left.y + 110, 15)
	}
	if len(app.backend_output.directory_pages) == 0 {
		ref_text_c("SEARCHING", right.x + 14, right.y + 18, 17, PAPER_INK)
		return
	}
	y := right.y + 18
	for page, page_index in app.backend_output.directory_pages {
		ref_text(fmt.tprintf("PAGE %d // %s", page.page_number, page.heading), right.x + 14, y, 12, PAPER_INK)
		y += 28
		for line in page.lines {
			if y > right.y + right.height - 18 do break
			ref_text(line, right.x + 14, y, 11, PAPER_INK)
			y += 23
		}
		if page_index + 1 < len(app.backend_output.directory_pages) do y += 7
	}
}

ref_directory_digit_buttons :: proc(index: int) -> (rl.Rectangle, rl.Rectangle) {
	left := ref_rect(940 + 14, 180 + 18, 218, 219)
	x := left.x + 12 + f32(index) * 51
	return ref_rect(x, left.y + 10, 42, 28), ref_rect(x, left.y + 106, 42, 28)
}

ref_draw_speaker :: proc(app: ^Input_State, delta: f32) {
	area := ref_rect(940, 450, 480, 70)
	ref_panel(area)
	active := app.backend_output.speaker_active
	if active do app.speaker_phase += delta * 7
	for index in 0 ..< 24 {
		height: f32 = 5
		if active {
			wave := math.sin_f32(app.speaker_phase + f32(index) * 0.73)
			if wave < 0 do wave = -wave
			height = 5 + 0.65 * (8 + wave * 29)
		}
		x := area.x + 18 + f32(index) * 18
		rl.DrawRectangleRounded(ref_rect(x, area.y + 57 - height, 12, height), 0.3, 3, active ? BLUE : BORDER)
	}
	ref_text(active ? "SPEAKER ACTIVE" : "SPEAKER STANDBY", area.x + 14, area.y + 12, 11, MUTED)
}

ref_draw_printer :: proc(app: ^Input_State) {
	area := ref_rect(940, 535, 480, 345)
	ref_panel(area)
	paper := ref_rect(area.x + 24, area.y + 18, area.width - 48, area.height - 36)
	rl.DrawRectangleRec(paper, PAPER)
	count := len(app.backend_output.printer_output)
	visible_lines := 13
	max_scroll := max(0, count - visible_lines)
	if ref_contains(paper, ref_mouse()) {
		app.receipt_scroll = clamp(app.receipt_scroll - rl.GetMouseWheelMove() * 2, 0, f32(max_scroll))
	}
	app.receipt_scroll = clamp(app.receipt_scroll, 0, f32(max_scroll))
	start_line := int(app.receipt_scroll)
	if start_line < 0 do start_line = 0
	if start_line > max_scroll do start_line = max_scroll
	end_line := min(count, start_line + visible_lines)
	y := paper.y + 14
	for index in start_line ..< end_line {
		entry := app.backend_output.printer_output[index]
		ref_text(fmt.tprintf("%04d  %s", entry.entry_id, entry.text), paper.x + 16, y, 12, PAPER_INK)
		y += 21
	}

	if max_scroll > 0 {
		track := ref_rect(paper.x + paper.width - 12, paper.y + 8, 6, paper.height - 16)
		rl.DrawRectangleRounded(track, 0.8, 4, rl.Color{194, 193, 183, 255})
		thumb_height := max(24, track.height * f32(visible_lines) / f32(count))
		thumb_y := track.y + (track.height - thumb_height) * app.receipt_scroll / f32(max_scroll)
		rl.DrawRectangleRounded(ref_rect(track.x, thumb_y, track.width, thumb_height), 0.8, 4, BLUE)
	}
}

ref_endpoint_at :: proc(jacks: ^[ENDPOINT_COUNT]rl.Vector2, position: rl.Vector2) -> int {
	for jack, index in jacks {
		if rl.CheckCollisionPointCircle(position, jack, 23) do return index
	}
	return -1
}

ref_draw_cord :: proc(start, finish: rl.Vector2, color: rl.Color) {
	bend_y := min(650, max(start.y, finish.y) + 42)
	control_1 := ref_point(start.x, bend_y)
	control_2 := ref_point(finish.x, bend_y)
	rl.DrawSplineSegmentBezierCubic(start, control_1, control_2, finish, 11, rl.Color{4, 5, 7, 230})
	rl.DrawSplineSegmentBezierCubic(start, control_1, control_2, finish, 6, color)
}

ref_draw_cords :: proc(app: ^Input_State, jacks: ^[ENDPOINT_COUNT]rl.Vector2) {
	for cord, index in app.intent.cord_topology {
		first := ref_endpoint_from_port(cord.first)
		second := ref_endpoint_from_port(cord.second)
		if first >= 0 && first < ENDPOINT_COUNT && second >= 0 && second < ENDPOINT_COUNT {
			ref_draw_cord(jacks[first], jacks[second], REF_CORD_COLORS[index % len(REF_CORD_COLORS)])
		}
	}
	if app.dragging && app.drag_endpoint >= 0 {
		ref_draw_cord(jacks[app.drag_endpoint], ref_mouse(), REF_CORD_COLORS[len(app.intent.cord_topology) % len(REF_CORD_COLORS)])
	}
}

ref_redraw_endpoint_jacks :: proc(app: ^Input_State, jacks: ^[ENDPOINT_COUNT]rl.Vector2) {
	for position, endpoint in jacks {
		color := rl.Color{143, 151, 161, 255}
		cord_index := ref_local_endpoint_cord(app, endpoint)
		if cord_index >= 0 do color = REF_CORD_COLORS[cord_index % len(REF_CORD_COLORS)]
		if app.dragging && app.drag_endpoint == endpoint do color = AMBER
		ref_draw_jack(position, color)
	}
}

update_camera :: proc() {
	width_scale := f32(rl.GetScreenWidth()) / f32(WINDOW_W)
	height_scale := f32(rl.GetScreenHeight()) / f32(WINDOW_H)
	scale := min(width_scale, height_scale)
	CAMERA = rl.Camera2D{
		offset = ref_point((f32(rl.GetScreenWidth()) - f32(WINDOW_W) * scale) / 2, (f32(rl.GetScreenHeight()) - f32(WINDOW_H) * scale) / 2),
		target = ref_point(0, 0),
		zoom = scale,
	}
}

draw_reference_switchboard :: proc(app: ^Input_State, delta: f32) {
	if !app.backend_output_ready {
		ref_panel(ref_rect(20, 20, 1400, 860))
		ref_text_c("WAITING FOR AUTHORITATIVE BACKEND STATE", 350, 410, 20, AMBER)
		ref_text(app.status, 350, 450, 13, MUTED)
		return
	}
	jacks: [ENDPOINT_COUNT]rl.Vector2
	ref_draw_subscriber_lines(app, &jacks)
	ref_draw_exchange_ports(&jacks)
	ref_draw_action_row(app)
	ref_draw_manual_controls(app)
	ref_draw_clock(app)
	ref_draw_directory(app)
	ref_draw_speaker(app, delta)
	ref_draw_printer(app)
	ref_draw_cords(app, &jacks)
	ref_redraw_endpoint_jacks(app, &jacks)
	ref_handle_cords(app, &jacks)
}
