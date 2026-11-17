// Обновление из приложения: проверенный APK (sf_services::update, SHA-256 из релиза) уходит в PackageInstaller.
// Система показывает своё окно подтверждения; после установки приложение перезапускается новой версией.
// Без разрешения «Установка неизвестных приложений» открываются его настройки, а установка возвращает ошибку
// с подсказкой — после разрешения кнопку «Установить» нажимают ещё раз.
package org.materialfy;

import android.app.PendingIntent;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageInstaller;
import android.net.Uri;
import android.os.Build;
import android.provider.Settings;

import java.io.File;
import java.io.FileInputStream;
import java.io.InputStream;
import java.io.OutputStream;

public final class ApkInstaller
{
    private static final String ACTION_RESULT = "org.materialfy.INSTALL_RESULT";

    private ApkInstaller() {}

    // Вызывается из потока Rust (через JNI). Пустая строка — окно установки показано; иначе — текст ошибки.
    public static String install(Context context, String path)
    {
        Context app = context.getApplicationContext();
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O && !app.getPackageManager().canRequestPackageInstalls()) {
            Intent settings = new Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
                                         Uri.parse("package:" + app.getPackageName()))
                                  .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            try {
                app.startActivity(settings);
            } catch (RuntimeException e) {
                // настроек нет (необычная прошивка) — подсказка ниже всё равно объяснит, что делать
            }
            return "permission";
        }
        File file = new File(path);
        PackageInstaller installer = app.getPackageManager().getPackageInstaller();
        PackageInstaller.SessionParams params =
            new PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL);
        params.setAppPackageName(app.getPackageName());
        params.setSize(file.length());
        try {
            int id = installer.createSession(params);
            try (PackageInstaller.Session session = installer.openSession(id)) {
                try (InputStream in = new FileInputStream(file);
                     OutputStream out = session.openWrite("materialfy.apk", 0, file.length())) {
                    byte[] buffer = new byte[1 << 16];
                    int n;
                    while ((n = in.read(buffer)) > 0)
                        out.write(buffer, 0, n);
                    session.fsync(out);
                }
                Intent result = new Intent(app, InstallResultReceiver.class).setAction(ACTION_RESULT);
                int flags = PendingIntent.FLAG_UPDATE_CURRENT
                          | (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S ? PendingIntent.FLAG_MUTABLE : 0);
                PendingIntent pending = PendingIntent.getBroadcast(app, id, result, flags);
                session.commit(pending.getIntentSender());
            }
            return "";
        } catch (Exception e) {
            return e.getMessage() != null ? e.getMessage() : e.toString();
        }
    }

    // Итог сессии: системе нужно подтверждение — показываем её окно; ошибка — в logcat (окно система уже показала)
    public static class InstallResultReceiver extends BroadcastReceiver
    {
        @Override
        public void onReceive(Context context, Intent intent)
        {
            int status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE);
            if (status == PackageInstaller.STATUS_PENDING_USER_ACTION) {
                Intent confirm = intent.getParcelableExtra(Intent.EXTRA_INTENT);
                if (confirm != null) {
                    confirm.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
                    try {
                        context.startActivity(confirm);
                    } catch (RuntimeException e) {
                        android.util.Log.w("materialfy", "install confirmation: " + e);
                    }
                }
            } else if (status != PackageInstaller.STATUS_SUCCESS) {
                android.util.Log.w("materialfy", "install failed: "
                    + intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE));
            }
        }
    }
}
