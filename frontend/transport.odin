package frontend

import "core:encoding/cbor"
import "core:fmt"
import "core:net"
import "core:os"
import "core:strings"

initial_backend_output :: proc() -> State_Output {
	return State_Output{
		game_phase = "ready",
		clock_shift = 1,
		speaker_active = false,
		tuning = Tuning{},
		shift = Shift{number = 1, phase = "ready"},
	}
}

initial_intent :: proc() -> Input_Intent {
	return Input_Intent{
		directory_digits = [4]u8{0, 0, 0, 1},
		debug = Input_Debug{firmware_version = "odin-raylib"},
	}
}

backend_address :: proc() -> string {
	if configured, ok := os.lookup_env("NN_BACKEND_ADDRESS", context.temp_allocator); ok && configured != "" {
		return configured
	}
	return "127.0.0.1:7878"
}

probe_backend :: proc() -> bool {
	address := backend_address()
	socket, err := net.dial_tcp_from_hostname_and_port_string(address)
	if err != nil {
		fmt.println(fmt.tprintf("BACKEND UNAVAILABLE // %s // start the backend before the frontend", address))
		return false
	}
	net.close(socket)
	return true
}

poll_backend :: proc(app: ^Input_State, now: f64) {
	if !app.connected {
		if now - app.last_connect_attempt < 1.0 { return }
		app.last_connect_attempt = now
		address := backend_address()
		socket, err := net.dial_tcp_from_hostname_and_port_string(address)
		if err != nil {
			set_status(app, fmt.tprintf("BACKEND OFFLINE // %s", address))
			return
		}
		if net.set_blocking(socket, false) != nil {
			net.close(socket)
			set_status(app, "SOCKET SETUP FAILED")
			return
		}
		app.socket = socket
		app.connected = true
		app.intent.debug.transport_connected = true
		set_status(app, "LOOPBACK ONLINE // SYNCHRONIZING")
		app.last_send = 0
		if app.retry_pending {
			app.tx = make([dynamic]u8, len(app.retry_frame))
			copy(app.tx[:], app.retry_frame[:])
			app.tx_offset = 0
			app.retry_pending = false
		}
	}

	if !flush_tx(app) {
		drop_connection(app, "TRANSPORT LOST // RETRYING")
		return
	}
	if !app.waiting_for_response && now - app.last_send >= 0.10 {
		if !send_input_message(app) {
			drop_connection(app, "TRANSPORT LOST // RETRYING")
			return
		}
		app.last_send = now
	}
	if !receive_state_messages(app) {
		drop_connection(app, "BACKEND DISCONNECTED // RETRYING")
	}
}

send_input_message :: proc(app: ^Input_State) -> bool {
	app.input_sequence += 1
	app.last_sent_directory_digits = app.intent.directory_digits
	value := input_to_cbor(app.intent, app.input_sequence, app.backend_output.state_revision)
	payload, marshal_err := cbor.marshal(value, cbor.ENCODE_FULLY_DETERMINISTIC)
	trace_cbor("[frontend -> backend]", value)
	defer cbor.destroy(value)
	defer delete(payload)
	if marshal_err != nil || len(payload) > MAX_FRAME_SIZE { return false }

	frame := make([dynamic]u8, 4 + len(payload))
	length := u32(len(payload))
	frame[0] = u8(length >> 24)
	frame[1] = u8(length >> 16)
	frame[2] = u8(length >> 8)
	frame[3] = u8(length)
	copy(frame[4:], payload)
	delete(app.tx)
	app.tx = nil
	app.tx = frame
	delete(app.retry_frame)
	app.retry_frame = nil
	app.retry_frame = make([dynamic]u8, len(frame))
	copy(app.retry_frame[:], frame[:])
	app.tx_offset = 0
	app.waiting_for_response = true
	return flush_tx(app)
}

flush_tx :: proc(app: ^Input_State) -> bool {
	for app.tx_offset < len(app.tx) {
		sent, send_err := net.send_tcp(app.socket, app.tx[app.tx_offset:])
		if send_err == .Would_Block || send_err == .Timeout { return true }
		if send_err != nil || sent <= 0 { return false }
		app.tx_offset += sent
	}
	delete(app.tx)
	app.tx = nil
	app.tx_offset = 0
	return true
}

receive_state_messages :: proc(app: ^Input_State) -> bool {
	buffer: [4096]byte
	for {
		n, err := net.recv_tcp(app.socket, buffer[:])
		if err == .Would_Block || err == .Timeout { break }
		if err != nil || n == 0 { return false }
		append(&app.rx, ..buffer[:n])
	}

	for len(app.rx) >= 4 {
		length := (u32(app.rx[0]) << 24) | (u32(app.rx[1]) << 16) | (u32(app.rx[2]) << 8) | u32(app.rx[3])
		if length > MAX_FRAME_SIZE { return false }
		if len(app.rx) < 4 + int(length) { break }
		payload := app.rx[4:4+int(length)]
		value, decode_err := cbor.decode(string(payload), cbor.Decoder_Flags{.Disallow_Streaming})
		if decode_err == nil {
			trace_cbor("[frontend <- backend]", value)
			if !apply_state_message(app, value) { cbor.destroy(value); return false }
			cbor.destroy(value)
		} else {
			return false
		}
		remaining := make([dynamic]u8, len(app.rx)-4-int(length))
		copy(remaining[:], app.rx[4+int(length):])
		delete(app.rx)
		app.rx = remaining
	}
	return true
}

