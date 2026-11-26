package org.ytmrs.android;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Intent;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.IntentFilter;
import android.media.AudioAttributes;
import android.media.AudioFocusRequest;
import android.media.AudioManager;
import android.os.Build;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.media.session.MediaSession;
import android.media.session.PlaybackState;
import android.net.Uri;
import android.os.IBinder;
import android.os.Handler;
import android.os.Looper;
import android.os.PowerManager;

public final class PlaybackService extends Service {
    static final String UPDATE = "org.ytmrs.android.UPDATE";
    private static final String CHANNEL = "playback";
    private static final int NOTIFICATION = 11;
    private static final int MAX_ART_EDGE = 256;
    private static final long PAUSED_FOREGROUND_TIMEOUT_MS = 10 * 60 * 1000L;
    private static PlaybackService active;
    private MediaSession session;
    private AudioManager audio;
    private AudioFocusRequest focusRequest;
    private boolean focusRequested;
    private boolean focusWaiting;
    private Intent lastUpdate;
    private Notification lastNotification;
    private String artworkUri;
    private Bitmap artwork;
    private final Handler main = new Handler(Looper.getMainLooper());
    private final BroadcastReceiver noisy = new BroadcastReceiver() {
        @Override public void onReceive(Context context, Intent intent) {
            if (AudioManager.ACTION_AUDIO_BECOMING_NOISY.equals(intent.getAction()) && lastUpdate != null
                && lastUpdate.getBooleanExtra("playing", false)) {
                dispatchAction(2, 0);
            }
        }
    };
    private PowerManager.WakeLock wakeLock;
    private boolean foreground;
    private final Runnable leavePausedForeground = () -> {
        if (!foreground || lastUpdate == null || lastUpdate.getBooleanExtra("playing", false)
            || focusWaiting) return;
        stopForeground(STOP_FOREGROUND_DETACH);
        foreground = false;
    };

    private static native void mediaAction(int action, long position);
    private static native void audioFocus(int change);

