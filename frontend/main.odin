package frontend

import rl "vendor:raylib"

main :: proc() {
	if !probe_backend() do return

	rl.SetConfigFlags({.WINDOW_HIGHDPI, .BORDERLESS_WINDOWED_MODE})
	rl.InitWindow(WINDOW_W, WINDOW_H, "North Neeladesh")
	defer rl.CloseWindow()
	monitor := rl.GetCurrentMonitor()
	rl.SetWindowPosition(0, 0)
	rl.SetWindowSize(rl.GetMonitorWidth(monitor), rl.GetMonitorHeight(monitor))

	FONT = rl.LoadFontEx("target/Iosevka-Regular.ttf", 32, nil, 0)
	if !rl.IsFontValid(FONT) do FONT = rl.GetFontDefault()
	defer if FONT.texture.id != rl.GetFontDefault().texture.id do rl.UnloadFont(FONT)
	rl.SetTextureFilter(FONT.texture, .BILINEAR)
	rl.SetTargetFPS(60)

	app := Input_State{}
	app.backend_output = initial_backend_output()
	app.intent = initial_intent()
	app.displayed_directory_digits = app.intent.directory_digits
	set_status(&app, "BACKEND OFFLINE // RETRYING")
	app.drag_endpoint = -1
	app.active_action = -1
	defer destroy_input_state(&app)

	for !rl.WindowShouldClose() {
		update_camera()
		delta := rl.GetFrameTime()
		poll_backend(&app, rl.GetTime())
		update_reference_controls(&app, delta)

		rl.BeginDrawing()
		rl.ClearBackground(BACKGROUND)
		rl.BeginMode2D(CAMERA)
		draw_reference_switchboard(&app, delta)
		rl.EndMode2D()
		rl.EndDrawing()
		free_all(context.temp_allocator)
	}
}
