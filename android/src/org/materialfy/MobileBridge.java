// Мост Qt (cpp/mobile.cpp) ↔ Android: состояние плеера → PlaybackService, системные панели, «назад», вибрация.
// Команды из уведомления и MediaSession уходят в нативный код через onMediaCommand.
package org.materialfy;

import android.app.Activity;
import android.content.Context;
import android.content.Intent;
import android.net.wifi.WifiManager;
import android.os.Build;
import android.view.HapticFeedbackConstants;
import android.view.View;
import android.view.WindowInsetsController;

import java.lang.ref.WeakReference;

public final class MobileBridge
{
    private static WeakReference<Activity> activity = new WeakReference<>(null);
    // Spotify Connect: колонки в сети ищутся через mDNS, а без этой блокировки Android отбрасывает multicast
    private static WifiManager.MulticastLock multicast;

    private MobileBridge() {}

    // Регистрирует cpp/mobile.cpp (register_natives)
    public static native void onMediaCommand(String command, long arg);

    // Из Java-кода: команда в приложение. Нативная часть грузится вместе с активностью; если плеер системы
    // нажал кнопку раньше (процесс поднят ради MediaBrowser), команды некому принять — не падаем
    static void send(String command, long arg)
    {
        try {
            onMediaCommand(command, arg);
        } catch (UnsatisfiedLinkError e) {
            android.util.Log.w("materialfy", "media command before the app started: " + command);
        }
    }

    static void attach(Activity a)
    {
        activity = new WeakReference<>(a);
        if (multicast == null) {
            WifiManager wifi = (WifiManager) a.getApplicationContext().getSystemService(Context.WIFI_SERVICE);
            if (wifi != null) {
                multicast = wifi.createMulticastLock("materialfy:mdns");
                multicast.setReferenceCounted(false);
                multicast.acquire();
            }
        }
    }

    static void detach(Activity a)
    {
        if (activity.get() == a)
            activity = new WeakReference<>(null);
    }

    static Activity activityOf(Context context)
    {
        if (context instanceof Activity)
            return (Activity) context;
        return activity.get();
    }

    // Вызывается из потока Qt: сервис сам переносит работу в главный поток
    public static void updateMedia(Context context, String title, String artist, String album, String coverUrl,
                                   long durationMs, long positionMs, boolean playing, boolean hasTrack,
                                   boolean canPrevious, boolean canNext, boolean local)
    {
        PlaybackService.State state = new PlaybackService.State();
        state.title = title;
        state.artist = artist;
        state.album = album;
        state.coverUrl = coverUrl;
        state.durationMs = durationMs;
        state.positionMs = positionMs;
        state.playing = playing;
        state.hasTrack = hasTrack;
        state.canPrevious = canPrevious;
        state.canNext = canNext;
        state.local = local;
        PlaybackService.publish(context.getApplicationContext(), state);
    }

    // Сердце в карточке плеера системы: текущий трек в любимых или нет
    public static void setMediaLiked(boolean liked)
    {
        PlaybackService.setLiked(liked);
    }

    // lightBars=true — тёмные значки статус-бара и навигации (светлая тема интерфейса)
    public static void setSystemBars(Context context, boolean lightBars)
    {
        Activity a = activityOf(context);
        if (a == null)
            return;
        View decor = a.getWindow().getDecorView();
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            WindowInsetsController controller = a.getWindow().getInsetsController();
            if (controller == null)
                return;
            int mask = WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS
                     | WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS;
            controller.setSystemBarsAppearance(lightBars ? mask : 0, mask);
        } else {
            int flags = decor.getSystemUiVisibility();
            int light = View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR | View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR;
            decor.setSystemUiVisibility(lightBars ? (flags | light) : (flags & ~light));
        }
    }

    // Плеер в острове системы (IslandNotification)
    public static boolean islandSupported() { return IslandNotification.supported(); }

    public static boolean islandEnabled(Context context) { return IslandNotification.enabled(context); }

    public static void setIslandEnabled(Context context, boolean on)
    {
        IslandNotification.setEnabled(context, on);
        PlaybackService.refreshIsland();
    }

    // Вход в Spotify ждёт ответа браузера (см. SignInService)
    public static void setBrowserWait(Context context, boolean waiting)
    {
        SignInService.setWaiting(context, waiting);
    }

    public static void moveTaskToBack(Context context)
    {
        Activity a = activityOf(context);
        if (a != null)
            a.moveTaskToBack(true);
    }

    public static void bringToFront(Context context)
    {
        Intent intent = new Intent(context, MaterialfyActivity.class);
        intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_REORDER_TO_FRONT
                        | Intent.FLAG_ACTIVITY_SINGLE_TOP);
        try {
            context.startActivity(intent);
        } catch (RuntimeException e) {
            // фоновый запуск активности запрещён — пользователь вернётся сам
        }
    }

    // «Поделиться журналом» (настройки): текст в системное окно выбора — мессенджер, почта, «Копировать»
    public static void shareText(Context context, String title, String text)
    {
        Intent send = new Intent(Intent.ACTION_SEND)
                          .setType("text/plain")
                          .putExtra(Intent.EXTRA_SUBJECT, title)
                          .putExtra(Intent.EXTRA_TEXT, text);
        Intent chooser = Intent.createChooser(send, title).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
        try {
            context.startActivity(chooser);
        } catch (RuntimeException e) {
            android.util.Log.w("materialfy", "share: " + e);
        }
    }

    public static void haptic(Context context)
    {
        Activity a = activityOf(context);
        if (a != null)
            a.getWindow().getDecorView().performHapticFeedback(HapticFeedbackConstants.CONTEXT_CLICK);
    }
}
