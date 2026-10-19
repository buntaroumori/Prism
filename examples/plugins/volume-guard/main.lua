-- Volume Guard — example Prism plugin (API v1).
--
-- Hooks are optional; the ones defined here are the three the host knows:
--   on_load(config_json)   once, after the module is instantiated
--   on_event(event_json)   for every playback event forwarded by the engine
--   on_config(config_json) after the user edits a setting
--
-- The sandbox has no io, os, package, debug or load*; the only way out is the
-- `prism` table, and every function on it is gated by a declared capability.

local target_db = -12.0
local announce = false

local function read_settings()
  -- `prism.setting` returns a Lua value (number, string, boolean) or nil.
  target_db = prism.setting("target_db") or target_db
  announce = prism.setting("announce") == true
end

local function apply_level()
  -- set_volume takes dB in [-60, 0]; the manifest already constrains the
  -- setting, but the sandbox is the last line of defence, so check anyway.
  if type(target_db) ~= "number" or target_db < -60 or target_db > 0 then
    prism.log("warn", "refusing an out-of-range target level")
    return
  end

  local ok, err = pcall(prism.set_volume, target_db)
  if not ok then
    prism.log("error", "cannot set the level: " .. tostring(err))
    return
  end

  if announce then
    local text = string.format("Level set to %.1f dB", target_db)
    pcall(prism.emit, "volume-guard", text)
  end
end

function on_load(_config)
  read_settings()
  prism.log("info", string.format("armed at %.1f dB", target_db))
end

function on_config(_config)
  read_settings()
  prism.log("debug", string.format("target level is now %.1f dB", target_db))
end

function on_event(event)
  -- The payload is JSON. Matching the event kind is enough here; a plugin that
  -- needs the full object can bring its own decoder (Lua has none in-sandbox).
  if string.find(event, '"kind":"track_changed"', 1, true) then
    apply_level()
  end
end
