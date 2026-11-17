// Фоновое воспроизведение: foreground-сервис с MediaSession и уведомлением плеера (кнопки, обложка, экран
// блокировки, гарнитура), аудиофокус, «звук стал шумным» (выдернули наушники), блокировки сна и Wi-Fi.
// Состояние приходит из QML (MobileShell.updateMedia → MobileBridge.updateMedia → publish).
package org.materialfy;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.content.pm.ServiceInfo;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.media.AudioAttributes;
import android.media.AudioFocusRequest;
import android.media.AudioManager;
import android.media.MediaMetadata;
import android.media.session.MediaSession;
import android.media.session.PlaybackState;
import android.net.wifi.WifiManager;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;
import android.os.PowerManager;
import android.os.SystemClock;

import java.io.InputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

public class PlaybackService extends Service
{
    static final class State
    {
        String title = "";
        String artist = "";
        String album = "";
        String coverUrl = "";
        long durationMs;
        long positionMs;
        boolean playing;
        boolean clockRunning;
        boolean hasTrack;
        boolean canPrevious;
        boolean canNext;
        boolean local = true;
        boolean liked;          // трек в любимых — кнопка-сердце в карточке плеера системы
        // когда пришло (позиция между обновлениями — по времени)
        long at = SystemClock.elapsedRealtime();

        long positionNow()
        {
            long pos = positionMs + (clockRunning ? SystemClock.elapsedRealtime() - at : 0);
            return durationMs > 0 ? Math.min(pos, durationMs) : pos;
        }
    }

    private static final String CHANNEL = "playback";
    private static final int NOTIFICATION_ID = 1;
    private static final String ACTION_COMMAND = "org.materialfy.COMMAND";
    private static final String EXTRA_COMMAND = "command";

    private static final Handler MAIN = new Handler(Looper.getMainLooper());
    private static State latest = new State();
    private static PlaybackService running;
    private static boolean starting;

    private MediaSession session;
    private NotificationManager notifications;
    private AudioManager audio;
    private AudioFocusRequest focusRequest;
    private boolean hasFocus;
    private boolean resumeOnFocusGain;
    private boolean foreground;
    private boolean noisyRegistered;
    private PowerManager.WakeLock wakeLock;
    private WifiManager.WifiLock wifiLock;

    private final ExecutorService loader = Executors.newSingleThreadExecutor();
    private final IslandNotification island = new IslandNotification();
    // прогресс в острове системы — раз в 10 с, пока играет
    private final Runnable islandTick = new Runnable() {
        @Override
        public void run()
        {
            updateIsland();
            if (latest.playing && latest.hasTrack)
                MAIN.postDelayed(this, 10_000L);
        }
    };
    private String coverUrl = "";
    private Bitmap cover;

    private final BroadcastReceiver noisy = new BroadcastReceiver() {
        @Override
        public void onReceive(Context context, Intent intent)
        {
            if (AudioManager.ACTION_AUDIO_BECOMING_NOISY.equals(intent.getAction()) && latest.playing && latest.local)
                MobileBridge.send("pause", 0);
        }
    };

    private final AudioManager.OnAudioFocusChangeListener focusListener = change -> {
        switch (change) {
        case AudioManager.AUDIOFOCUS_GAIN:
            hasFocus = true;
            if (resumeOnFocusGain) {
                resumeOnFocusGain = false;
                MobileBridge.send("play", 0);
            }
            break;
        case AudioManager.AUDIOFOCUS_LOSS_TRANSIENT:
            // звонок, навигатор: пауза и продолжение, когда фокус вернут
            if (latest.playing && latest.local) {
                resumeOnFocusGain = true;
                MobileBridge.send("pause", 0);
            }
            break;
        case AudioManager.AUDIOFOCUS_LOSS:
            hasFocus = false;
            resumeOnFocusGain = false;
            if (latest.playing && latest.local)
                MobileBridge.send("pause", 0);
            break;
        default:
            break;
        }
    };

    // ---- из MobileBridge (любой поток) ----

    // Сердце в карточке плеера: трек сохранён / нет (Library.isSaved). Состояние живёт до следующего updateMedia
    static void setLiked(boolean liked)
    {
        MAIN.post(() -> {
            if (latest.liked == liked)
                return;
            latest.liked = liked;
            if (running != null)
                running.apply();
        });
    }

