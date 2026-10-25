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
import android.view.DisplayCutout;
import android.content.BroadcastReceiver;
import android.content.IntentFilter;
import android.content.pm.PackageInstaller;
import android.media.AudioManager;
import android.view.KeyEvent;
import android.view.WindowInsetsController;
import android.view.WindowManager;
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
// How the installer ended the last session ("installed", "cancelled", "failed: <why>"), or ""
// while it has not ended. Set by the receiver, taken once by razdorInstallOutcome.
private volatile String razdorInstallOutcome = "";

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
        } else if (status == PackageInstaller.STATUS_SUCCESS) {
            razdorInstallOutcome = "installed";
        } else {
            // The player cancelled on the installer's screen, or the install failed: Razdor's
            // window reads this (razdorInstallOutcome) and offers the update again or says why.
            String message = intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE);
            razdorInstallOutcome = status == PackageInstaller.STATUS_FAILURE_ABORTED
                    ? "cancelled" : "failed: " + message;
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

// The installer's report on the last session, taken: "" while it has not ended.
public String razdorInstallOutcome() {
    String outcome = razdorInstallOutcome;
    razdorInstallOutcome = "";
    return outcome;
}

// The folder the app keeps its own files in (cleared by Android, never shared).
public String razdorCacheDir() {
    return getCacheDir().getAbsolutePath();
}

// The phone's safe area in physical px (src/ui/display.rs reads it): "left,top,right,bottom,bl,br",
// the display cutout's insets and the radii of the two bottom rounded corners. A corner the
// system does not report (before API 31) is -1; "" while the window has no insets yet. The
// system bars are left out: the game is in full screen, where they are hidden.
public String razdorSafeArea() {
    WindowInsets insets = getWindow().getDecorView().getRootWindowInsets();
    if (insets == null) {
        return "";
    }
    int left = 0;
    int top = 0;
    int right = 0;
    int bottom = 0;
    if (Build.VERSION.SDK_INT >= 28) {
        DisplayCutout cutout = insets.getDisplayCutout();
        if (cutout != null) {
            left = cutout.getSafeInsetLeft();
            top = cutout.getSafeInsetTop();
            right = cutout.getSafeInsetRight();
            bottom = cutout.getSafeInsetBottom();
        }
    }
    int bottomLeft = -1;
    int bottomRight = -1;
    if (Build.VERSION.SDK_INT >= 31) {
        // RoundedCorner.POSITION_BOTTOM_RIGHT is 2 and POSITION_BOTTOM_LEFT is 3 (API 31).
        bottomLeft = razdorCornerRadius(insets, 3);
        bottomRight = razdorCornerRadius(insets, 2);
    }
    return left + "," + top + "," + right + "," + bottom + "," + bottomLeft + "," + bottomRight;
}

// The radius of the rounded corner at `position`, 0 when the screen has none there, -1 when it
// cannot be read. Reflection: RoundedCorner is API 31, and this runs only from there.
private int razdorCornerRadius(WindowInsets insets, int position) {
    try {
        Object corner = insets.getClass().getMethod("getRoundedCorner", int.class).invoke(insets, position);
        if (corner == null) {
            return 0;
        }
        return (Integer) corner.getClass().getMethod("getRadius").invoke(corner);
    } catch (Exception e) {
        return -1;
    }
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

// Full screen (the settings' «Экран» row, src/ui/display.rs): the system bars are hidden, and a
// swipe from the edge shows them for a moment. Razdor's flag: on again when the window regains
// the focus (Android drops the immersive state after a dialog, e.g. the package installer).
private volatile boolean razdorImmersive = false;

// Full screen (`on`) hides the status and navigation bars. Windowed keeps the status bar hidden
// (as the fullscreen theme did before 1.0.4) and shows the navigation bar. Called from the game's
// thread; the window is changed on the UI thread.
public void razdorSetImmersive(final boolean on) {
    razdorImmersive = on;
    runOnUiThread(new Runnable() {
        @Override
        public void run() {
            razdorApplyImmersive(on);
        }
    });
}

private void razdorApplyImmersive(boolean on) {
    View decor = getWindow().getDecorView();
    if (Build.VERSION.SDK_INT >= 30) {
        getWindow().setDecorFitsSystemWindows(!on);
        WindowInsetsController controller = getWindow().getInsetsController();
        if (controller != null) {
            if (on) {
                controller.setSystemBarsBehavior(WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
                controller.hide(WindowInsets.Type.systemBars());
            } else {
                controller.show(WindowInsets.Type.navigationBars());
                controller.hide(WindowInsets.Type.statusBars());
            }
        }
    } else if (on) {
        decor.setSystemUiVisibility(View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                | View.SYSTEM_UI_FLAG_FULLSCREEN | View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN);
    } else {
        decor.setSystemUiVisibility(View.SYSTEM_UI_FLAG_FULLSCREEN);
    }
    if (Build.VERSION.SDK_INT >= 28) {
        // Draw into the notch's area too in full screen, so the game fills the screen.
        WindowManager.LayoutParams params = getWindow().getAttributes();
        params.layoutInDisplayCutoutMode = on ? WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES
                : WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_DEFAULT;
        getWindow().setAttributes(params);
    }
}

@Override
public void onWindowFocusChanged(boolean hasFocus) {
    super.onWindowFocusChanged(hasFocus);
    if (hasFocus && razdorImmersive) {
        razdorApplyImmersive(true);
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
