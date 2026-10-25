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

import android.app.PendingIntent;
import android.content.BroadcastReceiver;
import android.content.IntentFilter;
import android.content.pm.PackageInstaller;
import android.media.AudioManager;
import android.view.KeyEvent;
import android.widget.Toast;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;

//% END

//% MAIN_ACTIVITY_BODY

// Razdor's self-update on Android (src/update/phone.rs calls these through the JNI, on the
// update's own thread; the game never waits on them). Each returns "" when it worked, else why
// not. The download is plain HttpURLConnection (the phone has no curl); the install hands the
// downloaded file to the system's PackageInstaller, which asks the player to confirm it.

private static final String RAZDOR_INSTALL_ACTION = "io.github.indicozy.razdor.INSTALL_STATUS";
private static final String RAZDOR_INSTALL_TOKEN = "io.github.indicozy.razdor.INSTALL_TOKEN";
private boolean razdorInstallReceiverRegistered = false;
// The random token of the install under way: a status broadcast counts only with this token
// (any app can send our action, but cannot read the PendingIntent that carries the token).
private String razdorInstallToken = null;

// The installer's report: a confirmation screen to show, or why the install did not happen.
// The explicit intent (our package, our token) is the only way in; the confirmation is started
// only when it is the system installer's own (see razdorIsSystemInstaller).
private final BroadcastReceiver razdorInstallReceiver = new BroadcastReceiver() {
    @Override
    public void onReceive(Context context, Intent intent) {
        String expected = razdorInstallToken;
        String got = intent.getStringExtra(RAZDOR_INSTALL_TOKEN);
        if (expected == null || got == null || !expected.equals(got)) {
            return;
        }
        int status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE);
        if (status == PackageInstaller.STATUS_PENDING_USER_ACTION) {
            Intent confirm = intent.getParcelableExtra(Intent.EXTRA_INTENT);
            if (confirm != null && razdorIsSystemInstaller(context, confirm)) {
                // No URI grants go along: the confirmation needs none of ours.
                confirm.setFlags((confirm.getFlags()
                        & ~(Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                        | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION | Intent.FLAG_GRANT_PREFIX_URI_PERMISSION))
                        | Intent.FLAG_ACTIVITY_NEW_TASK);
                context.startActivity(confirm);
            }
        } else if (status != PackageInstaller.STATUS_SUCCESS) {
            String message = intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE);
            Toast.makeText(context, "Razdor: " + message, Toast.LENGTH_LONG).show();
        }
    }
};

// Whether `confirm` is the confirmation activity of the system's own installer: its action is
// the installer's, and the activity it resolves to belongs to a system app. On API 30+ package
// visibility can hide the installer's activity from us (cargo-quad-apk cannot declare a
// <queries> entry), so resolveActivity may return null: that does not block the confirmation.
// The broadcast itself is already authenticated by the session's random token above.
private boolean razdorIsSystemInstaller(Context context, Intent confirm) {
    // PackageInstaller.ACTION_CONFIRM_INSTALL is hidden from the public SDK: its value, literally.
    if (!"android.content.pm.action.CONFIRM_INSTALL".equals(confirm.getAction())) {
        return false;
    }
    android.content.pm.ResolveInfo info = context.getPackageManager().resolveActivity(confirm, 0);
    if (info == null || info.activityInfo == null) {
        return true;
    }
    return (info.activityInfo.applicationInfo.flags & android.content.pm.ApplicationInfo.FLAG_SYSTEM) != 0;
}

// A random token of 128 bits, in hex.
private String razdorNewToken() {
    byte[] bytes = new byte[16];
    new java.security.SecureRandom().nextBytes(bytes);
    StringBuilder hex = new StringBuilder();
    for (byte b : bytes) {
        hex.append(String.format("%02x", b));
    }
    return hex.toString();
}

// The folder the app keeps its own files in (cleared by Android, never shared).
public String razdorCacheDir() {
    return getCacheDir().getAbsolutePath();
}

// Downloads `url` into the file `path`, giving up after `seconds` without an answer.
public String razdorDownload(String url, String path, int seconds) {
    HttpURLConnection connection = null;
    try {
        connection = (HttpURLConnection) new URL(url).openConnection();
        connection.setConnectTimeout(seconds * 1000);
        connection.setReadTimeout(seconds * 1000);
        connection.setRequestProperty("User-Agent", "razdor");
        int code = connection.getResponseCode();
        if (code / 100 != 2) {
            return "HTTP " + code;
        }
        try (InputStream in = connection.getInputStream(); FileOutputStream out = new FileOutputStream(path)) {
            byte[] buffer = new byte[65536];
            int n;
            while ((n = in.read(buffer)) > 0) {
                out.write(buffer, 0, n);
            }
        }
        return "";
    } catch (Exception e) {
        return e.toString();
    } finally {
        if (connection != null) {
            connection.disconnect();
        }
    }
}

// Installs the APK at `path` over this app: the file is copied into an installer session
// (before this returns, so the caller may delete it), and the session is committed. The
// installer then asks the player to confirm (or says why not) through razdorInstallReceiver.
public String razdorInstall(String path) {
    PackageInstaller.Session session = null;
    try {
        PackageInstaller installer = getPackageManager().getPackageInstaller();
        File file = new File(path);
        PackageInstaller.SessionParams params = new PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL);
        params.setSize(file.length());
        int id = installer.createSession(params);
        session = installer.openSession(id);
        try (InputStream in = new FileInputStream(file); OutputStream out = session.openWrite("razdor.apk", 0, file.length())) {
            byte[] buffer = new byte[65536];
            int n;
            while ((n = in.read(buffer)) > 0) {
                out.write(buffer, 0, n);
            }
            session.fsync(out);
        }
        if (!razdorInstallReceiverRegistered) {
            IntentFilter filter = new IntentFilter(RAZDOR_INSTALL_ACTION);
            if (Build.VERSION.SDK_INT >= 33) {
                registerReceiver(razdorInstallReceiver, filter, Context.RECEIVER_NOT_EXPORTED);
            } else {
                // Before API 33 the receiver is exported; the token is what keeps others out.
                registerReceiver(razdorInstallReceiver, filter);
            }
            razdorInstallReceiverRegistered = true;
        }
        razdorInstallToken = razdorNewToken();
        Intent report = new Intent(RAZDOR_INSTALL_ACTION).setPackage(getPackageName());
        report.putExtra(RAZDOR_INSTALL_TOKEN, razdorInstallToken);
        int flags = PendingIntent.FLAG_UPDATE_CURRENT;
        if (Build.VERSION.SDK_INT >= 31) {
            // The installer fills in the status, so the intent must be mutable (API 31+).
            flags |= PendingIntent.FLAG_MUTABLE;
        }
        PendingIntent pending = PendingIntent.getBroadcast(this, id, report, flags);
        session.commit(pending.getIntentSender());
        return "";
    } catch (Exception e) {
        if (session != null) {
            try {
                session.abandon();
            } catch (Exception ignored) {
                // The session is gone already: nothing to undo.
            }
        }
        return e.toString();
    } finally {
        if (session != null) {
            session.close();
        }
    }
}

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
