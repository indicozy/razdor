// Razdor's inject into miniquad's MainActivity (cargo-quad-apk reads quad.toml at the repo
// root). miniquad's onKey returns true for every key, so the system never saw the volume keys
// and the phone's volume did not change. The activity takes the volume keys itself, changes the
// media volume, and passes every other key on.
//
// The keyboard does not squash the game: on API 30+ miniquad pads its layout by the keyboard's
// inset, so the game's surface shrinks into the top part while the keyboard shows. The section
// below pads by the system bars only, so the keyboard floats over the full-size picture, and the
// game draws the text being typed in a strip at the top (src/ui/widgets.rs).

//% IMPORTS

import android.media.AudioManager;
import android.view.KeyEvent;

//% END

//% MAIN_ACTIVITY_BODY

@Override
public boolean dispatchKeyEvent(KeyEvent event) {
    int code = event.getKeyCode();
    if (code == KeyEvent.KEYCODE_VOLUME_UP || code == KeyEvent.KEYCODE_VOLUME_DOWN || code == KeyEvent.KEYCODE_VOLUME_MUTE) {
        if (event.getAction() == KeyEvent.ACTION_DOWN) {
            int direction = code == KeyEvent.KEYCODE_VOLUME_UP ? AudioManager.ADJUST_RAISE
                : code == KeyEvent.KEYCODE_VOLUME_DOWN ? AudioManager.ADJUST_LOWER
                : AudioManager.ADJUST_TOGGLE_MUTE;
            ((AudioManager) getSystemService(AUDIO_SERVICE)).adjustStreamVolume(AudioManager.STREAM_MUSIC, direction, AudioManager.FLAG_SHOW_UI);
        }
        return true;
    }
    return super.dispatchKeyEvent(event);
}

//% END

//% RESIZING_LAYOUT_ON_APPLY_WINDOW_INSETS

// Returned early on API 30+: the IME (keyboard) inset is left out of the padding. Below 30
// miniquad's own code does nothing, so it is left as it is.
if (Build.VERSION.SDK_INT >= 30) {
    Insets sysInsets = insets.getInsets(WindowInsets.Type.systemBars());
    v.setPadding(sysInsets.left, sysInsets.top, sysInsets.right, sysInsets.bottom);
    return insets;
}

//% END

//% MAIN_ACTIVITY_ON_CREATE

setVolumeControlStream(AudioManager.STREAM_MUSIC);

//% END
