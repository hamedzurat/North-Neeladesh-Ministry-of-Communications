package frontend

// The Odin cabinet owns the small edge of the voice protocol. The backend still
// owns STT, dialogue, and TTS;
// this file only captures PCM, transports tagged UDP datagrams, and plays RTP.

import "core:encoding/cbor"
import "core:fmt"
import "core:io"
import "core:net"
import "core:os"
import "core:strings"
import rl "nn_vendor:raylib"

VOICE_PROTOCOL_VERSION :: 2
VOICE_STATUS_TAG :: u8(0x01)
VOICE_CONTROL_TAG :: u8(0x02)
VOICE_INPUT_AUDIO_TAG :: u8(0x03)
VOICE_INPUT_SAMPLE_RATE :: 16000
VOICE_INPUT_PACKET_SAMPLES :: 320
VOICE_AUDIO_SAMPLE_RATE :: 24000
VOICE_AUDIO_PACKET_SAMPLES :: 480
VOICE_PLAYBACK_BUFFER_SAMPLES :: VOICE_AUDIO_PACKET_SAMPLES * 4
VOICE_AUDIO_PAYLOAD_TYPE :: u8(96)
VOICE_AUDIO_SSRC :: u32(0x4e45_5554)
VOICE_MAX_CAPTURE_SAMPLES :: VOICE_INPUT_SAMPLE_RATE * 15

voice_start :: proc(app: ^Input_State) {
	app.voice.session_id = 1
	app.voice.turn_id = 1
	address := "127.0.0.1:7879"
	if configured, ok := os.lookup_env("NN_VOICE_BACKEND_ADDRESS", context.temp_allocator); ok && configured != "" {
		address = configured
	}
	endpoint, ok := net.parse_endpoint(address)
	if !ok {
		app.voice.status = "VOICE ADDRESS INVALID"
		return
	}
	socket, err := net.make_unbound_udp_socket(.IP4)
	if err != nil {
		app.voice.status = "VOICE UDP UNAVAILABLE"
		return
	}
	if net.set_blocking(socket, false) != nil {
		net.close(socket)
		app.voice.status = "VOICE UDP SETUP FAILED"
		return
	}
	app.voice.socket = socket
	app.voice.endpoint = endpoint
	app.voice.connected = true
	app.voice.status = "VOICE RELAY ONLINE"
	app.voice.capture = nn_pw_capture_start(VOICE_INPUT_SAMPLE_RATE, 1, VOICE_MAX_CAPTURE_SAMPLES)
	if app.voice.capture == nil {
		app.voice.status = "PIPEWIRE CAPTURE UNAVAILABLE"
		voice_send_status_error(&app.voice, "failed", "capture_start_failed", "unable to open the native PipeWire microphone stream")
		return
	}
	voice_send_status(&app.voice, "ready")
	app.voice.last_ready_at = 0
	if rl.IsAudioDeviceReady() {
		rl.SetAudioStreamBufferSizeDefault(VOICE_PLAYBACK_BUFFER_SAMPLES)
		app.voice.playback_stream = rl.LoadAudioStream(VOICE_AUDIO_SAMPLE_RATE, 16, 1)
		app.voice.playback_ready = rl.IsAudioStreamValid(app.voice.playback_stream)
		if app.voice.playback_ready {
			rl.SetAudioStreamVolume(app.voice.playback_stream, 1.0)
			rl.PlayAudioStream(app.voice.playback_stream)
		}
	}
}

voice_poll :: proc(app: ^Input_State, now: f64) {
	if !app.voice.connected do return
	if now - app.voice.last_ready_at >= 4.0 {
		voice_send_status(&app.voice, "ready")
		app.voice.last_ready_at = now
	}
	if app.voice.capture != nil && nn_pw_capture_failed(app.voice.capture) != 0 {
		app.voice.status = "PIPEWIRE CAPTURE FAILED"
	}
	buffer: [65535]byte
	for {
		count, _, err := net.recv_udp(app.voice.socket, buffer[:])
		if err == .Would_Block do break
		if err != nil {
			app.voice.status = "VOICE UDP RECEIVE FAILED"
			break
		}
		if count <= 0 do break
		voice_handle_datagram(&app.voice, buffer[:count])
	}
}

