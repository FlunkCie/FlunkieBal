package nl.flunkcie.flunkiebal;

import android.app.AlertDialog;
import android.app.NativeActivity;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageInstaller;
import android.content.pm.PackageManager;
import android.net.Uri;
import android.provider.Settings;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.os.VibrationEffect;
import android.os.Vibrator;
import android.speech.tts.TextToSpeech;
import android.text.InputFilter;
import android.text.InputType;
import android.util.Log;
import android.view.View;
import android.view.Window;
import android.view.WindowManager;
import android.widget.EditText;
import android.widget.FrameLayout;

/**
 * The game itself is native code (libflunkiebal.so, see src/lib.rs: android_main). This activity
 * sets up the window and does what only Java can do; the game calls the public methods over JNI
 * (src/android.rs), from its own thread.
 */
public class MainActivity extends NativeActivity {
    private static final String TAG = "flunkiebal";
    private final Handler ui = new Handler(Looper.getMainLooper());
    private TextToSpeech tts;
    private volatile boolean ttsReady = false;
    private volatile String textAnswer = null;
    private volatile String insets = "0,0,0,0";
    /** The game is on screen: notifications wait (the game shows invites and messages itself). */
    static volatile boolean looking = false;
    /** What a tapped notification asks for ("room:CODE", "chat:CODE", "update"), for takeOpen. */
    private volatile String open = null;
    /** How an update is going, for updateState. */
    private volatile String update = "";

    @Override
    protected void onCreate(Bundle state) {
        Window w = getWindow();
        w.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        WindowManager.LayoutParams lp = w.getAttributes();
        if (Build.VERSION.SDK_INT >= 28) {
            lp.layoutInDisplayCutoutMode = WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES;
        }
        lp.preferredRefreshRate = refreshRate(false);
        w.setAttributes(lp);
        super.onCreate(state);
        hideSystemBars();
        tts = new TextToSpeech(this, status -> ttsReady = status == TextToSpeech.SUCCESS);
        opened(getIntent());
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        opened(intent);
    }

    private void opened(Intent i) {
        if (i == null) return;
        String o = i.getStringExtra("open");
        if (o != null) {
            Log.i(TAG, "opened for " + o);
            open = o;
        }
        if (i.hasExtra(PackageInstaller.EXTRA_STATUS)) installed(i);
    }

    @Override
    protected void onResume() {
        super.onResume();
        looking = true;
    }

    @Override
    protected void onPause() {
        looking = false;
        super.onPause();
    }

    // ---------- notifications ----------
    /** Who the background check asks for; see Notes. */
    public void notesAccount(String url, String code, String secret, boolean on) {
        bg.execute(() -> Notes.account(getApplicationContext(), url, code, secret, on));
    }

    /** Android 13 and up ask the player before an app may show notifications; once is enough. */
    public void askNotify() {
        if (Build.VERSION.SDK_INT < 33) return;
        ui.post(() -> {
            if (checkSelfPermission("android.permission.POST_NOTIFICATIONS") == PackageManager.PERMISSION_GRANTED) return;
            android.content.SharedPreferences p = Notes.prefs(this);
            if (p.getBoolean("asked", false)) return;
            p.edit().putBoolean("asked", true).apply();
            requestPermissions(new String[] { "android.permission.POST_NOTIFICATIONS" }, 1);
        });
    }

    /** A note that came in on the game's connection (maybe from its socket's thread, in the background). */
    public void note(long id, int kind, String from, String name, String text, String room) {
        Notes.offer(getApplicationContext(), id, kind, from, name, text, room, looking);
    }

    public String takeOpen() {
        String o = open;
        open = null;
        return o;
    }

