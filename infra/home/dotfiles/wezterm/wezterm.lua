local wezterm = require('wezterm')
local act = wezterm.action
local config = wezterm.config_builder()

-- Send ESC + CR on shift+enter (ported from the ghostty
-- `keybind = shift+enter=text:\x1b\r`). Lets apps that read shift+enter
-- as "newline, don't submit" work in the terminal.
config.keys = {
  {
    key = 'Enter',
    mods = 'SHIFT',
    action = act.SendString('\x1b\r'),
  },
}

-- Silent bell: no sound, blink the screen instead — the wezterm
-- equivalent of ghostty's `window-urgency = true` + silent bell.
config.audible_bell = 'Disabled'
config.visual_bell = {
  fade_in_duration_ms = 75,
  fade_out_duration_ms = 75,
  target = 'CursorColor',
}

return config
