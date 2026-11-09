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
import android.content.Intent;
import android.database.Cursor;
import android.os.ParcelFileDescriptor;
import android.net.Uri;
import android.provider.OpenableColumns;
import android.view.Display;
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

// The Rust side of the session slot (src/ui/android.rs): written when the activity pauses
// (MAIN_ACTIVITY_ON_PAUSE below). Runs on the activity's UI thread; no game work is done here.
private static native void razdorOnPause();

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

// The app's own folder (src/ui/android.rs files_dir): the external files folder, else the internal one.
public String razdorFilesDir() {
    File dir = getExternalFilesDir(null);
    if (dir == null) {
        dir = getFilesDir();
    }
    return dir.getAbsolutePath();
}

// The phone's safe area in physical px (src/ui/display.rs reads it): "left,top,right,bottom,bl,br,tl,tr",
// the display cutout's insets and the radii of the four rounded corners. A corner the system does
// not report (before API 31) is -1; "" while the window has no insets yet. The system bars are left
// out: the game is in full screen, where they are hidden.
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
    int topLeft = -1;
    int topRight = -1;
    if (Build.VERSION.SDK_INT >= 31) {
        // RoundedCorner.POSITION_TOP_LEFT is 0, POSITION_TOP_RIGHT 1, POSITION_BOTTOM_RIGHT 2 and
        // POSITION_BOTTOM_LEFT 3 (API 31).
        bottomLeft = razdorCornerRadius(insets, 3);
        bottomRight = razdorCornerRadius(insets, 2);
        topLeft = razdorCornerRadius(insets, 0);
        topRight = razdorCornerRadius(insets, 1);
    }
    return left + "," + top + "," + right + "," + bottom + "," + bottomLeft + "," + bottomRight + "," + topLeft + "," + topRight;
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

// The system's document picker (Storage Access Framework) for «Добавить карту…» (src/ui/map_import.rs,
// razdorPickFile) and for «Обновить игру из архива…» (src/ui/game_import.rs, razdorPickArchive). Its
// answer comes back in miniquad's onActivityResult, which hands it to razdorOnActivityResult (no
// AndroidX). A map is copied into the app's cache, because the game reads a file, not a content URI.
// An archive is not copied: its descriptor goes to Rust, which reads it in place (a stream that cannot
// seek is copied there). razdorPickOutcome is the game's poll: "" while the picker is open or the copy
// runs, "cancel", "error: <why>", "ok\n<name>\n<path>" (a map copy) or "fd\n<name>\n<size>\n<fd>" (an
// archive; size is -1 when the document does not say).
private static final int RAZDOR_PICK_REQUEST = 7301;
// "" until the picker answers; then as razdorPickOutcome describes.
private volatile String razdorPickResult = "";
// True while the picker is open for a game archive (see above).
private volatile boolean razdorPickingArchive = false;

// The map picker, on the UI thread. See razdorStartPicker.
public String razdorPickFile() {
    return razdorStartPicker(false);
}

// The archive picker, on the UI thread. See razdorStartPicker.
public String razdorPickArchive() {
    return razdorStartPicker(true);
}

// Opens the picker on the UI thread. "" once it is requested; a failure to open it is the poll's
// answer ("error: ..."), so the game learns of it from razdorPickOutcome.
private String razdorStartPicker(final boolean archive) {
    razdorPickResult = "";
    razdorPickingArchive = archive;
    runOnUiThread(new Runnable() {
        @Override
        public void run() {
            try {
                Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
                intent.addCategory(Intent.CATEGORY_OPENABLE);
                intent.setType("*/*");
                startActivityForResult(intent, RAZDOR_PICK_REQUEST);
            } catch (Exception e) {
                razdorPickResult = "error: " + e;
            }
        }
    });
    return "";
}

// The game's poll (see above). Taken as it is: the answer is kept until it is read.
public String razdorPickOutcome() {
    return razdorPickResult;
}

// Called from miniquad's own onActivityResult (the MAIN_ACTIVITY_ON_ACTIVITY_RESULT section at
// the end of this file): MainActivity already defines that method, so it can't be overridden here.
private void razdorOnActivityResult(int requestCode, int resultCode, Intent data) {
    if (requestCode != RAZDOR_PICK_REQUEST) {
        return;
    }
    if (resultCode != RESULT_OK || data == null || data.getData() == null) {
        razdorPickResult = "cancel";
        return;
    }
    final Uri uri = data.getData();
    final boolean archive = razdorPickingArchive;
    // A thread: the copy of a big file, or a document's open, must not stall the activity.
    new Thread(new Runnable() {
        @Override
        public void run() {
            razdorPickResult = archive ? razdorOpenArchive(uri) : razdorCopyPicked(uri);
        }
    }).start();
}

// The picked document's display name, with its line breaks taken out; "picked" when there is none
// (its extension decides what the file is).
private String razdorDisplayName(Uri uri) {
    String name = "picked";
    try (Cursor cursor = getContentResolver().query(uri, new String[] {OpenableColumns.DISPLAY_NAME}, null, null, null)) {
        if (cursor != null && cursor.moveToFirst()) {
            String display = cursor.getString(0);
            if (display != null && !display.isEmpty()) {
                name = display.replace('\n', ' ').replace('\r', ' ');
            }
        }
    } catch (Exception e) {
        // No display name: the file keeps the name "picked".
    }
    return name;
}