voice_handle_datagram :: proc(voice: ^Voice_State, datagram: []byte) {
	if len(datagram) == 0 do return
	if datagram[0] == VOICE_CONTROL_TAG {
		value, err := cbor.decode(string(datagram[1:]), cbor.Decoder_Flags{.Disallow_Streaming})
		if err != nil do return
		defer cbor.destroy(value)
		if u16_value(map_get_or(value, "protocol_version")) != VOICE_PROTOCOL_VERSION do return
		if u64_value(map_get_or(value, "session_id")) != voice.session_id do return
		incoming_turn_id := u64_value(map_get_or(value, "turn_id"))
		voice.state_revision = u64_value(map_get_or(value, "state_revision"))
		control := string_value(map_get_or(value, "control"))
		if control != "start_ptt" && incoming_turn_id != voice.turn_id do return
		voice.turn_id = incoming_turn_id
		switch control {
		case "start_ptt": voice_start_capture(voice)
		case "release_ptt": voice_release_capture(voice)
		case "cancel": voice_cancel_capture(voice)
		}
		return
	}
	if datagram[0] == VOICE_STATUS_TAG {
		value, err := cbor.decode(string(datagram[1:]), cbor.Decoder_Flags{.Disallow_Streaming})
		if err != nil do return
		defer cbor.destroy(value)
		if u16_value(map_get_or(value, "protocol_version")) != VOICE_PROTOCOL_VERSION do return
		if u64_value(map_get_or(value, "session_id")) != voice.session_id do return
		voice.state_revision = u64_value(map_get_or(value, "state_revision"))
		status := string_value(map_get_or(value, "status"))
		if status == "completed" || status == "failed" || status == "cancelled" {
			fmt.println(fmt.tprintf("[VOICE-DEBUG] RTP complete packets=%d samples=%d status=%s", voice.rtp_packets_received, voice.rtp_samples_received, status))
			voice_finish_playback(voice)
		}
		return
	}
	voice_handle_rtp(voice, datagram)
}

voice_send_status :: proc(voice: ^Voice_State, status: string) {
	voice_send_status_error(voice, status, "", "")
}

voice_send_status_error :: proc(voice: ^Voice_State, status, error_code, error_message: string) {
	error_value: cbor.Value = cbor.Nil(nil)
	if error_code != "" {
		error_value = cbor_map({entry("code", text(error_code)), entry("message", text(error_message))})
	}
	value := cbor_map({
		entry("protocol_version", u16(VOICE_PROTOCOL_VERSION)),
		entry("session_id", voice.session_id),
		entry("turn_id", voice.turn_id),
		entry("state_revision", voice.state_revision),
		entry("status", text(status)),
		entry("transcript", cbor.Nil(nil)),
		entry("response_text", cbor.Nil(nil)),
		entry("error", error_value),
	})
	payload, err := cbor.marshal(value, cbor.ENCODE_FULLY_DETERMINISTIC)
	cbor.destroy(value)
	if err != nil do return
	packet := make([]byte, len(payload) + 1)
	packet[0] = VOICE_STATUS_TAG
	copy(packet[1:], payload)
	_, _ = net.send_udp(voice.socket, packet, voice.endpoint)
	delete(packet)
	delete(payload)
}

voice_start_capture :: proc(voice: ^Voice_State) {
	if voice.capturing do return
	if voice.capture == nil {
		voice_send_status_error(voice, "failed", "capture_start_failed", "PipeWire microphone capture is unavailable")
		return
	}
	nn_pw_capture_begin(voice.capture)
	voice.capturing = true
	voice.playback_finishing = false
	voice.accept_audio = false
	resize(&voice.playback_queue, 0)
	voice_send_status(voice, "listening")
}

