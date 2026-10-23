// Razdor's inject into miniquad's MainActivity (cargo-quad-apk reads quad.toml at the repo
// root). miniquad's onKey returns true for every key, so the system never saw the volume keys
// and the phone's volume did not change. The activity takes the volume keys itself, changes the
// media volume, and passes every other key on.

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

//% MAIN_ACTIVITY_ON_CREATE

setVolumeControlStream(AudioManager.STREAM_MUSIC);

//% END
