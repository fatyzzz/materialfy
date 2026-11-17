// Ожидание входа в браузере. Spotify возвращает браузер на http://127.0.0.1:<порт>/login, а слушает этот порт
// само приложение — в фоне, пока пользователь на сайте Spotify. Фоновое приложение Android может «заморозить»
// (cached apps freezer), и ответ браузера повис бы до возвращения в приложение. Короткий foreground-сервис
// с уведомлением «Ждём подтверждения входа» держит процесс живым, пока идёт вход (Android 14+: тип shortService,
// до трёх минут; раньше — dataSync).
package org.materialfy;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Context;
import android.content.Intent;
import android.content.pm.ServiceInfo;
import android.os.Build;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;

public class SignInService extends Service
{
    private static final String CHANNEL = "signin";
    private static final int NOTIFICATION_ID = 2;
    private static final Handler MAIN = new Handler(Looper.getMainLooper());

    // Вызывается из потока Qt
    static void setWaiting(Context context, boolean waiting)
    {
        final Context app = context.getApplicationContext();
        MAIN.post(() -> {
            Intent intent = new Intent(app, SignInService.class);
            if (!waiting) {
                app.stopService(intent);
                return;
            }
            try {
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O)
                    app.startForegroundService(intent);
                else
                    app.startService(intent);
            } catch (RuntimeException e) {
                // не дали запустить (приложение уже в фоне) — вход всё равно продолжится, когда пользователь вернётся
            }
        });
    }

    @Override
    public void onCreate()
    {
        super.onCreate();
        NotificationManager notifications = (NotificationManager) getSystemService(NOTIFICATION_SERVICE);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            NotificationChannel channel =
                new NotificationChannel(CHANNEL, getString(R.string.signin_channel), NotificationManager.IMPORTANCE_LOW);
            channel.setShowBadge(false);
            notifications.createNotificationChannel(channel);
        }
        Intent open = new Intent(this, MaterialfyActivity.class)
                          .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP | Intent.FLAG_ACTIVITY_REORDER_TO_FRONT);
        Notification.Builder b = Build.VERSION.SDK_INT >= Build.VERSION_CODES.O
                                     ? new Notification.Builder(this, CHANNEL)
                                     : new Notification.Builder(this);
        Notification notification = b.setSmallIcon(R.drawable.ic_notification)
                                        .setContentTitle(getString(R.string.signin_title))
                                        .setContentText(getString(R.string.signin_text))
                                        .setOngoing(true)
                                        .setShowWhen(false)
                                        .setContentIntent(PendingIntent.getActivity(this, 1, open,
                                            PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT))
                                        .build();
        try {
            if (Build.VERSION.SDK_INT >= 34)
                startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_SHORT_SERVICE);
            else if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q)
                startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC);
            else
                startForeground(NOTIFICATION_ID, notification);
        } catch (RuntimeException e) {
            stopSelf();
        }
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId)
    {
        return START_NOT_STICKY;
    }

    // Android 14+: время shortService вышло — вход затянулся, дальше без сервиса
    @Override
    public void onTimeout(int startId)
    {
        stopSelf();
    }

    @Override
    public void onDestroy()
    {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N)
            stopForeground(STOP_FOREGROUND_REMOVE);
        else
            stopForeground(true);
        super.onDestroy();
    }

    @Override
    public IBinder onBind(Intent intent) { return null; }
}
