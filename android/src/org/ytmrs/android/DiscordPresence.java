package org.ytmrs.android;

import android.app.Activity;
import android.os.Handler;
import android.os.HandlerThread;
import java.io.File;
import java.nio.charset.StandardCharsets;

/** Owns the SDK client and pumps its callbacks on one thread. */
final class DiscordPresence {
    private final HandlerThread thread = new HandlerThread("ytm-discord");
    private final Handler worker;
    private long client;
    private final Runnable heartbeat = this::pump;

    static boolean bundled(Activity activity) {
        return new File(activity.getApplicationInfo().nativeLibraryDir, "libytm_discord.so").isFile();
    }

    DiscordPresence(Activity activity) throws ReflectiveOperationException {
        Class.forName("com.discord.socialsdk.DiscordSocialSdkInit")
            .getMethod("setEngineActivity", Activity.class).invoke(null, activity);
        System.loadLibrary("ytm_discord");
        thread.start();
        worker = new Handler(thread.getLooper());
    }

    void set(String title, String artist, String album, String art, String url, long start, long end) {
        worker.post(() -> {
            if (client == 0 && title != null) client = create();
            if (client == 0) return;
            if (title == null) {
                clear(client);
            } else {
                publish(client, bytes(title), bytes(artist), bytes(album), bytes(art), bytes(url), start, end);
            }
            worker.removeCallbacks(heartbeat);
            pump();
        });
    }

    void close() {
        worker.post(() -> {
            worker.removeCallbacks(heartbeat);
            if (client != 0) {
                clear(client);
                tick(client);
                destroy(client);
                client = 0;
            }
            thread.quitSafely();
        });
    }

    private void pump() {
        if (client != 0 && tick(client)) worker.postDelayed(heartbeat, 250L);
    }

    private static byte[] bytes(String value) {
        return value.getBytes(StandardCharsets.UTF_8);
    }

    private static native long create();
    private static native void publish(long client, byte[] title, byte[] artist, byte[] album,
                                       byte[] art, byte[] url, long start, long end);
    private static native void clear(long client);
    private static native boolean tick(long client);
    private static native void destroy(long client);
}
