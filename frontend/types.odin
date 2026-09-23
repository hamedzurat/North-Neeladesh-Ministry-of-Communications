package frontend

import "core:net"
import rl "nn_vendor:raylib"

PROTOCOL_VERSION :: 3
MAX_FRAME_SIZE :: 1_048_576
WINDOW_W :: 1440
WINDOW_H :: 900
LINE_COUNT :: 12
ENDPOINT_COUNT :: 16
CORD_SLOT_COUNT :: 8
ACTION_COUNT :: 4

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

Port_Kind :: enum { Subscriber, Operator, Ring_Generator, Tap_Bridge }
Port :: struct { kind: Port_Kind, index: u8 }
Cord :: struct { first, second: Port }
    Held :: struct { ptt, police, ems, tap: bool }
Tuning :: struct { coarse, fine: u16 }
Input_Debug :: struct { firmware_version: string, transport_connected: bool, device_faults: [dynamic]string }
Input_Intent :: struct {
	cord_topology: [dynamic]Cord,
	held_controls: Held,
	directory_digits: [4]u8,
	ring_line: i16,
	tuning: Tuning,
	debug: Input_Debug,
}

Page :: struct { page_number: u8, heading: string, lines: [dynamic]string }
Printer_Entry :: struct { entry_id: u64, text: string }
Call :: struct { caller_line, requested_callee_line: u8, phase: string }
Service_Call :: struct { service, phase: string }
Service_Error_Count :: struct { kind: string, count: u32 }
Shift :: struct { number: u8, phase: string, active_call_count: u8, completed_routings, required_service_calls, completed_service_calls, service_errors: u32, service_error_counts: [dynamic]Service_Error_Count }

State_Output :: struct {
	state_revision: u64,
    line_lamps: [12]bool,
	game_phase: string,
	clock_shift: u8,
	elapsed_seconds: u32,
	interference_level: u8,
	tuning: Tuning,
	directory_pages: [dynamic]Page,
 printer_output: [dynamic]Printer_Entry,
 call: Maybe(Call),
 calls: [dynamic]Call,
 service_call: Maybe(Service_Call),
	tap_bridge_monitoring: int,
	tap_bridge_audio_active: bool,
	shift: Shift,
	speaker_active: bool,
	backend_messages: [dynamic]string,
	shapla_story_beat: string,
	neel_story_beat: string,
	dirty_work_story_beat: string,
	dirty_work_completed_contacts: [dynamic]string,
	nahid_story_beat: string,
	nahid_scam_count: u8,
}

Voice_State :: struct {
	socket: net.UDP_Socket,
	endpoint: net.Endpoint,
	connected: bool,
	session_id: u64,
	turn_id: u64,
	state_revision: u64,
	last_ready_at: f64,
	capturing: bool,
	capture: rawptr,
	playback_stream: rl.AudioStream,
	playback_ready: bool,
	playback_finishing: bool,
	playback_queue: [dynamic]i16,
	last_audio_sequence: u16,
	has_audio_sequence: bool,
	accept_audio: bool,
	rtp_packets_received: u32,
	rtp_samples_received: u64,
	status: string,
}

Input_State :: struct {
	backend_output: State_Output,
	intent: Input_Intent,
	input_sequence: u64,
	displayed_directory_digits: [4]u8,
	last_sent_directory_digits: [4]u8,
	snapshot_owned: bool,
	last_send: f64,
	last_connect_attempt: f64,
	status: string,
	socket: net.TCP_Socket,
	connected: bool,
	rx: [dynamic]u8,
	tx: [dynamic]u8,
	tx_offset: int,
	retry_frame: [dynamic]u8,
	retry_pending: bool,
	waiting_for_response: bool,
	backend_output_ready: bool,
	status_owned: bool,
	crank_active_until: f64,
	dragging: bool,
	drag_endpoint: int,
	active_action: int,
	crank_fill: f32,
	crank_flash: f32,
    speaker_phase: f32,
    receipt_scroll: f32,
    voice: Voice_State,
}

REF_LINE_LABELS := [LINE_COUNT]cstring {
    "MOTIJHEEL FLAT", "SHAPLA APARTMENTS", "NEEL UNIVERSITY", "SHADHIN HOUSING", "MEGHNA ABASHON", "PADMA NIBASH",
    "SECRET POLICE DIRECTORATE", "BAGHA NEWS DESK", "KOYAL MARKET DEPOT", "NEEL UNIVERSITY", "SHAPLA APARTMENTS", "SHONARPARA EXCHANGE TOWER",
}

REF_ACTION_LABELS := [ACTION_COUNT]cstring {
	"PTT / OPERATOR", "POLICE", "EMS", "TAP BRIDGE LISTEN",
}

REF_SEGMENTS := [10][7]bool {
	{true, true, true, true, true, true, false}, {false, true, true, false, false, false, false},
	{true, true, false, true, true, false, true}, {true, true, true, true, false, false, true},
	{false, true, true, false, false, true, true}, {true, false, true, true, false, true, true},
	{true, false, true, true, true, true, true}, {true, true, true, false, false, false, false},
	{true, true, true, true, true, true, true}, {true, true, true, true, false, true, true},
}

REF_CORD_COLORS := [8]rl.Color {
	{220, 105, 80, 255}, {224, 179, 67, 255}, {96, 174, 224, 255}, {102, 190, 126, 255},
	{164, 127, 205, 255}, {213, 124, 165, 255}, {99, 194, 179, 255}, {202, 148, 91, 255},
}
