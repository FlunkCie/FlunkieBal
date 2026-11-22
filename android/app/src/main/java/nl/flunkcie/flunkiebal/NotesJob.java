package nl.flunkcie.flunkiebal;

import android.app.job.JobParameters;
import android.app.job.JobService;

/** The background check for notifications, every quarter of an hour or so (see Notes.check). */
public class NotesJob extends JobService {
    @Override
    public boolean onStartJob(JobParameters params) {
        new Thread(() -> {
            Notes.check(getApplicationContext());
            jobFinished(params, false);
        }, "notes-check").start();
        return true;
    }

    @Override
    public boolean onStopJob(JobParameters params) {
        return true;
    }
}