    // Настройки → «Плеер в острове системы»: применить сразу
    static void refreshIsland()
    {
        MAIN.post(() -> {
            if (running != null)
                running.updateIsland();
        });
    }

    static void publish(Context context, State state)
    {
        MAIN.post(() -> {
            state.liked = latest.liked;   // сердце приходит отдельно (setLiked)
            latest = state;
            if (running != null) {
                running.apply();
                return;
            }
            if (!state.hasTrack || !state.playing || starting)
                return;
            starting = true;
            try {
                Intent intent = new Intent(context, PlaybackService.class);
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O)
                    context.startForegroundService(intent);
                else
                    context.startService(intent);
            } catch (RuntimeException e) {
                // Android 12+: из фона foreground-сервис не запустить — попробуем при следующем обновлении
                starting = false;
            }
        });
    }

    // ---- жизненный цикл ----

    @Override
    public void onCreate()
    {
        super.onCreate();
        running = this;
        starting = false;
        notifications = (NotificationManager) getSystemService(NOTIFICATION_SERVICE);
        audio = (AudioManager) getSystemService(AUDIO_SERVICE);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            NotificationChannel channel =
                new NotificationChannel(CHANNEL, getString(R.string.playback_channel), NotificationManager.IMPORTANCE_LOW);
            channel.setShowBadge(false);
            channel.setLockscreenVisibility(Notification.VISIBILITY_PUBLIC);
            notifications.createNotificationChannel(channel);
        }
        // общая сессия процесса: к ней же подключаются плееры системы через MediaBrowser
        session = PlayerSession.get(this);
        session.setActive(true);

        PowerManager power = (PowerManager) getSystemService(POWER_SERVICE);
        wakeLock = power.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "materialfy:playback");
        wakeLock.setReferenceCounted(false);
        WifiManager wifi = (WifiManager) getApplicationContext().getSystemService(WIFI_SERVICE);
        if (wifi != null) {
            int mode = Build.VERSION.SDK_INT >= 29 ? WifiManager.WIFI_MODE_FULL_HIGH_PERF : WifiManager.WIFI_MODE_FULL;
            wifiLock = wifi.createWifiLock(mode, "materialfy:playback");
            wifiLock.setReferenceCounted(false);
        }
        // startForegroundService требует startForeground в течение нескольких секунд — сразу
        goForeground(buildNotification());
        apply();
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId)
    {
        if (intent != null && ACTION_COMMAND.equals(intent.getAction())) {
            String command = intent.getStringExtra(EXTRA_COMMAND);
            if (command != null)
                MobileBridge.send(command, 0);
        }
        return START_NOT_STICKY;
    }

    @Override
    public void onTaskRemoved(Intent rootIntent)
    {
        // Приложение смахнули из недавних: процесс Qt завершается вместе с активностью — уведомление не оставляем
        shutdown();
        super.onTaskRemoved(rootIntent);
    }

    @Override
    public void onDestroy()
    {
        shutdown();
        loader.shutdownNow();
        super.onDestroy();
    }

    @Override
    public IBinder onBind(Intent intent) { return null; }

    private void shutdown()
    {
        if (running == this)
            running = null;
        MAIN.removeCallbacks(islandTick);
        if (notifications != null)
            island.clear(notifications);
        abandonFocus();
        setNoisy(false);
        releaseLocks();
        // сессию не освобождаем — она общая с MediaBrowser; плеерам системы — «остановлено»
        if (session != null) {
            session.setPlaybackState(new PlaybackState.Builder()
                                         .setState(PlaybackState.STATE_STOPPED, 0, 0f, SystemClock.elapsedRealtime())
                                         .build());
            session = null;
        }
        stopForegroundCompat(true);
        if (notifications != null)
            notifications.cancel(NOTIFICATION_ID);
    }

    // ---- состояние → сессия, уведомление, фокус ----

    private void apply()
    {
        State s = latest;
        if (session == null)
            return;
        if (!s.hasTrack) {
            stopSelf();
            shutdown();
            return;
        }
        if (!s.coverUrl.equals(coverUrl)) {
            coverUrl = s.coverUrl;
            cover = null;
            loadCover(coverUrl);
        }
        MediaMetadata.Builder meta = new MediaMetadata.Builder()
            .putString(MediaMetadata.METADATA_KEY_TITLE, s.title)
            .putString(MediaMetadata.METADATA_KEY_ARTIST, s.artist)
            .putString(MediaMetadata.METADATA_KEY_ALBUM, s.album)
            .putLong(MediaMetadata.METADATA_KEY_DURATION, s.durationMs);
        if (cover != null)
            meta.putBitmap(MediaMetadata.METADATA_KEY_ALBUM_ART, cover);
        session.setMetadata(meta.build());

        long actions = PlaybackState.ACTION_PLAY | PlaybackState.ACTION_PAUSE | PlaybackState.ACTION_PLAY_PAUSE
                     | PlaybackState.ACTION_SEEK_TO | PlaybackState.ACTION_STOP;
        if (s.canNext)
            actions |= PlaybackState.ACTION_SKIP_TO_NEXT;
        if (s.canPrevious)
            actions |= PlaybackState.ACTION_SKIP_TO_PREVIOUS;
        PlaybackState.Builder state = new PlaybackState.Builder();
        if (s.hasTrack)
            state.addCustomAction(new PlaybackState.CustomAction.Builder(
                                      PlayerSession.ACTION_LIKE, getString(s.liked ? R.string.unlike : R.string.like),
                                      s.liked ? R.drawable.ic_liked : R.drawable.ic_like).build());
        session.setPlaybackState(state
                                     .setActions(actions)
                                     .setState(s.playing ? PlaybackState.STATE_PLAYING : PlaybackState.STATE_PAUSED,
                                               s.positionMs, s.clockRunning ? 1f : 0f, SystemClock.elapsedRealtime())
                                     .build());

        if (s.playing && s.local) {
            requestFocus();
            setNoisy(true);
            acquireLocks();
        } else {
            setNoisy(false);
            releaseLocks();
        }

        MAIN.removeCallbacks(islandTick);
        updateIsland();
        if (s.playing)
            MAIN.postDelayed(islandTick, 10_000L);

        Notification notification = buildNotification();
        if (s.playing) {
            goForeground(notification);
        } else {
            // на паузе уведомление можно смахнуть, а система может остановить сервис
            stopForegroundCompat(false);
            notifications.notify(NOTIFICATION_ID, notification);
        }
    }

    private void updateIsland()
    {
        State s = latest;
        State now = new State();
        now.title = s.title;
        now.artist = s.artist;
        now.album = s.album;
        now.durationMs = s.durationMs;
        now.positionMs = s.positionNow();
        now.playing = s.playing;
        now.clockRunning = s.clockRunning;
        now.hasTrack = s.hasTrack;
        PendingIntent open = session != null ? session.getController().getSessionActivity() : null;
        island.update(this, now, cover, open, action(android.R.drawable.ic_media_pause, R.string.pause, "pause"),
                      s.canNext ? action(android.R.drawable.ic_media_next, R.string.next, "next") : null);
    }

    private Notification buildNotification()
    {
        State s = latest;
        Notification.Builder b = Build.VERSION.SDK_INT >= Build.VERSION_CODES.O
                                     ? new Notification.Builder(this, CHANNEL)
                                     : new Notification.Builder(this);
        b.setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(s.title)
            .setContentText(s.artist)
            .setSubText(s.album)
            .setShowWhen(false)
            .setOnlyAlertOnce(true)
            .setVisibility(Notification.VISIBILITY_PUBLIC)
            .setCategory(Notification.CATEGORY_TRANSPORT)
            .setOngoing(s.playing)
            .setContentIntent(session != null ? session.getController().getSessionActivity() : null);
        if (cover != null)
            b.setLargeIcon(cover);
        b.addAction(action(android.R.drawable.ic_media_previous, R.string.previous, "previous"));
        b.addAction(s.playing ? action(android.R.drawable.ic_media_pause, R.string.pause, "pause")
                              : action(android.R.drawable.ic_media_play, R.string.play, "play"));
        b.addAction(action(android.R.drawable.ic_media_next, R.string.next, "next"));
        Notification.MediaStyle style = new Notification.MediaStyle().setShowActionsInCompactView(0, 1, 2);
        if (session != null)
            style.setMediaSession(session.getSessionToken());
        b.setStyle(style);
        return b.build();
    }

    private Notification.Action action(int icon, int title, String command)
    {
        Intent intent = new Intent(this, PlaybackService.class).setAction(ACTION_COMMAND).putExtra(EXTRA_COMMAND, command);
        PendingIntent pending = PendingIntent.getService(this, command.hashCode(), intent,
                                                         PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
        return new Notification.Action.Builder(android.graphics.drawable.Icon.createWithResource(this, icon),
                                               getString(title), pending).build();
    }

    private void goForeground(Notification notification)
    {
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q)
                startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK);
            else
                startForeground(NOTIFICATION_ID, notification);
            foreground = true;
        } catch (RuntimeException e) {
            notifications.notify(NOTIFICATION_ID, notification);
        }
    }

    private void stopForegroundCompat(boolean remove)
    {
        if (!foreground)
            return;
        foreground = false;
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N)
            stopForeground(remove ? STOP_FOREGROUND_REMOVE : STOP_FOREGROUND_DETACH);
        else
            stopForeground(remove);
    }

    private void requestFocus()
    {
        if (hasFocus)
            return;
        int result;
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            if (focusRequest == null) {
                AudioAttributes attributes = new AudioAttributes.Builder()
                                                 .setUsage(AudioAttributes.USAGE_MEDIA)
                                                 .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                                                 .build();
                focusRequest = new AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
                                   .setAudioAttributes(attributes)
                                   .setWillPauseWhenDucked(false)
                                   .setOnAudioFocusChangeListener(focusListener, MAIN)
                                   .build();
            }
            result = audio.requestAudioFocus(focusRequest);
        } else {
            result = audio.requestAudioFocus(focusListener, AudioManager.STREAM_MUSIC, AudioManager.AUDIOFOCUS_GAIN);
        }
        hasFocus = result == AudioManager.AUDIOFOCUS_REQUEST_GRANTED;
    }

    private void abandonFocus()
    {
        if (!hasFocus || audio == null)
            return;
        hasFocus = false;
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O && focusRequest != null)
            audio.abandonAudioFocusRequest(focusRequest);
        else
            audio.abandonAudioFocus(focusListener);
    }

    private void setNoisy(boolean on)
    {
        if (on == noisyRegistered)
            return;
        noisyRegistered = on;
        if (on) {
            IntentFilter filter = new IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY);
            if (Build.VERSION.SDK_INT >= 33)
                registerReceiver(noisy, filter, Context.RECEIVER_NOT_EXPORTED);
            else
                registerReceiver(noisy, filter);
        } else {
            unregisterReceiver(noisy);
        }
    }

    private void acquireLocks()
    {
        if (wakeLock != null && !wakeLock.isHeld())
            wakeLock.acquire();
        if (wifiLock != null && !wifiLock.isHeld())
            wifiLock.acquire();
    }

    private void releaseLocks()
    {
        if (wakeLock != null && wakeLock.isHeld())
            wakeLock.release();
        if (wifiLock != null && wifiLock.isHeld())
            wifiLock.release();
    }

    private void loadCover(String url)
    {
        if (url == null || url.isEmpty())
            return;
        loader.execute(() -> {
            Bitmap bitmap = null;
            try {
                if (url.startsWith("file:") || url.startsWith("/")) {
                    bitmap = BitmapFactory.decodeFile(url.startsWith("file:") ? new URL(url).getPath() : url);
                } else {
                    HttpURLConnection connection = (HttpURLConnection) new URL(url).openConnection();
                    connection.setConnectTimeout(10000);
                    connection.setReadTimeout(15000);
                    try (InputStream in = connection.getInputStream()) {
                        bitmap = BitmapFactory.decodeStream(in);
                    } finally {
                        connection.disconnect();
                    }
                }
            } catch (Exception e) {
                bitmap = null;
            }
            final Bitmap result = bitmap;
            MAIN.post(() -> {
                if (!url.equals(coverUrl) || result == null)
                    return;
                cover = result;
                apply();
            });
        });
    }
}
