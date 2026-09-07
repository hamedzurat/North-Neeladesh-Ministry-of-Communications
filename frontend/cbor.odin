package frontend

import "core:encoding/cbor"
import "core:fmt"
import "core:strings"

input_to_cbor :: proc(input: Input_Intent, input_sequence, expected_state_revision: u64) -> cbor.Value {
	debug := cbor_map({
		entry("firmware_version", optional_text(input.debug.firmware_version)),
		entry("transport_connected", input.debug.transport_connected),
		entry("device_faults", strings_array(input.debug.device_faults[:])),
	})
	timestamps: [4]cbor.Value
	for timestamp, i in input.crank_rotation_timestamps { timestamps[i] = timestamp }
	input_state := cbor_map({
		entry("cord_topology", cords_to_cbor(input.cord_topology[:])),
		entry("held_controls", held_to_cbor(input.held_controls)),
		entry("directory_digits", digits_to_cbor(input.directory_digits)),
		entry("crank_rotation_timestamps", cbor_array(timestamps[:])),
		entry("tuning", cbor_map({entry("coarse", input.tuning.coarse), entry("fine", input.tuning.fine)})),
		entry("debug", debug),
	})
	return cbor_map({
		entry("protocol_version", u16(PROTOCOL_VERSION)),
		entry("input_sequence", input_sequence),
		entry("expected_state_revision", expected_state_revision),
		entry("input", input_state),
	})
}

held_to_cbor :: proc(held: Held) -> cbor.Value {
	return cbor_map({
		entry("ptt", held.ptt), entry("police", held.police), entry("ems", held.ems), entry("fire", held.fire),
		entry("tap_1", held.tap_1), entry("tap_2", held.tap_2),
	})
}

cords_to_cbor :: proc(cords: []Cord) -> cbor.Value {
	values := make([]cbor.Value, len(cords))
	for cord, i in cords {
		values[i] = cbor_map({entry("first", port_to_cbor(cord.first)), entry("second", port_to_cbor(cord.second))})
	}
	result := cbor_array(values)
	delete(values)
	return result
}

port_to_cbor :: proc(port: Port) -> cbor.Value {
	switch port.kind {
	case .Subscriber: return text(fmt.tprintf("subscriber_%d", port.index))
	case .Operator: return text("operator")
	case .Ring_Generator: return text("ring_generator")
	case .Tap_Bridge: return text(fmt.tprintf("tap_%d", port.index))
	}
	return text("operator")
}

digits_to_cbor :: proc(digits: [4]u8) -> cbor.Value {
	values: [4]cbor.Value
	for digit, i in digits { values[i] = digit }
	return cbor_array(values[:])
}

strings_array :: proc(values: []string) -> cbor.Value {
	result := make([]cbor.Value, len(values))
	for value, i in values { result[i] = text(value) }
	array := cbor_array(result)
	delete(result)
	return array
}

entry :: proc(key: string, value: cbor.Value) -> cbor.Map_Entry { return cbor.Map_Entry{key = text(key), value = value} }
text :: proc(value: string) -> cbor.Value { pointer := new(cbor.Text); pointer^ = strings.clone(value) or_else ""; return pointer }
optional_text :: proc(value: string) -> cbor.Value { if value == "" do return cbor.Nil(nil); return text(value) }
cbor_array :: proc(values: []cbor.Value) -> cbor.Value { pointer := new(cbor.Array); pointer^ = make([]cbor.Value, len(values)); copy(pointer^, values); return pointer }
cbor_map :: proc(values: []cbor.Map_Entry) -> cbor.Value { pointer := new(cbor.Map); pointer^ = make([]cbor.Map_Entry, len(values)); copy(pointer^, values); return pointer }

map_get_or :: proc(value: cbor.Value, key: string) -> cbor.Value { result, _ := map_get(value, key); return result }
map_get :: proc(value: cbor.Value, key: string) -> (cbor.Value, bool) {
	#partial switch pointer in value {
	case ^cbor.Map:
		for item in pointer^ {
			if string_value(item.key) == key { return item.value, true }
		}
	case: {}
	}
	return cbor.Nil(nil), false
}

map_has_keys :: proc(value: cbor.Value, keys: []string) -> bool {
	for key in keys {
		if _, ok := map_get(value, key); !ok do return false
	}
	return true
}

state_message_is_complete :: proc(value: cbor.Value) -> bool {
	if !map_has_keys(value, {"protocol_version", "input_sequence", "accepted", "error", "state_revision", "output"}) do return false
	if u16_value(map_get_or(value, "protocol_version")) != PROTOCOL_VERSION do return false
	output, output_ok := map_get(value, "output")
	if !output_ok || !map_has_keys(output, {"line_lamps", "game_phase", "clock", "speaker_active", "interference_level", "tap_bridge_audio_active", "tuning", "directory_pages", "printer_output", "call", "calls", "service_call", "tap_bridge_monitoring", "shift", "debug"}) do return false
	clock, clock_ok := map_get(output, "clock")
	if !clock_ok || !map_has_keys(clock, {"shift", "elapsed_seconds"}) do return false
	tuning, tuning_ok := map_get(output, "tuning")
	if !tuning_ok || !map_has_keys(tuning, {"coarse", "fine"}) do return false
	debug, debug_ok := map_get(output, "debug")
	if !debug_ok || !map_has_keys(debug, {"messages"}) do return false
	call, call_ok := map_get(output, "call")
	if !call_ok || (!is_nil(call) && !map_has_keys(call, {"caller_line", "requested_callee_line", "phase"})) do return false
	service, service_ok := map_get(output, "service_call")
	if !service_ok || (!is_nil(service) && !map_has_keys(service, {"service", "phase"})) do return false
	shift, shift_ok := map_get(output, "shift")
	if !shift_ok || !map_has_keys(shift, {"number", "phase", "active_call_count", "completed_routings", "required_service_calls", "completed_service_calls", "service_errors", "service_error_counts"}) do return false
	return true
}

