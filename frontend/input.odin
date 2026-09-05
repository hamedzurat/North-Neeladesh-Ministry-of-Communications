package frontend

import rl "vendor:raylib"

ref_port_from_endpoint :: proc(endpoint: int) -> Port {
	if endpoint < 16 do return Port{kind = .Subscriber, index = u8(endpoint)}
	if endpoint == 16 do return Port{kind = .Operator}
	if endpoint == 17 do return Port{kind = .Ring_Generator}
	return Port{kind = .Tap_Bridge, index = u8(endpoint - 17)}
}

ref_endpoint_from_port :: proc(port: Port) -> int {
	switch port.kind {
	case .Subscriber: return int(port.index)
	case .Operator: return 16
	case .Ring_Generator: return 17
	case .Tap_Bridge: return 17 + int(port.index)
	}
	return -1
}

ref_local_endpoint_cord :: proc(app: ^Input_State, endpoint: int) -> int {
	for cord, index in app.intent.cord_topology {
		if ref_endpoint_from_port(cord.first) == endpoint || ref_endpoint_from_port(cord.second) == endpoint do return index
	}
	return -1
}

ref_remove_cord :: proc(app: ^Input_State, index: int) {
	if index < 0 || index >= len(app.intent.cord_topology) do return
	copy(app.intent.cord_topology[index:], app.intent.cord_topology[index+1:])
	resize(&app.intent.cord_topology, len(app.intent.cord_topology) - 1)
}

ref_handle_cords :: proc(app: ^Input_State, jacks: ^[ENDPOINT_COUNT]rl.Vector2) {
	mouse := ref_mouse()
	hovered := ref_endpoint_at(jacks, mouse)
	if rl.IsMouseButtonPressed(.RIGHT) {
		cord_index := ref_local_endpoint_cord(app, hovered)
		if cord_index >= 0 do ref_remove_cord(app, cord_index)
		app.dragging = false
		app.drag_endpoint = -1
	}
	if rl.IsMouseButtonPressed(.LEFT) && hovered >= 0 {
		app.dragging = true
		app.drag_endpoint = hovered
	}
	if rl.IsMouseButtonReleased(.LEFT) && app.dragging {
		start := app.drag_endpoint
		app.dragging = false
		app.drag_endpoint = -1
		if hovered >= 0 && hovered != start {
			cord_index := ref_local_endpoint_cord(app, start)
			if cord_index >= 0 {
				cord := app.intent.cord_topology[cord_index]
				other := ref_endpoint_from_port(cord.first)
				if other == start do other = ref_endpoint_from_port(cord.second)
				ref_remove_cord(app, cord_index)
				if hovered == other do return
			}
			if target_cord := ref_local_endpoint_cord(app, hovered); target_cord >= 0 {
				ref_remove_cord(app, target_cord)
			}
			if len(app.intent.cord_topology) < CORD_SLOT_COUNT {
				append(&app.intent.cord_topology, Cord{ref_port_from_endpoint(start), ref_port_from_endpoint(hovered)})
			}
		}
	}
}

ref_action_slot :: proc(held: ^Held, index: int) -> ^bool {
	switch index {
	case 0: return &held.ptt
	case 1: return &held.police
	case 2: return &held.ems
	case 3: return &held.fire
	case 4: return &held.tap_1
	case 5: return &held.tap_2
	}
	return nil
}

ref_set_action_input :: proc(app: ^Input_State, index: int, active: bool) {
	if slot := ref_action_slot(&app.intent.held_controls, index); slot != nil {
		slot^ = active
	}
}

update_reference_controls :: proc(app: ^Input_State, delta: f32) {
	mouse := ref_mouse()
	buttons := ref_action_rectangles()
	if rl.IsMouseButtonPressed(.LEFT) {
		for button_area, index in buttons {
			if ref_contains(button_area, mouse) {
				app.active_action = index
				ref_set_action_input(app, index, true)
				break
			}
		}
	}
	if rl.IsMouseButtonReleased(.LEFT) {
		if app.active_action >= 0 && app.active_action < ACTION_COUNT do ref_set_action_input(app, app.active_action, false)
		app.active_action = -1
	}
	if app.active_action >= 0 && app.active_action < ACTION_COUNT && !rl.IsMouseButtonDown(.LEFT) do app.active_action = -1

	for index in 0 ..< 4 {
		up, down := ref_directory_digit_buttons(index)
		if rl.IsMouseButtonPressed(.LEFT) {
			if ref_contains(up, mouse) {
				app.intent.directory_digits[index] = (app.intent.directory_digits[index] + 1) % 10
			} else if ref_contains(down, mouse) {
				app.intent.directory_digits[index] = (app.intent.directory_digits[index] + 9) % 10
			}
		}
	}
	if rl.IsMouseButtonPressed(.LEFT) && ref_contains(ref_rect(20, 745, 176, 135), mouse) do app.crank_active_until = rl.GetTime() + 0.35
	crank_area := ref_rect(36, 766, 160, 93)
	crank_input := ref_contains(crank_area, mouse) && (rl.IsMouseButtonDown(.LEFT) || rl.GetMouseWheelMove() != 0)
	if crank_input {
		app.crank_active_until = rl.GetTime() + 0.35
		app.crank_fill += max(0.02, abs(rl.GetMouseWheelMove()) * 0.11)
		if app.crank_fill >= 1 {
			app.crank_fill = 0
			for index in 0 ..< 3 { app.intent.crank_rotation_timestamps[index] = app.intent.crank_rotation_timestamps[index + 1] }
			app.intent.crank_rotation_timestamps[3] = u64(rl.GetTime() * 1000) + 1
			app.crank_flash = 0.35
		}
	} else if rl.GetTime() > app.crank_active_until {
		app.crank_fill = max(0, app.crank_fill - delta * 0.055)
	}
	app.crank_flash = max(0, app.crank_flash - delta)

	for index in 0 ..< 2 {
		area := ref_rect(230, 758 + f32(index) * 57, 658, 38)
		if rl.IsMouseButtonDown(.LEFT) && ref_contains(area, mouse) {
			value := clamp((mouse.x - area.x) / area.width, 0, 1) * 1023
			if index == 0 do app.intent.tuning.coarse = u16(value)
			else do app.intent.tuning.fine = u16(value)
		}
	}
}