// The picked document's size from OpenableColumns.SIZE, -1 when the document does not say.
private long razdorArchiveSize(Uri uri) {
    try (Cursor cursor = getContentResolver().query(uri, new String[] {OpenableColumns.SIZE}, null, null, null)) {
        if (cursor != null && cursor.moveToFirst() && !cursor.isNull(0)) {
            return cursor.getLong(0);
        }
    } catch (Exception e) {
        // No size: the room check is left to the copy's own failure.
    }
    return -1;
}

// Opens the picked archive for reading in place: the descriptor is detached from the ParcelFileDescriptor
// (so nothing closes it here) and handed to Rust, which owns it from then on.
private String razdorOpenArchive(Uri uri) {
    String name = razdorDisplayName(uri);
    long size = razdorArchiveSize(uri);
    try {
        ParcelFileDescriptor pfd = getContentResolver().openFileDescriptor(uri, "r");
        if (pfd == null) {
            return "error: the file has no data";
        }
        return "fd\n" + name + "\n" + size + "\n" + pfd.detachFd();
    } catch (Exception e) {
        return "error: " + e;
    }
}

// Copies the picked document into the app's cache folder; returns the answer for the poll.
private String razdorCopyPicked(Uri uri) {
    String name = razdorDisplayName(uri);
    File dir = new File(getCacheDir(), "razdor-import");
    dir.mkdirs();
    File copy = new File(dir, "picked-" + System.currentTimeMillis());
    try (InputStream in = getContentResolver().openInputStream(uri); OutputStream out = new FileOutputStream(copy)) {
        if (in == null) {
            copy.delete();
            return "error: the file has no data";
        }
        byte[] buffer = new byte[65536];
        int n;
        while ((n = in.read(buffer)) > 0) {
            out.write(buffer, 0, n);
        }
    } catch (Exception e) {
        copy.delete();
        return "error: " + e;
    }
    return "ok\n" + name + "\n" + copy.getAbsolutePath();
}

// Asks Android for the display's highest refresh rate at its current resolution. In the phone's
// dynamic refresh mode Android gives an app 60 Hz unless the app states a preference, and the game
// holds about 120 fps on such phones. Razdor's own: the original is a desktop game. Called on
// create and on each focus gain (some ROMs reset the window attributes after a dialog). Never
// fatal: on any failure the system keeps its own choice.
private void razdorPreferTopRefreshRate() {
    if (Build.VERSION.SDK_INT < 23) {
        return;
    }
    try {
        Display display = Build.VERSION.SDK_INT >= 30 ? getDisplay() : getWindowManager().getDefaultDisplay();
        if (display == null) {
            return;
        }
        Display.Mode current = display.getMode();
        // With a frame rate cap («Ограничение FPS», written to the app folder by src/ui/frame_cap.rs) the app
        // asks for the cap's rate instead: the lowest mode at least that fast, else the top one.
        int cap = razdorFrameCap();
        Display.Mode best = cap > 0 ? razdorModeAtLeast(display, cap) : null;
        if (best == null) {
            best = razdorTopMode(display);
        }
        if (best == null) {
            return;
        }
        WindowManager.LayoutParams params = getWindow().getAttributes();
        if (best.getRefreshRate() > current.getRefreshRate() || best.getModeId() != params.preferredDisplayModeId) {
            params.preferredDisplayModeId = best.getModeId();
            if (Build.VERSION.SDK_INT >= 30) {
                // Some OEM ROMs read the rate itself, not the mode id.
                params.preferredRefreshRate = cap > 0 ? cap : best.getRefreshRate();
            }
            getWindow().setAttributes(params);
        }
    } catch (Throwable t) {
        // Not fatal: the system keeps its own refresh rate choice.
    }
}

// The display's mode with the highest refresh rate at its current resolution, or null. Used by
// the window's preference (razdorPreferTopRefreshRate) and by the surface's vote below.
private Display.Mode razdorTopMode(Display display) {
    Display.Mode current = display.getMode();
    Display.Mode best = null;
    for (Display.Mode mode : display.getSupportedModes()) {
        if (mode.getPhysicalWidth() != current.getPhysicalWidth()
                || mode.getPhysicalHeight() != current.getPhysicalHeight()) {
            continue;
        }
        if (best == null || mode.getRefreshRate() > best.getRefreshRate()) {
            best = mode;
        }
    }
    return best;
}

// The frame rate cap the settings chose (30 or 60; 0 for none): the file src/ui/frame_cap.rs writes to the app
// folder when the setting changes. Never fatal: a missing or unreadable file means no cap.
private int razdorFrameCap() {
    try {
        java.io.BufferedReader reader = new java.io.BufferedReader(new java.io.FileReader("/storage/emulated/0/Razdor/frame-cap"));
        try {
            int cap = Integer.parseInt(reader.readLine().trim());
            return cap == 30 || cap == 60 ? cap : 0;
        } finally {
            reader.close();
        }
    } catch (Throwable t) {
        return 0;
    }
}

