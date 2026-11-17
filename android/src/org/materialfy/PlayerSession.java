// Медиасессия приложения — одна на процесс. Её создаёт первый, кому она нужна: PlaybackService (играет музыка)
// или MediaBrowser (к приложению подключился системный плеер — Origin Player vivo, карточка медиа Android).
// Раньше сессия жила только вместе с PlaybackService, и плеер системы, подключившийся раньше, получал отказ и
// пустую карточку.
package org.materialfy;

import android.app.PendingIntent;
import android.content.Context;
import android.content.Intent;
import android.media.session.MediaSession;
import android.os.Bundle;

final class PlayerSession
{
    // своя кнопка медиасессии: сердце (как у Spotify)
    static final String ACTION_LIKE = "org.materialfy.LIKE";

    private static MediaSession session;

    private PlayerSession() {}

    static synchronized MediaSession get(Context context)
    {
        if (session != null)
            return session;
        Context app = context.getApplicationContext();
        session = new MediaSession(app, "Materialfy");
        session.setCallback(new MediaSession.Callback() {
            @Override public void onPlay() { MobileBridge.send("play", 0); }
            @Override public void onPause() { MobileBridge.send("pause", 0); }
            @Override public void onSkipToNext() { MobileBridge.send("next", 0); }
            @Override public void onSkipToPrevious() { MobileBridge.send("previous", 0); }
            @Override public void onSeekTo(long pos) { MobileBridge.send("seek", pos); }
            @Override public void onStop() { MobileBridge.send("pause", 0); }
            @Override public void onCustomAction(String action, Bundle extras)
            {
                if (ACTION_LIKE.equals(action))
                    MobileBridge.send("like", 0);
            }
        });
        Intent open = new Intent(app, MaterialfyActivity.class)
                          .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP | Intent.FLAG_ACTIVITY_REORDER_TO_FRONT);
        session.setSessionActivity(
            PendingIntent.getActivity(app, 0, open, PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT));
        session.setActive(true);
        return session;
    }

    // уже созданная (или null) — без создания
    static synchronized MediaSession peek()
    {
        return session;
    }
}
