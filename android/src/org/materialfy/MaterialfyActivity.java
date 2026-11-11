// Главная активность: QtActivity + edge-to-edge, разрешение на уведомления (Android 13+), «Назад» системы → QML.
package org.materialfy;

import android.Manifest;
import android.content.pm.PackageManager;
import android.graphics.Color;
import android.os.Build;
import android.os.Bundle;
import android.view.KeyEvent;
import android.view.Window;
import android.window.OnBackInvokedCallback;
import android.window.OnBackInvokedDispatcher;

import org.qtproject.qt.android.bindings.QtActivity;

public class MaterialfyActivity extends QtActivity
{
    private static final int REQUEST_NOTIFICATIONS = 4101;

    @Override
    public void onCreate(Bundle savedInstanceState)
    {
        super.onCreate(savedInstanceState);
        Window window = getWindow();
        // Интерфейс рисуется под системными панелями (отступы — SafeArea в QML)
        window.setStatusBarColor(Color.TRANSPARENT);
        window.setNavigationBarColor(Color.TRANSPARENT);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            window.setNavigationBarContrastEnforced(false);
            window.setStatusBarContrastEnforced(false);
        }
        MobileBridge.attach(this);
        // Без разрешения уведомление плеера (и кнопки на экране блокировки) не показывается
        if (Build.VERSION.SDK_INT >= 33
            && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            requestPermissions(new String[] { Manifest.permission.POST_NOTIFICATIONS }, REQUEST_NOTIFICATIONS);
        }
    }

    private Object backCallback;

    // Android 13+: «Назад» (кнопка и жест) приходит в OnBackInvokedCallback, KEYCODE_BACK система больше не шлёт.
    // Qt регистрирует свой колбэк в onResume, а срабатывает последний зарегистрированный — поэтому свой ставим
    // здесь же, после Qt, и снимаем в onPause. Решает QML (MobileMain.goBack): клавиатура, меню, плеер, стек.
    @Override
    protected void onResume()
    {
        super.onResume();
        if (Build.VERSION.SDK_INT >= 33) {
            OnBackInvokedCallback callback = MaterialfyActivity::sendBack;
            backCallback = callback;
            getOnBackInvokedDispatcher().registerOnBackInvokedCallback(OnBackInvokedDispatcher.PRIORITY_DEFAULT,
                                                                       callback);
        }
    }

    @Override
    protected void onPause()
    {
        unregisterBack();
        super.onPause();
    }

    private void unregisterBack()
    {
        if (Build.VERSION.SDK_INT >= 33 && backCallback != null) {
            getOnBackInvokedDispatcher().unregisterOnBackInvokedCallback((OnBackInvokedCallback) backCallback);
            backCallback = null;
        }
    }

    private static void sendBack()
    {
        try {
            MobileBridge.onMediaCommand("back", 0);
        } catch (UnsatisfiedLinkError e) {
            // нативная часть ещё не загружена — нажатие теряется, как и раньше
        }
    }

    // Android 12 и старше: клавиша «Назад» — тоже в QML, а не в обработчик QtActivity (тот закрыл бы окно)
    @Override
    public boolean dispatchKeyEvent(KeyEvent event)
    {
        if (event.getKeyCode() == KeyEvent.KEYCODE_BACK) {
            if (event.getAction() == KeyEvent.ACTION_UP && !event.isCanceled())
                sendBack();
            return true;
        }
        return super.dispatchKeyEvent(event);
    }

    @Override
    protected void onDestroy()
    {
        unregisterBack();
        // Активность закрыта, а процесс живёт (музыка играет в сервисе) — ссылку отпускаем
        MobileBridge.detach(this);
        super.onDestroy();
    }
}
