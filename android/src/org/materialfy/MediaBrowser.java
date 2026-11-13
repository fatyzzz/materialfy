// Медиабраузер: отдаёт системе медиасессию приложения (PlayerSession). Каталога нет — список пуст. По нему
// системные плееры (Origin Player vivo, карточка медиа Android) находят приложение и подключаются к сессии:
// название, обложку, позицию и кнопки они берут из неё. Подключение принимается всегда — сессия создаётся
// при первом запросе, а трек в ней появится, как только приложение начнёт играть.
package org.materialfy;

import android.media.browse.MediaBrowser.MediaItem;
import android.os.Bundle;
import android.service.media.MediaBrowserService;

import java.util.ArrayList;
import java.util.List;

public class MediaBrowser extends MediaBrowserService
{
    private static final String ROOT = "materialfy";

    @Override
    public void onCreate()
    {
        super.onCreate();
        setSessionToken(PlayerSession.get(this).getSessionToken());
    }

    @Override
    public BrowserRoot onGetRoot(String clientPackageName, int clientUid, Bundle rootHints)
    {
        return new BrowserRoot(ROOT, null);
    }

    @Override
    public void onLoadChildren(String parentId, Result<List<MediaItem>> result)
    {
        result.sendResult(new ArrayList<>());
    }
}