voice_release_capture :: proc(voice: ^Voice_State) {
	if !voice.capturing {
		voice_send_input_audio(voice, nil)
		voice_send_status(voice, "ready")
		return
	}
	voice.capturing = false
	samples := make([]i16, VOICE_MAX_CAPTURE_SAMPLES)
	count := nn_pw_capture_read(voice.capture, &samples[0], VOICE_MAX_CAPTURE_SAMPLES)
	voice_send_input_samples(voice, samples[:count])
	delete(samples)
}

voice_cancel_capture :: proc(voice: ^Voice_State) {
	if voice.capturing {
		voice.capturing = false
	}
	if voice.capture != nil {
		nn_pw_capture_begin(voice.capture)
	}
	voice_send_status(voice, "cancelled")
}

voice_send_input_audio :: proc(voice: ^Voice_State, bytes: []byte) {
	samples := len(bytes) / 2
	if samples == 0 {
		voice_send_input_chunk(voice, nil, 0, true)
		return
	}
	chunk_count := (samples + VOICE_INPUT_PACKET_SAMPLES - 1) / VOICE_INPUT_PACKET_SAMPLES
	for chunk_index in 0 ..< chunk_count {
		start := chunk_index * VOICE_INPUT_PACKET_SAMPLES
		end := min(samples, start + VOICE_INPUT_PACKET_SAMPLES)
		voice_send_input_chunk(voice, bytes[start * 2:end * 2], chunk_index, chunk_index + 1 == chunk_count)
	}
}

voice_send_input_samples :: proc(voice: ^Voice_State, samples: []i16) {
	if len(samples) == 0 {
		voice_send_input_chunk(voice, nil, 0, true)
		return
	}
	chunk_count := (len(samples) + VOICE_INPUT_PACKET_SAMPLES - 1) / VOICE_INPUT_PACKET_SAMPLES
	for chunk_index in 0 ..< chunk_count {
		start := chunk_index * VOICE_INPUT_PACKET_SAMPLES
		end := min(len(samples), start + VOICE_INPUT_PACKET_SAMPLES)
		voice_send_input_sample_chunk(voice, samples[start:end], chunk_index, chunk_index + 1 == chunk_count)
	}
}

voice_send_input_chunk :: proc(voice: ^Voice_State, bytes: []byte, chunk_index: int, complete: bool) {
	samples := make([]i16, len(bytes) / 2)
	for index in 0 ..< len(samples) {
		samples[index] = i16(u16(bytes[index * 2]) | u16(bytes[index * 2 + 1]) << 8)
	}
	voice_send_input_sample_chunk(voice, samples, chunk_index, complete)
	delete(samples)
}

voice_send_input_sample_chunk :: proc(voice: ^Voice_State, samples: []i16, chunk_index: int, complete: bool) {
	values := make([]cbor.Value, len(samples))
	for index in 0 ..< len(samples) {
		sample := samples[index]
		if sample < 0 { values[index] = cbor.Negative_U16(u16(-1 - sample)) }
		else { values[index] = u16(sample) }
	}
	value := cbor_map({
		entry("protocol_version", u16(VOICE_PROTOCOL_VERSION)),
		entry("session_id", voice.session_id),
		entry("turn_id", voice.turn_id),
		entry("state_revision", voice.state_revision),
		entry("chunk_index", u32(chunk_index)),
		entry("complete", complete),
		entry("samples", cbor_array(values)),
	})
	payload, err := cbor.marshal(value, cbor.ENCODE_FULLY_DETERMINISTIC)
	cbor.destroy(value)
	if err == nil {
		packet := make([]byte, len(payload) + 1)
		packet[0] = VOICE_INPUT_AUDIO_TAG
		copy(packet[1:], payload)
		_, _ = net.send_udp(voice.socket, packet, voice.endpoint)
		delete(packet)
	}
	delete(payload)
}