frontend_trace_enabled :: proc() -> bool {
	configured, ok := os.lookup_env("NN_FRONTEND_TRACE", context.temp_allocator)
	return ok && configured == "1"
}

trace_cbor :: proc(prefix: string, value: cbor.Value) {
	if !frontend_trace_enabled() do return
	diagnostic, err := cbor.to_diagnostic_format(value, allocator=context.temp_allocator, padding=-1)
	if err == nil do fmt.eprintfln("%s %s", prefix, diagnostic)
}

drop_connection :: proc(app: ^Input_State, message: string) {
	close_socket(app)
	app.intent.debug.transport_connected = false
	set_status(app, message)
}

close_socket :: proc(app: ^Input_State) {
	if app.connected {
		net.close(app.socket)
		app.connected = false
	}
	delete(app.rx)
	app.rx = nil
	delete(app.tx)
	app.tx = nil
	app.tx_offset = 0
	if app.waiting_for_response && len(app.retry_frame) > 0 do app.retry_pending = true
	if !app.retry_pending do app.waiting_for_response = false
}

set_status :: proc(app: ^Input_State, message: string) {
	if app.status_owned { delete(app.status) }
	app.status = strings.clone(message) or_else ""
	app.status_owned = true
}

apply_state_message :: proc(app: ^Input_State, value: cbor.Value) -> bool {
	if !state_message_is_complete(value) { return false }
	output_value, ok := map_get(value, "output")
	if !ok { return false }
	if app.snapshot_owned { destroy_snapshot_storage(&app.backend_output) }
	response_sequence := u64_value(map_get_or(value, "input_sequence"))
	state := &app.backend_output
	state.state_revision = u64_value(map_get_or(value, "state_revision"))
	state.line_lamps = decode_lamps(map_get_or(output_value, "line_lamps"))
	state.game_phase = owned_string(map_get_or(output_value, "game_phase"))
	clock := map_get_or(output_value, "clock")
	state.clock_shift = u8_value(map_get_or(clock, "shift"))
	state.elapsed_seconds = u32_value(map_get_or(clock, "elapsed_seconds"))
	state.speaker_active = bool_value(map_get_or(output_value, "speaker_active"))
	tuning := map_get_or(output_value, "tuning")
	state.tuning = Tuning{coarse = u16_value(map_get_or(tuning, "coarse")), fine = u16_value(map_get_or(tuning, "fine"))}
	state.directory_pages = decode_pages(map_get_or(output_value, "directory_pages"))
	state.printer_output = decode_printer(map_get_or(output_value, "printer_output"))
	call_value := map_get_or(output_value, "call")
	if is_nil(call_value) { state.call = nil } else { state.call = decode_call(call_value) }
	state.shift = decode_shift(map_get_or(output_value, "shift"))
	state.backend_messages = decode_backend_messages(map_get_or(map_get_or(output_value, "debug"), "messages"))
	app.backend_output_ready = true
	accepted := bool_value(map_get_or(value, "accepted"))
	if response_sequence == app.input_sequence {
		app.waiting_for_response = false
		delete(app.retry_frame)
		app.retry_frame = nil
	}
	if accepted {
		if response_sequence == app.input_sequence {
			app.displayed_directory_digits = app.last_sent_directory_digits
		}
		if len(state.backend_messages) > 0 {
			set_status(app, fmt.tprintf("%s // %s", fmt.tprintf("AUTHORITATIVE STATE // REVISION %d", state.state_revision), state.backend_messages[0]))
		} else {
			set_status(app, fmt.tprintf("AUTHORITATIVE STATE // REVISION %d", state.state_revision))
		}
	}
	else {
		error := map_get_or(value, "error")
		set_status(app, fmt.tprintf("REJECTED // %s", string_value(map_get_or(error, "code"))))
	}
	app.snapshot_owned = true
	return true
}

destroy_input_intent :: proc(intent: ^Input_Intent) { delete(intent.cord_topology) }

destroy_snapshot_storage :: proc(snapshot: ^State_Output) {
	for &page in snapshot.directory_pages {
		delete(page.heading)
		delete(page.lines)
	}
	delete(snapshot.directory_pages)
	for &entry in snapshot.printer_output { delete(entry.text) }
	delete(snapshot.printer_output)
	if call, ok := snapshot.call.?; ok { delete(call.phase) }
	snapshot.call = nil
	for message in snapshot.backend_messages { delete(message) }
	delete(snapshot.backend_messages)
	delete(snapshot.game_phase)
	delete(snapshot.shift.phase)
}

destroy_input_state :: proc(app: ^Input_State) {
	close_socket(app)
	delete(app.retry_frame)
	app.retry_frame = nil
	destroy_input_intent(&app.intent)
	if app.snapshot_owned do destroy_snapshot_storage(&app.backend_output)
	if app.status_owned do delete(app.status)
}