is_nil :: proc(value: cbor.Value) -> bool { #partial switch item in value { case cbor.Nil: return true; case: return false } }
string_value :: proc(value: cbor.Value) -> string {
	#partial switch item in value { case ^cbor.Text: return item^; case: return "" }
}
owned_string :: proc(value: cbor.Value) -> string { return strings.clone(string_value(value)) or_else "" }
bool_value :: proc(value: cbor.Value) -> bool { #partial switch item in value { case bool: return item; case: return false } }
u64_value :: proc(value: cbor.Value) -> u64 {
	#partial switch item in value { case u8: return u64(item); case u16: return u64(item); case u32: return u64(item); case u64: return item; case: return 0 }
}
u32_value :: proc(value: cbor.Value) -> u32 { return u32(u64_value(value)) }
u16_value :: proc(value: cbor.Value) -> u16 { return u16(u64_value(value)) }
u8_value :: proc(value: cbor.Value) -> u8 { return u8(u64_value(value)) }
array_value :: proc(value: cbor.Value) -> []cbor.Value { #partial switch item in value { case ^cbor.Array: return item^; case: return nil } }

decode_lamps :: proc(value: cbor.Value) -> [16]bool { result: [16]bool; for item, i in array_value(value) { if i < 16 { result[i] = bool_value(item) } }; return result }

decode_pages :: proc(value: cbor.Value) -> [dynamic]Page {
	result := make([dynamic]Page, 0)
	for item in array_value(value) {
		lines := decode_strings(map_get_or(item, "lines"))
		append(&result, Page{page_number = u8_value(map_get_or(item, "page_number")), heading = owned_string(map_get_or(item, "heading")), lines = lines})
	}
	return result
}
decode_printer :: proc(value: cbor.Value) -> [dynamic]Printer_Entry {
	result := make([dynamic]Printer_Entry, 0)
	for item in array_value(value) { append(&result, Printer_Entry{entry_id = u64_value(map_get_or(item, "entry_id")), text = owned_string(map_get_or(item, "text"))}) }
	return result
}
decode_call_value :: proc(value: cbor.Value) -> Call { return Call{u8_value(map_get_or(value, "caller_line")), u8_value(map_get_or(value, "requested_callee_line")), owned_string(map_get_or(value, "phase"))} }
decode_call :: proc(value: cbor.Value) -> Maybe(Call) { return decode_call_value(value) }
decode_calls :: proc(value: cbor.Value) -> [dynamic]Call {
	result := make([dynamic]Call, 0)
	for item in array_value(value) { append(&result, decode_call_value(item)) }
	return result
}
decode_service_call :: proc(value: cbor.Value) -> Maybe(Service_Call) { return Service_Call{service = owned_string(map_get_or(value, "service")), phase = owned_string(map_get_or(value, "phase"))} }
decode_service_error_counts :: proc(value: cbor.Value) -> [dynamic]Service_Error_Count {
	result := make([dynamic]Service_Error_Count, 0)
	for item in array_value(value) {
		append(&result, Service_Error_Count{kind = owned_string(map_get_or(item, "kind")), count = u32_value(map_get_or(item, "count"))})
	}
	return result
}
decode_shift :: proc(value: cbor.Value) -> Shift { return Shift{number = u8_value(map_get_or(value, "number")), phase = owned_string(map_get_or(value, "phase")), active_call_count = u8_value(map_get_or(value, "active_call_count")), completed_routings = u32_value(map_get_or(value, "completed_routings")), required_service_calls = u32_value(map_get_or(value, "required_service_calls")), completed_service_calls = u32_value(map_get_or(value, "completed_service_calls")), service_errors = u32_value(map_get_or(value, "service_errors")), service_error_counts = decode_service_error_counts(map_get_or(value, "service_error_counts"))} }
decode_strings :: proc(value: cbor.Value) -> [dynamic]string { result := make([dynamic]string, 0); for item in array_value(value) { append(&result, owned_string(item)) }; return result }
decode_backend_messages :: proc(value: cbor.Value) -> [dynamic]string {
	result := make([dynamic]string, 0)
	for item in array_value(value) {
		formatted := fmt.tprintf("%s // %s", string_value(map_get_or(item, "code")), string_value(map_get_or(item, "message")))
		append(&result, strings.clone(formatted) or_else "")
	}
	return result
}