voice_handle_rtp :: proc(voice: ^Voice_State, packet: []byte) {
	if voice.playback_finishing do return
	if len(packet) < 12 || packet[0] >> 6 != 2 || packet[1] & 0x7f != VOICE_AUDIO_PAYLOAD_TYPE do return
	csrc_count := int(packet[0] & 0x0f)
	header_length := 12 + csrc_count * 4
	if packet[0] & 0x10 != 0 {
		if len(packet) < header_length + 4 do return
		extension_length := int(u16(packet[header_length + 2]) << 8 | u16(packet[header_length + 3])) * 4
		header_length += 4 + extension_length
	}
	if len(packet) < header_length do return
	payload := packet[header_length:]
	if packet[0] & 0x20 != 0 {
		if len(payload) == 0 do return
		padding := int(payload[len(payload) - 1])
		if padding == 0 || padding > len(payload) do return
		payload = payload[:len(payload) - padding]
	}
	if len(payload) % 2 != 0 do return
	ssrc := u32(packet[8]) << 24 | u32(packet[9]) << 16 | u32(packet[10]) << 8 | u32(packet[11])
	if ssrc != VOICE_AUDIO_SSRC do return
	sequence := u16(packet[2]) << 8 | u16(packet[3])
	marker := packet[1] & 0x80 != 0
	if !voice.accept_audio && !marker do return
	if marker {
		voice.accept_audio = true
		voice.rtp_packets_received = 0
		voice.rtp_samples_received = 0
		fmt.println(fmt.tprintf("[VOICE-DEBUG] RTP start sequence=%d", sequence))
	}
	if voice.has_audio_sequence && sequence != voice.last_audio_sequence + 1 {
		// UDP loss is audible but does not invalidate the rest of the turn.
	}
	voice.last_audio_sequence = sequence
	voice.has_audio_sequence = true
	for index in 0 ..< len(payload) / 2 {
		sample := i16(u16(payload[index * 2]) << 8 | u16(payload[index * 2 + 1]))
		append(&voice.playback_queue, sample)
	}
	voice.rtp_packets_received += 1
	voice.rtp_samples_received += u64(len(payload) / 2)
	if len(voice.playback_queue) > VOICE_AUDIO_SAMPLE_RATE * 30 {
		resize(&voice.playback_queue, 0)
		voice.accept_audio = false
	}
}

voice_update_playback :: proc(app: ^Input_State) {
	voice := &app.voice
	if !voice.playback_ready do return
	if len(voice.playback_queue) < VOICE_PLAYBACK_BUFFER_SAMPLES {
		if !voice.playback_finishing || len(voice.playback_queue) == 0 || !rl.IsAudioStreamProcessed(voice.playback_stream) do return
		pad := make([dynamic]i16, VOICE_PLAYBACK_BUFFER_SAMPLES)
		copy(pad[:], voice.playback_queue[:])
		rl.UpdateAudioStream(voice.playback_stream, rawptr(&pad[0]), VOICE_PLAYBACK_BUFFER_SAMPLES)
		delete(pad)
		resize(&voice.playback_queue, 0)
		voice.playback_finishing = false
		return
	}
	if !rl.IsAudioStreamProcessed(voice.playback_stream) do return
	chunk := voice.playback_queue[:VOICE_PLAYBACK_BUFFER_SAMPLES]
	rl.UpdateAudioStream(voice.playback_stream, rawptr(&chunk[0]), VOICE_PLAYBACK_BUFFER_SAMPLES)
	copy(voice.playback_queue[:], voice.playback_queue[VOICE_PLAYBACK_BUFFER_SAMPLES:])
	resize(&voice.playback_queue, len(voice.playback_queue) - VOICE_PLAYBACK_BUFFER_SAMPLES)
}

voice_finish_playback :: proc(voice: ^Voice_State) {
	voice.playback_finishing = true
	voice.has_audio_sequence = false
	voice.accept_audio = false
}

voice_close :: proc(voice: ^Voice_State) {
	voice.capturing = false
	if voice.capture != nil {
		nn_pw_capture_destroy(voice.capture)
		voice.capture = nil
	}
	if voice.playback_ready {
		rl.StopAudioStream(voice.playback_stream)
		rl.UnloadAudioStream(voice.playback_stream)
		voice.playback_ready = false
	}
	voice.playback_finishing = false
	voice.accept_audio = false
	delete(voice.playback_queue)
	if voice.connected {
		net.close(voice.socket)
		voice.connected = false
	}
}
