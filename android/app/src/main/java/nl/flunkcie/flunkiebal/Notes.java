package nl.flunkcie.flunkiebal;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.job.JobInfo;
import android.app.job.JobScheduler;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.content.SharedPreferences;
import android.util.Log;

import org.json.JSONArray;
import org.json.JSONObject;

import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.nio.charset.StandardCharsets;

/**
 * Notifications for invites and messages from friends. While the game runs they come in on its own
 * connection (MainActivity.note); while it's closed, NotesJob asks the server now and then (a plain
 * HTTP POST to /api/notes, see server/src/main.rs). Either way each note is shown once: the prefs
 * keep the last note id shown, per account.
 */
final class Notes {
    private static final String TAG = "flunkiebal";
    static final String PREFS = "notes";
    private static final String CH_FRIENDS = "friends";
    private static final String CH_UPDATES = "updates";
    private static final int JOB = 1;
    /** Mirrors proto::NOTE_INVITE. */
    static final int INVITE = 0;

    private Notes() {}

    static SharedPreferences prefs(Context c) {
        return c.getSharedPreferences(PREFS, Context.MODE_PRIVATE);
    }

    /** Who to check for (url empty or off: nobody), and the background check on or off with it. */
    static void account(Context c, String url, String code, String secret, boolean on) {
        prefs(c).edit().putString("url", url).putString("code", code).putString("secret", secret).putBoolean("on", on).apply();
        // a background check that can't be set up costs notifications, never the game
        try {
            JobScheduler js = (JobScheduler) c.getSystemService(Context.JOB_SCHEDULER_SERVICE);
            if (js == null) return;
            if (on && !code.isEmpty()) {
                JobInfo job = new JobInfo.Builder(JOB, new ComponentName(c, NotesJob.class))
                        .setRequiredNetworkType(JobInfo.NETWORK_TYPE_ANY)
                        .setPeriodic(15 * 60 * 1000L)
                        .setPersisted(true)
                        .build();
                js.schedule(job);
            } else {
                js.cancel(JOB);
            }
        } catch (Exception e) {
            Log.w(TAG, "notes job", e);
        }
    }

    /** A note, from the game or the background check: shown if it's new and nobody is looking at the game. */
    static synchronized void offer(Context c, long id, int kind, String from, String name, String text, String room, boolean looking) {
        SharedPreferences p = prefs(c);
        String key = "after_" + p.getString("code", "");
        if (id <= p.getLong(key, 0)) return;
        p.edit().putLong(key, id).apply();
        if (looking || !p.getBoolean("on", false)) return;
        String title = kind == INVITE ? name + " invites you" : name;
        String body = kind == INVITE ? "Come play FlunkieBal: match " + room : text;
        String open = kind == INVITE ? "room:" + room : "chat:" + from;
        show(c, CH_FRIENDS, (int) (id % 100000) + 10, title, body, open);
    }

    static void show(Context c, String channel, int nid, String title, String body, String open) {
        try {
            NotificationManager nm = (NotificationManager) c.getSystemService(Context.NOTIFICATION_SERVICE);
            if (nm == null) return;
            if (nm.getNotificationChannel(CH_FRIENDS) == null) {
                nm.createNotificationChannel(new NotificationChannel(CH_FRIENDS, "Invites and messages", NotificationManager.IMPORTANCE_HIGH));
                nm.createNotificationChannel(new NotificationChannel(CH_UPDATES, "New versions", NotificationManager.IMPORTANCE_DEFAULT));
            }
            Intent i = new Intent(c, MainActivity.class).putExtra("open", open).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_SINGLE_TOP);
            PendingIntent pi = PendingIntent.getActivity(c, nid, i, PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE);
            Notification n = new Notification.Builder(c, channel)
                    .setSmallIcon(R.mipmap.ic_launcher)
                    .setContentTitle(title)
                    .setContentText(body)
                    .setStyle(new Notification.BigTextStyle().bigText(body))
                    .setContentIntent(pi)
                    .setAutoCancel(true)
                    .build();
            nm.notify(nid, n);
        } catch (Exception e) {
            Log.w(TAG, "notification", e);
        }
    }

    /** The background check: runs on a thread of NotesJob's, with the game closed (or in the background). */
    @SuppressWarnings("deprecation")
    static void check(Context c) {
        SharedPreferences p = prefs(c);
        String url = p.getString("url", ""), code = p.getString("code", ""), secret = p.getString("secret", "");
        if (!p.getBoolean("on", false) || url.isEmpty() || code.isEmpty()) return;
        String key = "after_" + code;
        boolean first = !p.contains(key);
        try {
            JSONObject req = new JSONObject().put("code", code).put("secret", secret).put("after", p.getLong(key, 0));
            HttpURLConnection h = (HttpURLConnection) new URL(url + "/api/notes").openConnection();
            h.setConnectTimeout(10000);
            h.setReadTimeout(10000);
            h.setRequestMethod("POST");
            h.setDoOutput(true);
            h.setRequestProperty("Content-Type", "application/json");
            try (OutputStream o = h.getOutputStream()) {
                o.write(req.toString().getBytes(StandardCharsets.UTF_8));
            }
            if (h.getResponseCode() != 200) {
                Log.i(TAG, "notes check: HTTP " + h.getResponseCode());
                return;
            }
            ByteArrayOutputStream b = new ByteArrayOutputStream();
            try (InputStream in = h.getInputStream()) {
                byte[] buf = new byte[4096];
                for (int n; (n = in.read(buf)) > 0; ) b.write(buf, 0, n);
            }
            JSONObject res = new JSONObject(b.toString("UTF-8"));
            JSONArray notes = res.optJSONArray("notes");
            for (int k = 0; notes != null && k < notes.length(); k++) {
                JSONObject n = notes.getJSONObject(k);
                // a new phone or account: what came before isn't news
                offer(c, n.getLong("id"), n.getInt("kind"), n.optString("from"), n.optString("name"), n.optString("text"), n.optString("room"), first || MainActivity.looking);
            }
            int build = res.optInt("build", 0);
            int mine = c.getPackageManager().getPackageInfo(c.getPackageName(), 0).versionCode;
            if (build > mine && p.getInt("update_told", 0) < build && !MainActivity.looking) {
                p.edit().putInt("update_told", build).apply();
                show(c, CH_UPDATES, 2, "A new FlunkieBal is out", "Open the game to update", "update");
            }
        } catch (Exception e) {
            Log.i(TAG, "notes check failed: " + e);
        }
    }
}
