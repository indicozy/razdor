-- Menu script of the converted map (M5, spec 3.12): quits the game when the end window's Quit is
-- pressed. The global script sends the event through the player (types.Player.sendMenuEvent).
local menu = require('openmw.menu')

return {
    eventHandlers = {
        RazdorQuit = function()
            menu.quit()
        end,
    },
}
