package nl.flunkcie.flunkiebal;

import android.app.AlertDialog;
import android.app.NativeActivity;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
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

    @Override
    protected void onCreate(Bundle state) {
        Window w = getWindow();
        w.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        WindowManager.LayoutParams lp = w.getAttributes();
        if (Build.VERSION.SDK_INT >= 28) {
            lp.layoutInDisplayCutoutMode = WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES;
        }
        w.setAttributes(lp);
        super.onCreate(state);
        hideSystemBars();
        tts = new TextToSpeech(this, status -> ttsReady = status == TextToSpeech.SUCCESS);
    }

    // ---------- called by the game ----------
    /** Quick-message voices: the phone's own text-to-speech says the line. */
    public void speak(String text, String lang, float pitch, float rate, float volume) {
        if (!ttsReady || tts == null || text == null) return;
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

    public void vibrate(int ms) {
        Vibrator v = (Vibrator) getSystemService(Context.VIBRATOR_SERVICE);
        if (v != null && v.hasVibrator()) v.vibrate(VibrationEffect.createOneShot(Math.max(1, ms), VibrationEffect.DEFAULT_AMPLITUDE));
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
