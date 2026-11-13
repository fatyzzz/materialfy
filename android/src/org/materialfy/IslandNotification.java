// Плеер в «острове» системы: отдельное уведомление рядом с медиа-уведомлением (MediaStyle повышать нельзя).
//  - Android 16+: Live Update (запрос promoted ongoing + ProgressStyle). Его показывают чип статус-бара (Pixel),
//    Now Bar (Samsung One UI 8+), Super Island (HyperOS 3.1+), Fluid Cloud (ColorOS 16+) и другие оболочки,
//    которые выводят Live Updates.
//  - vivo OriginOS: «атомный остров» читает extras notification.superx.* (сцена MUSIC_PLAYBACK) — так делают
//    сторонние плееры (MeloX). Музыкальную карточку острова vivo открывает не всем приложениям: если прошивка
//    не пускает, остаётся обычное уведомление плеера.
// Включается в Настройки → Воспроизведение («Плеер в острове системы»), по умолчанию — да.
package org.materialfy;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.content.Context;
import android.content.Intent;
import android.graphics.Bitmap;
import android.graphics.drawable.Icon;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.os.SystemClock;
import android.service.notification.StatusBarNotification;

import java.util.Locale;

final class IslandNotification
{
    private static final String CHANNEL = "island";
    static final int NOTIFICATION_ID = 3;
    private static final String PREFS = "materialfy";
    private static final String KEY_ENABLED = "islandEnabled";
    // vivo отверг остров на этой прошивке и версии приложения (см. checkVivoVerdict)
    private static final String KEY_VIVO_REJECTED = "islandVivoRejected";
    // vivo: прогресс острова обновляется не чаще, чем раз в 10 с (так советуют плееры, которые это делают)
    private static final long MIN_INTERVAL_MS = 10_000L;

    private String lastKey = "";
    private boolean lastClockRunning;
    private long lastAt;

    static boolean isVivo()
    {
        String m = Build.MANUFACTURER.toLowerCase(Locale.ROOT);
        String b = Build.BRAND.toLowerCase(Locale.ROOT);
        return m.contains("vivo") || b.contains("vivo") || b.contains("iqoo");
    }

    // Есть куда показать: Live Updates (Android 16) или остров vivo
    static boolean supported()
    {
        return Build.VERSION.SDK_INT >= 36 || isVivo();
    }

    static boolean enabled(Context context)
    {
        return context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getBoolean(KEY_ENABLED, true);
    }

    static void setEnabled(Context context, boolean on)
    {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putBoolean(KEY_ENABLED, on).apply();
    }

    void update(Context context, PlaybackService.State s, Bitmap cover, PendingIntent open,
                Notification.Action playPause, Notification.Action next)
    {
        NotificationManager nm = (NotificationManager) context.getSystemService(Context.NOTIFICATION_SERVICE);
        if (!supported() || !enabled(context) || !s.hasTrack || !s.playing || vivoRejected(context)) {
            clear(nm);
            return;
        }
        String key = s.title + "\u0000" + s.artist;
        long now = SystemClock.elapsedRealtime();
        // тот же трек — не чаще раза в 10 с (позицию между обновлениями ведёт таймер чипа)
        if (key.equals(lastKey) && s.clockRunning == lastClockRunning && now - lastAt < MIN_INTERVAL_MS)
            return;
        boolean newTrack = !key.equals(lastKey);
        lastKey = key;
        lastClockRunning = s.clockRunning;
        lastAt = now;

        ensureChannel(context, nm);
        int progress = s.durationMs > 0 ? (int) Math.max(0, Math.min(1000, s.positionMs * 1000 / s.durationMs)) : 0;
        Notification.Builder b = new Notification.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(s.title)
            .setContentText(s.artist)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setLocalOnly(true)
            .setCategory(Notification.CATEGORY_TRANSPORT)
            .setVisibility(Notification.VISIBILITY_PUBLIC)
            .setContentIntent(open)
            // время в чипе острова идёт само: с начала трека
            .setWhen(System.currentTimeMillis() - Math.max(0, s.positionMs))
            .setShowWhen(s.clockRunning)
            .setUsesChronometer(s.clockRunning);
        if (cover != null)
            b.setLargeIcon(cover);
        if (playPause != null)
            b.addAction(playPause);
        if (next != null)
            b.addAction(next);

        Bundle extras = new Bundle();
        if (Build.VERSION.SDK_INT >= 36) {
            // Live Update: система сама решает, повысить ли (пользователь может запретить в настройках)
            extras.putBoolean("android.requestPromotedOngoing", true);
            b.setStyle(new Notification.ProgressStyle().setProgress(progress).setStyledByProgress(false)
                           .setProgressSegments(java.util.Collections.singletonList(
                               new Notification.ProgressStyle.Segment(1000))));
            b.setShortCriticalText(shortTitle(s.title));
        }
        if (isVivo()) {
            extras.putInt("notification.superx.operation", newTrack ? 0 : 1);
            extras.putBoolean("notification.superx.showNotify", true);
            extras.putInt("notification.superx.template", 2);
            extras.putString("notification.superx.scene", "MUSIC_PLAYBACK");
            if (open != null)
                extras.putParcelable("notification.superx.clickResp", open);
            Bundle base = new Bundle();
            base.putParcelable("notification.superx.baseInfos.icon",
                               cover != null ? Icon.createWithBitmap(cover)
                                             : Icon.createWithResource(context, R.drawable.ic_notification));
            base.putCharSequence("notification.superx.baseInfos.title", s.title);
            base.putCharSequence("notification.superx.baseInfos.content", s.artist);
            extras.putBundle("notification.superx.baseInfos", base);
            if (Build.VERSION.SDK_INT < 36)
                b.setProgress(1000, progress, false);
        }
        b.addExtras(extras);
        try {
            nm.notify(NOTIFICATION_ID, b.build());
        } catch (RuntimeException e) {
            // прошивка отвергла расширенное уведомление — остаётся обычное уведомление плеера
            return;
        }
        if (isVivo() && newTrack)
            new Handler(Looper.getMainLooper()).postDelayed(() -> checkVivoVerdict(context, nm), 1500);
    }

