package frontend

foreign import nn_pipewire "system:nn_pipewire_capture"

foreign nn_pipewire {
	nn_pw_capture_start :: proc(rate, channels, max_samples: u32) -> rawptr ---
	nn_pw_capture_begin :: proc(capture: rawptr) ---
	nn_pw_capture_read :: proc(capture: rawptr, output: ^i16, capacity: u32) -> u32 ---
	nn_pw_capture_failed :: proc(capture: rawptr) -> int ---
	nn_pw_capture_destroy :: proc(capture: rawptr) ---
}
