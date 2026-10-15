-- Nested Hyprland for the eave glass spike. ALT is the modifier, so the outer session keeps SUPER.
hl.monitor({ output = "", mode = "preferred", position = "auto", scale = 1.75 })

hl.plugin.load("/home/francesco/code/fran/eaveshell/eave/.worktrees/glass-spike/hypr-eave/hypr-eave.so")

hl.config({
	general = { gaps_in = 5, gaps_out = 5, border_size = 1 },
	decoration = { rounding = 10, blur = { enabled = true, size = 4, passes = 2 } },
	misc = { disable_hyprland_logo = true, disable_splash_rendering = true },
	debug = { disable_logs = false },
})

hl.on("hyprland.start", function()
	hl.exec_cmd("hyprpaper")
	hl.exec_cmd("/home/francesco/code/fran/eaveshell/eave/.worktrees/glass-spike/target/release/eave")
end)

hl.bind("ALT + RETURN", hl.dsp.exec_cmd("kitty"))
hl.bind("ALT + Q", hl.dsp.window.close())
hl.bind("ALT + M", hl.dsp.exit())
hl.bind("ALT + V", hl.dsp.window.float({ action = "toggle" }))
hl.bind("ALT + mouse:272", hl.dsp.window.drag(), { mouse = true })
hl.bind("ALT + mouse:273", hl.dsp.window.resize(), { mouse = true })