    @Override public void onCreate() {
        super.onCreate();
        active = this;
        audio = getSystemService(AudioManager.class);
        focusRequest = new AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
            .setAudioAttributes(new AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA)
                .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC).build())
            .setWillPauseWhenDucked(true)
            .setAcceptsDelayedFocusGain(true)
            .setOnAudioFocusChangeListener(this::focusChanged, main).build();
        IntentFilter unplug = new IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY);
        if (Build.VERSION.SDK_INT >= 33) registerReceiver(noisy, unplug, Context.RECEIVER_NOT_EXPORTED);
        else registerReceiver(noisy, unplug);
        session = new MediaSession(this, "ytm-rs");
        session.setCallback(new MediaSession.Callback() {
            @Override public void onPlay() { dispatchAction(1, 0); }
            @Override public void onPause() { dispatchAction(2, 0); }
            @Override public void onSkipToNext() { mediaAction(3, 0); }
            @Override public void onSkipToPrevious() { mediaAction(4, 0); }
            @Override public void onSeekTo(long position) { mediaAction(5, position); }
            @Override public void onStop() { dispatchAction(6, 0); }
        });
        session.setSessionActivity(PendingIntent.getActivity(this, 0,
            new Intent(this, PlayerActivity.class), PendingIntent.FLAG_IMMUTABLE));
        session.setActive(true);
        NotificationManager manager = getSystemService(NotificationManager.class);
        manager.createNotificationChannel(new NotificationChannel(CHANNEL, "Music playback", NotificationManager.IMPORTANCE_LOW));
        PowerManager power = getSystemService(PowerManager.class);
        wakeLock = power.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "ytm-rs:playback");
        wakeLock.setReferenceCounted(false);
    }

    @Override public int onStartCommand(Intent intent, int flags, int id) {
        if (intent == null) return START_NOT_STICKY;
        if (!UPDATE.equals(intent.getAction())) {
            dispatchAction(intent.getIntExtra("action", 0), 0);
            return START_NOT_STICKY;
        }
        update(intent);
        return START_NOT_STICKY;
    }

    // Called on Android's main thread. Updating the existing service directly also
    // works after the Activity goes into the background.
    static boolean deliver(Intent update) {
        if (active == null) return false;
        active.update(update);
        return true;
    }

    private void dispatchAction(int action, long position) {
        if (action == 1 && !foreground && lastNotification != null) {
            // Promote during the user's media command, before the native engine reports playing.
            if (!enterForeground(lastNotification)) return;
            main.postDelayed(leavePausedForeground, PAUSED_FOREGROUND_TIMEOUT_MS);
        }
        mediaAction(action, position);
        if (action == 2 || action == 6) cancelAutomaticResume();
    }

    static void cancelAutomaticResume() {
        if (active == null || !active.focusWaiting) return;
        PlaybackService service = active;
        service.releaseAudioFocus();
        audioFocus(AudioManager.AUDIOFOCUS_LOSS);
        if (service.lastUpdate != null) service.update(new Intent(service.lastUpdate).putExtra("playing", false));
    }

    private void focusChanged(int change) {
        if (!focusRequested) return;
        switch (change) {
            case AudioManager.AUDIOFOCUS_LOSS_TRANSIENT:
            case AudioManager.AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK:
                if (!focusWaiting) {
                    focusWaiting = true;
                    audioFocus(change);
                    if (lastUpdate != null) update(lastUpdate);
                }
                break;
            case AudioManager.AUDIOFOCUS_GAIN:
                if (focusWaiting) {
                    focusWaiting = false;
                    audioFocus(change);
                    // If the engine was manually paused meanwhile, it will stay paused.
                    main.postDelayed(() -> {
                        if (active == this && !focusWaiting && lastUpdate != null
                            && !lastUpdate.getBooleanExtra("playing", false)) update(lastUpdate);
                    }, 1500);
                }
                break;
            case AudioManager.AUDIOFOCUS_LOSS:
                releaseAudioFocus();
                audioFocus(change);
                if (lastUpdate != null) update(new Intent(lastUpdate).putExtra("playing", false));
                break;
            default: break;
        }
    }

    private void releaseAudioFocus() {
        focusWaiting = false;
        if (focusRequested) {
            focusRequested = false;
            audio.abandonAudioFocusRequest(focusRequest);
        }
    }

    private boolean enterForeground(Notification notification) {
        try {
            startForeground(NOTIFICATION, notification);
            foreground = true;
            return true;
        } catch (IllegalStateException error) {
            android.util.Log.w("ytm-rs", "Android denied foreground playback", error);
            return false;
        }
    }

    private Bitmap trackArtwork(String cover) {
        if (java.util.Objects.equals(cover, artworkUri)) return artwork;
        artworkUri = cover;
        artwork = null;
        if (cover != null && cover.startsWith("file:")) {
            Bitmap decoded = BitmapFactory.decodeFile(Uri.parse(cover).getPath());
            if (decoded != null) {
                int edge = Math.max(decoded.getWidth(), decoded.getHeight());
                artwork = edge <= MAX_ART_EDGE ? decoded : Bitmap.createScaledBitmap(decoded,
                    Math.max(1, decoded.getWidth() * MAX_ART_EDGE / edge),
                    Math.max(1, decoded.getHeight() * MAX_ART_EDGE / edge), true);
            }
        }
        return artwork;
    }

    private void update(Intent intent) {
        boolean wasPlaying = lastUpdate != null && lastUpdate.getBooleanExtra("playing", false);
        lastUpdate = intent;
        boolean requested = intent.getBooleanExtra("playing", false);
        boolean playing = requested && !focusWaiting;
        boolean pauseButton = playing || focusWaiting;
        String title = intent.getStringExtra("title");
        String artist = intent.getStringExtra("artist");
        String album = intent.getStringExtra("album");
        String cover = intent.getStringExtra("cover");
        android.media.MediaMetadata.Builder metadata = new android.media.MediaMetadata.Builder()
            .putString(android.media.MediaMetadata.METADATA_KEY_TITLE, title)
            .putString(android.media.MediaMetadata.METADATA_KEY_ARTIST, artist)
            .putString(android.media.MediaMetadata.METADATA_KEY_ALBUM, album);
        long duration = intent.getLongExtra("duration", -1);
        if (duration >= 0) metadata.putLong(android.media.MediaMetadata.METADATA_KEY_DURATION, duration);
        Bitmap bitmap = trackArtwork(cover);
        if (bitmap != null) {
            metadata.putBitmap(android.media.MediaMetadata.METADATA_KEY_ALBUM_ART, bitmap)
                .putBitmap(android.media.MediaMetadata.METADATA_KEY_ART, bitmap)
                .putBitmap(android.media.MediaMetadata.METADATA_KEY_DISPLAY_ICON, bitmap);
        }
        session.setMetadata(metadata.build());
        long actions = PlaybackState.ACTION_PLAY | PlaybackState.ACTION_PAUSE | PlaybackState.ACTION_PLAY_PAUSE |
            PlaybackState.ACTION_SKIP_TO_NEXT | PlaybackState.ACTION_SKIP_TO_PREVIOUS | PlaybackState.ACTION_SEEK_TO |
            PlaybackState.ACTION_STOP;
        session.setPlaybackState(new PlaybackState.Builder().setActions(actions)
            .setState(playing ? PlaybackState.STATE_PLAYING : PlaybackState.STATE_PAUSED,
                intent.getLongExtra("position", 0), playing ? 1.0f : 0.0f).build());

        Notification notification = new Notification.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.app_icon).setContentTitle(title).setContentText(artist)
            .setContentIntent(session.getController().getSessionActivity()).setLargeIcon(bitmap)
            .setVisibility(Notification.VISIBILITY_PUBLIC).setOnlyAlertOnce(true).setOngoing(playing)
            .addAction(action(android.R.drawable.ic_media_previous, "Previous", 4))
            .addAction(action(pauseButton ? android.R.drawable.ic_media_pause : android.R.drawable.ic_media_play,
                pauseButton ? "Pause" : "Play", pauseButton ? 2 : 1))
            .addAction(action(android.R.drawable.ic_media_next, "Next", 3))
            .setStyle(new Notification.MediaStyle().setMediaSession(session.getSessionToken()).setShowActionsInCompactView(0, 1, 2))
            .build();
        lastNotification = notification;
        if (requested || focusWaiting) {
            main.removeCallbacks(leavePausedForeground);
            if (!enterForeground(notification)) {
                releaseAudioFocus();
                audioFocus(AudioManager.AUDIOFOCUS_LOSS);
                update(new Intent(intent).putExtra("playing", false));
                return;
            }
            if (!focusRequested && requested) {
                int result = audio.requestAudioFocus(focusRequest);
                focusRequested = result == AudioManager.AUDIOFOCUS_REQUEST_GRANTED
                    || result == AudioManager.AUDIOFOCUS_REQUEST_DELAYED;
                if (result == AudioManager.AUDIOFOCUS_REQUEST_DELAYED) {
                    focusChanged(AudioManager.AUDIOFOCUS_LOSS_TRANSIENT);
                    return;
                }
                if (!focusRequested) audioFocus(AudioManager.AUDIOFOCUS_LOSS);
            }
        } else {
            releaseAudioFocus();
            // Keep the session available for a follow-up Play command from the launcher.
            // The paused player releases both audio focus and the wake lock.
            if (foreground && wasPlaying) {
                main.removeCallbacks(leavePausedForeground);
                main.postDelayed(leavePausedForeground, PAUSED_FOREGROUND_TIMEOUT_MS);
            }
            getSystemService(NotificationManager.class).notify(NOTIFICATION, notification);
        }
        if (playing && focusRequested) {
            if (!wakeLock.isHeld()) wakeLock.acquire(4 * 60 * 60 * 1000L);
        } else if (wakeLock.isHeld()) {
            wakeLock.release();
        }
    }

    private Notification.Action action(int icon, String title, int id) {
        Intent command = new Intent(this, PlaybackService.class).putExtra("action", id);
        return new Notification.Action.Builder(icon, title, PendingIntent.getService(this, id, command,
            PendingIntent.FLAG_IMMUTABLE)).build();
    }

    @Override public IBinder onBind(Intent intent) { return null; }

    @Override public void onDestroy() {
        active = null;
        main.removeCallbacksAndMessages(null);
        unregisterReceiver(noisy);
        releaseAudioFocus();
        if (wakeLock.isHeld()) wakeLock.release();
        session.release();
        getSystemService(NotificationManager.class).cancel(NOTIFICATION);
        super.onDestroy();
    }
}
