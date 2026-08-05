-- An example plugin. Two registrations, which is the whole extension point:
-- a command, and a surface.
--
-- The surface returns a *declaration*, not drawing. Coordinates are fractions
-- of the window — root-ui's own normalized layout — while radii and shadows are
-- density-independent pixels, so a corner is the same size whatever the surface
-- is sized to. Colours are asked for by role, so this plugin follows the
-- editor's scheme instead of choosing its own.

local shown = 0

nvimglsl.command('Hello', function(argument)
  shown = shown + 1
  nvimglsl.notify('hello ' .. (argument ~= '' and argument or 'world') .. ' (' .. shown .. ')')
end)

nvimglsl.surface('Card', function(window)
  local w, h = 0.44, 0.30
  local x, y = (1 - w) / 2, 0.22
  return {
    surfaces = {
      { id = 'scrim', x = 0, y = 0, w = 1, h = 1, fill = 'scrim' },
      { id = 'card', name = 'Dialog', x = x, y = y, w = w, h = h, radius = 10,
        fill = 'surface', stroke = 'outline', stroke_width = 0.002,
        shadow = { dy = 10, blur = 28 } },
      { id = 'rule', x = x + 0.02, y = y + 0.09, w = w - 0.04, h = 0.0015,
        fill = 'separator' },
    },
    texts = {
      { x = x + 0.025, y = y + 0.06, text = 'a plugin drew this', role = 'on_surface' },
      { x = x + 0.025, y = y + 0.145, text = 'window ' .. math.floor(window.width) ..
        ' x ' .. math.floor(window.height), role = 'on_surface_muted' },
      { x = x + 0.025, y = y + 0.205, text = ':Hello ran ' .. shown .. ' time(s)',
        role = 'accent' },
      { x = x + 0.025, y = y + 0.265, text = 'Esc to close', role = 'on_surface_muted' },
    },
  }
end)