    // vivo (Funtouch / OriginOS) пускает в остров по superx только свои приложения (разрешение
    // ISLAND_NOTI_SUPPORT — подписью системы) и помечает чужое уведомление SUPERX_VALUE=false. Live Update vivo
    // тоже не выводит — карточка «острова» в шторке тогда лишь дублирует уведомление плеера: убрать её и больше
    // не ставить на этой прошивке и этой версии приложения
    private void checkVivoVerdict(Context context, NotificationManager nm)
    {
        try {
            for (StatusBarNotification sbn : nm.getActiveNotifications()) {
                if (sbn.getId() != NOTIFICATION_ID)
                    continue;
                Object verdict = sbn.getNotification().extras.get("SUPERX_VALUE");
                if (verdict != null && "false".equals(String.valueOf(verdict))) {
                    context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
                        .putString(KEY_VIVO_REJECTED, verdictKey(context)).apply();
                    clear(nm);
                }
                return;
            }
        } catch (RuntimeException e) {
            // нет ответа — оставить как есть
        }
    }

    private static boolean vivoRejected(Context context)
    {
        if (!isVivo())
            return false;
        String saved = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString(KEY_VIVO_REJECTED, "");
        return saved.equals(verdictKey(context));
    }

    // прошивка + версия приложения: после обновления любого из них — проверить заново
    private static String verdictKey(Context context)
    {
        long version = 0;
        try {
            version = context.getPackageManager().getPackageInfo(context.getPackageName(), 0).getLongVersionCode();
        } catch (Exception e) {
            // без версии — по одной прошивке
        }
        return Build.FINGERPRINT + "|" + version;
    }

    void clear(NotificationManager nm)
    {
        // и без своего lastKey: уведомление могло остаться от прежнего процесса приложения
        lastKey = "";
        lastAt = 0;
        nm.cancel(NOTIFICATION_ID);
    }

    private static String shortTitle(String title)
    {
        String t = title == null ? "" : title.trim();
        return t.length() > 12 ? t.substring(0, 11) + "…" : t;
    }

    private static void ensureChannel(Context context, NotificationManager nm)
    {
        if (nm.getNotificationChannel(CHANNEL) != null)
            return;
        // DEFAULT, но без звука и вибрации: каналы MIN/LOW Live Updates не повышают
        NotificationChannel channel = new NotificationChannel(CHANNEL, context.getString(R.string.island_channel),
                                                              NotificationManager.IMPORTANCE_DEFAULT);
        channel.setSound(null, null);
        channel.enableVibration(false);
        channel.setShowBadge(false);
        channel.setLockscreenVisibility(Notification.VISIBILITY_PUBLIC);
        nm.createNotificationChannel(channel);
    }
}