    // ---------- updating ----------
    /** Download the newest APK into a PackageInstaller session; the system then asks the player. */
    public void update(String url) {
        if (update.startsWith("get:")) return;
        if (!getPackageManager().canRequestPackageInstalls()) {
            update = "err:allow FlunkieBal to install apps, then tap again";
            ui.post(() -> {
                try {
                    startActivity(new Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:" + getPackageName())));
                } catch (Exception e) {
                    Log.w(TAG, "unknown sources settings", e);
                }
            });
            return;
        }
        update = "get:0";
        new Thread(() -> {
            PackageInstaller pi = getPackageManager().getPackageInstaller();
            int sid = -1;
            try {
                java.net.HttpURLConnection h = (java.net.HttpURLConnection) new java.net.URL(url).openConnection();
                h.setConnectTimeout(15000);
                h.setReadTimeout(30000);
                if (h.getResponseCode() != 200) throw new java.io.IOException("the download said " + h.getResponseCode());
                long len = h.getContentLengthLong();
                PackageInstaller.SessionParams sp = new PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL);
                sp.setAppPackageName(getPackageName());
                if (len > 0) sp.setSize(len);
                sid = pi.createSession(sp);
                try (PackageInstaller.Session s = pi.openSession(sid)) {
                    try (java.io.InputStream in = h.getInputStream(); java.io.OutputStream out = s.openWrite("flunkiebal.apk", 0, len > 0 ? len : -1)) {
                        byte[] buf = new byte[65536];
                        long got = 0;
                        for (int n; (n = in.read(buf)) > 0; ) {
                            out.write(buf, 0, n);
                            got += n;
                            if (len > 0) update = "get:" + (got * 100 / len);
                        }
                        s.fsync(out);
                    }
                    // the answer comes back to this activity (opened → installed)
                    Intent back = new Intent(this, MainActivity.class).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP);
                    int mutable = Build.VERSION.SDK_INT >= 31 ? 0x02000000 /* FLAG_MUTABLE */ : 0;
                    android.app.PendingIntent done = android.app.PendingIntent.getActivity(this, 7, back, android.app.PendingIntent.FLAG_UPDATE_CURRENT | mutable);
                    s.commit(done.getIntentSender());
                }
                update = "install";
            } catch (Exception e) {
                Log.w(TAG, "update", e);
                if (sid >= 0) {
                    try { pi.abandonSession(sid); } catch (Exception x) { /* gone already */ }
                }
                update = "err:" + (e.getMessage() == null ? "no connection" : e.getMessage());
            }
        }, "update").start();
    }

    /** The installer's answer: it wants the player to confirm, or it's done (we're replaced then). */
    private void installed(Intent i) {
        int st = i.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE);
        if (st == PackageInstaller.STATUS_PENDING_USER_ACTION) {
            Intent confirm = i.getParcelableExtra(Intent.EXTRA_INTENT);
            if (confirm != null) {
                try {
                    startActivity(confirm.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
                } catch (Exception e) {
                    update = "err:" + e.getMessage();
                }
            }
        } else if (st != PackageInstaller.STATUS_SUCCESS) {
            String m = i.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE);
            update = "err:" + (st == PackageInstaller.STATUS_FAILURE_ABORTED ? "cancelled" : m == null ? "the installer said no" : m);
        }
    }

    public String updateState() {
        return update;
    }

    // ---------- called by the game ----------
    /** Quick-message voices: the phone's own text-to-speech says the line. */
    public void speak(String text, String lang, float pitch, float rate, float volume) {
        if (!ttsReady || tts == null || text == null) return;
        // the speech service is asked several things: not on the game's thread
        bg.execute(() -> say(text, lang, pitch, rate, volume));
    }

    private void say(String text, String lang, float pitch, float rate, float volume) {
        try {
            java.util.Locale loc = java.util.Locale.forLanguageTag(lang == null ? "en-GB" : lang);
            if (tts.isLanguageAvailable(loc) >= TextToSpeech.LANG_AVAILABLE) tts.setLanguage(loc);
            else tts.setLanguage(java.util.Locale.UK);
            tts.setPitch(Math.max(0.5f, Math.min(2f, pitch)));
            tts.setSpeechRate(Math.max(0.5f, Math.min(2f, rate)));
            Bundle b = new Bundle();
            b.putFloat(TextToSpeech.Engine.KEY_PARAM_VOLUME, Math.max(0f, Math.min(1f, volume)));
            tts.speak(text.length() > 60 ? text.substring(0, 60) : text, TextToSpeech.QUEUE_ADD, b, "qm" + System.nanoTime());
        } catch (Exception e) {
            Log.w(TAG, "speak", e);
        }
    }

    /** Calls into the system (vibrator, speech) go here, off the game's thread: slow on some phones (a hitch on HIT!). */
    private final java.util.concurrent.ExecutorService bg = java.util.concurrent.Executors.newSingleThreadExecutor();
    private Vibrator vibrator;

    public void vibrate(int ms) {
        bg.execute(() -> {
            if (vibrator == null) vibrator = (Vibrator) getSystemService(Context.VIBRATOR_SERVICE);
            Vibrator v = vibrator;
            if (v != null && v.hasVibrator()) v.vibrate(VibrationEffect.createOneShot(Math.max(1, ms), VibrationEffect.DEFAULT_AMPLITUDE));
        });
    }

    /** A 90 or 120 Hz screen has the game draw every frame twice as often, and heats the phone for it:
     *  60 Hz where the screen has it, unless the player asks for the most (graphics settings). */
    private float refreshRate(boolean max) {
        float best = 0f, sixty = 0f;
        for (float hz : getWindowManager().getDefaultDisplay().getSupportedRefreshRates()) {
            best = Math.max(best, hz);
            if (Math.abs(hz - 60f) < 1f) sixty = hz;
        }
        return max || sixty == 0f ? best : sixty;
    }

    public void maxFps(boolean max) {
        ui.post(() -> {
            WindowManager.LayoutParams lp = getWindow().getAttributes();
            lp.preferredRefreshRate = refreshRate(max);
            getWindow().setAttributes(lp);
        });
    }

    /** The display cutout (notch, punch hole) as top,right,bottom,left in density-independent pixels. */
    public String insets() {
        ui.post(this::readInsets);
        return insets;
    }

    private void readInsets() {
        try {
            if (Build.VERSION.SDK_INT >= 28) {
                android.view.WindowInsets wi = getWindow().getDecorView().getRootWindowInsets();
                if (wi != null && wi.getDisplayCutout() != null) {
                    android.view.DisplayCutout c = wi.getDisplayCutout();
                    float d = getResources().getDisplayMetrics().density;
                    insets = (c.getSafeInsetTop() / d) + "," + (c.getSafeInsetRight() / d) + "," + (c.getSafeInsetBottom() / d) + "," + (c.getSafeInsetLeft() / d);
                }
            }
        } catch (Exception e) {
            Log.w(TAG, "insets", e);
        }
    }

    /** A code to paste somewhere else. */
    public void copyText(String text) {
        ui.post(() -> {
            try {
                ClipboardManager cm = (ClipboardManager) getSystemService(Context.CLIPBOARD_SERVICE);
                if (cm != null) cm.setPrimaryClip(ClipData.newPlainText("FlunkieBal", text));
            } catch (Exception e) {
                Log.w(TAG, "copyText", e);
            }
        });
    }

    public void leave() {
        ui.post(() -> moveTaskToBack(true));
    }

    /** A text prompt with the phone's keyboard. The answer waits for pollText. */
    public void askText(String field, String current) {
        textAnswer = null;
        ui.post(() -> showPrompt(field, current));
    }

    /** "=text" when OK was pressed, "!" when cancelled, null while the prompt is open. */
    public String pollText() {
        String a = textAnswer;
        if (a != null) textAnswer = null;
        return a;
    }

    private void showPrompt(String field, String current) {
        EditText ed = new EditText(this);
        ed.setSingleLine(true);
        String title;
        switch (field) {
            case "code":
                title = "Match code";
                ed.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_CAP_CHARACTERS | InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS);
                ed.setFilters(new InputFilter[] { new InputFilter.LengthFilter(4), new InputFilter.AllCaps() });
                break;
            case "friendcode":
                title = "Friend code";
                ed.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_CAP_CHARACTERS | InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS);
                ed.setFilters(new InputFilter[] { new InputFilter.LengthFilter(8), new InputFilter.AllCaps() });
                break;
            case "loginuser":
                title = "Username";
                ed.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS);
                ed.setFilters(new InputFilter[] { new InputFilter.LengthFilter(16) });
                break;
            case "loginpass":
                title = "Password";
                ed.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_PASSWORD);
                ed.setFilters(new InputFilter[] { new InputFilter.LengthFilter(128) });
                break;
            case "bug":
                title = "What went wrong?";
                ed.setSingleLine(false);
                ed.setMinLines(3);
                ed.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_MULTI_LINE | InputType.TYPE_TEXT_FLAG_CAP_SENTENCES);
                ed.setFilters(new InputFilter[] { new InputFilter.LengthFilter(1000) });
                break;
            case "chat":
                title = "Message";
                ed.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_CAP_SENTENCES);
                ed.setFilters(new InputFilter[] { new InputFilter.LengthFilter(300) });
                break;
            default:
                title = "Your name";
                ed.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_CAP_WORDS);
                ed.setFilters(new InputFilter[] { new InputFilter.LengthFilter(16) });
        }
        ed.setText(current);
        ed.setSelection(ed.getText().length());
        FrameLayout box = new FrameLayout(this);
        int pad = (int) (22 * getResources().getDisplayMetrics().density);
        box.setPadding(pad, pad / 2, pad, 0);
        box.addView(ed);
        AlertDialog d = new AlertDialog.Builder(this, android.R.style.Theme_DeviceDefault_Dialog_Alert)
                .setTitle(title)
                .setView(box)
                .setPositiveButton(android.R.string.ok, (x, y) -> textAnswer = "=" + ed.getText())
                .setNegativeButton(android.R.string.cancel, (x, y) -> textAnswer = "!")
                .setOnCancelListener(x -> textAnswer = "!")
                .create();
        d.getWindow().setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_STATE_ALWAYS_VISIBLE);
        d.setOnDismissListener(x -> hideSystemBars());
        d.show();
        ed.requestFocus();
    }

    // ---------- the window ----------
    @SuppressWarnings("deprecation")
    private void hideSystemBars() {
        getWindow().getDecorView().setSystemUiVisibility(View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY | View.SYSTEM_UI_FLAG_FULLSCREEN
                | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION | View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN);
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (hasFocus) hideSystemBars();
    }

    @Override
    protected void onDestroy() {
        if (tts != null) tts.shutdown();
        // The game's loop runs for the life of the process and doesn't end with the activity, and
        // NativeActivity.onDestroy waits for it to: that would freeze us for good on every recreation
        // (a configuration change the manifest doesn't list, say). A clean start instead: Android
        // brings the activity back in a new process if it's still wanted.
        Log.i(TAG, "activity destroyed (finishing=" + isFinishing() + "), ending the process");
        android.os.Process.killProcess(android.os.Process.myPid());
        super.onDestroy();
    }
}