// The mode with the lowest refresh rate at least `cap` at the current resolution, or null.
private Display.Mode razdorModeAtLeast(Display display, int cap) {
    Display.Mode current = display.getMode();
    Display.Mode found = null;
    for (Display.Mode mode : display.getSupportedModes()) {
        if (mode.getPhysicalWidth() != current.getPhysicalWidth()
                || mode.getPhysicalHeight() != current.getPhysicalHeight()) {
            continue;
        }
        if (mode.getRefreshRate() + 0.5f < cap) {
            continue;
        }
        if (found == null || mode.getRefreshRate() < found.getRefreshRate()) {
            found = mode;
        }
    }
    return found;
}

// The surface vote (API 30+). miniquad draws into a SurfaceView, and the frame rate that counts
// for the panel is the one the surface asks for, not only the window's: adaptive refresh (OnePlus's
// ADFR) drops a game to 60 Hz while no finger touches the screen unless the surface says it is a
// game. Razdor's, like razdorPreferTopRefreshRate. Voted on each new surface (surfaceCreated), so
// a recreated surface is voted again. Never fatal.
private void razdorWatchSurfaceFrameRate() {
    if (Build.VERSION.SDK_INT < 30) {
        return;
    }
    try {
        view.getHolder().addCallback(new SurfaceHolder.Callback() {
            @Override
            public void surfaceCreated(SurfaceHolder holder) {
                razdorVoteSurfaceFrameRate(holder.getSurface());
            }

            @Override
            public void surfaceChanged(SurfaceHolder holder, int format, int width, int height) {
            }

            @Override
            public void surfaceDestroyed(SurfaceHolder holder) {
            }
        });
        // The surface may already exist (then no callback comes for it).
        Surface now = view.getHolder().getSurface();
        if (now != null && now.isValid()) {
            razdorVoteSurfaceFrameRate(now);
        }
    } catch (Throwable t) {
        // Not fatal: the system keeps its own choice.
    }
}

// Asks the surface for the display's top refresh rate (API 30: the 2-argument form; API 31+: the
// 3-argument form, which lets the system change the rate at any time), and the view for the
// game category (API 35+: REQUESTED_FRAME_RATE_CATEGORY_HIGH). Each call is guarded on its own.
private void razdorVoteSurfaceFrameRate(Surface surface) {
    try {
        Display display = getDisplay();
        Display.Mode best = display == null ? null : razdorTopMode(display);
        int cap = razdorFrameCap();
        if (best != null) {
            // With a frame rate cap the surface asks for the cap's rate (the settings' «Ограничение FPS»).
            float rate = cap > 0 ? cap : best.getRefreshRate();
            if (Build.VERSION.SDK_INT >= 31) {
                surface.setFrameRate(rate, Surface.FRAME_RATE_COMPATIBILITY_FIXED_SOURCE,
                        Surface.CHANGE_FRAME_RATE_ALWAYS);
            } else {
                surface.setFrameRate(rate, Surface.FRAME_RATE_COMPATIBILITY_FIXED_SOURCE);
            }
        }
    } catch (Throwable t) {
        // Not fatal: the surface keeps the system's default vote.
    }
    try {
        if (Build.VERSION.SDK_INT >= 35) {
            view.setRequestedFrameRate(View.REQUESTED_FRAME_RATE_CATEGORY_HIGH);
        }
    } catch (Throwable t) {
        // Not fatal: the view keeps the system's default.
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

// Quitting («Выход»): the app leaves the screen at once, with the system's own animation, and the
// game writes its saves off the screen (src/ui/android.rs, `hide_for_exit`; `exit_now` in
// src/main.rs). Called from the game's thread; the move is made on the UI thread. Without it the
// window stays frozen on the screen until the process is gone, and the system removes it only
// after it notices the death.
public void razdorKeepAwake(final boolean on) {
    runOnUiThread(new Runnable() {
        @Override
        public void run() {
            if (on) {
                getWindow().addFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
            } else {
                getWindow().clearFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
            }
        }
    });
}

public void razdorHideForExit() {
    runOnUiThread(new Runnable() {
        @Override
        public void run() {
            moveTaskToBack(true);
        }
    });
}

@Override
public void onWindowFocusChanged(boolean hasFocus) {
    super.onWindowFocusChanged(hasFocus);
    if (hasFocus) {
        razdorPreferTopRefreshRate();
    }
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
razdorPreferTopRefreshRate();
razdorWatchSurfaceFrameRate();

//% END

//% MAIN_ACTIVITY_ON_ACTIVITY_RESULT

razdorOnActivityResult(requestCode, resultCode, data);

//% END

//% MAIN_ACTIVITY_ON_PAUSE

// The session slot is written now, while the activity still runs: the frame loop stops in the
// background, so the Rust side (src/ui/android.rs, razdorOnPause) cannot write it on a frame.
razdorOnPause();

//% END
