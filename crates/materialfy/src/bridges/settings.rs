// Settings: персистентные свойства (ключи mf_platform::SettingKey) с сигналами <имя>Changed. Значения кэшируются в мосте,
// запись — сразу в SettingsStore; изменения из других мест (Player.volume, AuthService::apply_proxy) приходят подпиской.
use std::pin::Pin;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QString, QStringList, QVariant};

use crate::qt::QVariantMap;
use mf_platform::{SettingKey, SettingValue, SettingsStore};

use super::{Delivery, forward_events, forward_watch};
use crate::context::ctx;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qproperty(QString, theme_mode, cxx_name = "themeMode", READ, WRITE = set_theme_mode, NOTIFY = theme_mode_changed)]
        #[qproperty(QString, scheme_variant, cxx_name = "schemeVariant", READ, WRITE = set_scheme_variant, NOTIFY = scheme_variant_changed)]
        #[qproperty(bool, accent_from_cover, cxx_name = "accentFromCover", READ, WRITE = set_accent_from_cover, NOTIFY = accent_from_cover_changed)]
        #[qproperty(QString, custom_seed, cxx_name = "customSeed", READ, WRITE = set_custom_seed, NOTIFY = custom_seed_changed)]
        #[qproperty(bool, window_buttons, cxx_name = "windowButtons", READ = window_buttons_value, WRITE = set_window_buttons, NOTIFY = window_buttons_changed)]
        #[qproperty(QString, track_transition, cxx_name = "trackTransition", READ, WRITE = set_track_transition, NOTIFY = track_transition_changed)]
        #[qproperty(QString, backdrop_mode, cxx_name = "backdropMode", READ, WRITE = set_backdrop_mode, NOTIFY = backdrop_mode_changed)]
        #[qproperty(f64, now_playing_blur, cxx_name = "nowPlayingBlur", READ, WRITE = set_now_playing_blur, NOTIFY = now_playing_blur_changed)]
        #[qproperty(f64, now_playing_drift, cxx_name = "nowPlayingDrift", READ, WRITE = set_now_playing_drift, NOTIFY = now_playing_drift_changed)]
        #[qproperty(bool, ambient_background, cxx_name = "ambientBackground", READ, WRITE = set_ambient_background, NOTIFY = ambient_background_changed)]
        #[qproperty(bool, cover_spin, cxx_name = "coverSpin", READ, WRITE = set_cover_spin, NOTIFY = cover_spin_changed)]
        #[qproperty(bool, cover_pulse, cxx_name = "coverPulse", READ, WRITE = set_cover_pulse, NOTIFY = cover_pulse_changed)]
        #[qproperty(bool, calm_when_inactive, cxx_name = "calmWhenInactive", READ, WRITE = set_calm_when_inactive, NOTIFY = calm_when_inactive_changed)]
        /// "normal" | "spicy" | "karaoke" — вид колонки текста
        #[qproperty(QString, lyrics_mode, cxx_name = "lyricsMode", READ, WRITE = set_lyrics_mode, NOTIFY = lyrics_mode_changed)]
        /// множитель размера шрифта текста
        #[qproperty(f64, lyrics_font_scale, cxx_name = "lyricsFontScale", READ, WRITE = set_lyrics_font_scale, NOTIFY = lyrics_font_scale_changed)]
        /// "none" | "soft" | "strong" — затемняющая подложка
        #[qproperty(QString, lyrics_scrim, cxx_name = "lyricsScrim", READ, WRITE = set_lyrics_scrim, NOTIFY = lyrics_scrim_changed)]
        #[qproperty(QString, lyrics_text_align, cxx_name = "lyricsTextAlign", READ, WRITE = set_lyrics_text_align, NOTIFY = lyrics_text_align_changed)]
        #[qproperty(QString, lyrics_fill_mode, cxx_name = "lyricsFillMode", READ, WRITE = set_lyrics_fill_mode, NOTIFY = lyrics_fill_mode_changed)]
        #[qproperty(QString, lyrics_visible_lines, cxx_name = "lyricsVisibleLines", READ, WRITE = set_lyrics_visible_lines, NOTIFY = lyrics_visible_lines_changed)]
        #[qproperty(f64, lyrics_highlight, cxx_name = "lyricsHighlight", READ, WRITE = set_lyrics_highlight, NOTIFY = lyrics_highlight_changed)]
        /// масштаб активной строки (0.0 — по стилю, либо 1.0…1.35)
        #[qproperty(f64, lyrics_active_scale, cxx_name = "lyricsActiveScale", READ, WRITE = set_lyrics_active_scale, NOTIFY = lyrics_active_scale_changed)]
        /// масштаб неактивных строк (0.0 — по стилю, либо 0.8…1.0)
        #[qproperty(f64, lyrics_idle_scale, cxx_name = "lyricsIdleScale", READ, WRITE = set_lyrics_idle_scale, NOTIFY = lyrics_idle_scale_changed)]
        /// амплитуда волны слов (0.0…2.0)
        #[qproperty(f64, lyrics_word_bounce, cxx_name = "lyricsWordBounce", READ, WRITE = set_lyrics_word_bounce, NOTIFY = lyrics_word_bounce_changed)]
        /// непрозрачность неактивных строк (0.15…0.75)
        #[qproperty(f64, lyrics_dim_opacity, cxx_name = "lyricsDimOpacity", READ, WRITE = set_lyrics_dim_opacity, NOTIFY = lyrics_dim_opacity_changed)]
        /// длительность скролла строки в мс (-1 — по стилю, либо 200…800)
        #[qproperty(i32, lyrics_scroll_ms, cxx_name = "lyricsScrollMs", READ, WRITE = set_lyrics_scroll_ms, NOTIFY = lyrics_scroll_ms_changed)]
        /// задержка каскада соседних строк в мс (-1 — по стилю, либо 0…80)
        #[qproperty(i32, lyrics_stagger_ms, cxx_name = "lyricsStaggerMs", READ, WRITE = set_lyrics_stagger_ms, NOTIFY = lyrics_stagger_ms_changed)]
        #[qproperty(f64, lyrics_blur_amount, cxx_name = "lyricsBlurAmount", READ, WRITE = set_lyrics_blur_amount, NOTIFY = lyrics_blur_amount_changed)]
        #[qproperty(f64, lyrics_line_spacing, cxx_name = "lyricsLineSpacing", READ, WRITE = set_lyrics_line_spacing, NOTIFY = lyrics_line_spacing_changed)]
        #[qproperty(bool, lyrics_animate, cxx_name = "lyricsAnimate", READ, WRITE = set_lyrics_animate, NOTIFY = lyrics_animate_changed)]
        #[qproperty(bool, lyrics_show_source, cxx_name = "lyricsShowSource", READ, WRITE = set_lyrics_show_source, NOTIFY = lyrics_show_source_changed)]
        #[qproperty(i32, lyrics_sync_ms, cxx_name = "lyricsSyncMs", READ, WRITE = set_lyrics_sync_ms, NOTIFY = lyrics_sync_ms_changed)]
        #[qproperty(QString, animations, READ, WRITE = set_animations, NOTIFY = animations_changed)]
        /// "auto" | "on" | "off" — режим для слабых ПК (Theme.lowPower)
        #[qproperty(QString, low_power_mode, cxx_name = "lowPowerMode", READ, WRITE = set_low_power_mode, NOTIFY = low_power_mode_changed)]
        /// "auto" | "integrated" | "discrete" — предпочтение графического процессора
        #[qproperty(QString, gpu_preference, cxx_name = "gpuPreference", READ, WRITE = set_gpu_preference, NOTIFY = gpu_preference_changed)]
        /// "system" | "en" | "ru" | "pt"; the applied language and the dictionary — Translations (Md3/I18n.qml)
        #[qproperty(QString, language, READ, WRITE = set_language, NOTIFY = language_changed)]
        /// 75 | 100 | 125 | 150 | 200 — QT_SCALE_FACTOR, after a restart
        #[qproperty(i32, ui_scale, cxx_name = "uiScale", READ, WRITE = set_ui_scale, NOTIFY = ui_scale_changed)]
        #[qproperty(bool, right_panel_open, cxx_name = "rightPanelOpen", READ, WRITE = set_right_panel_open, NOTIFY = right_panel_open_changed)]
        #[qproperty(QString, right_panel_tab, cxx_name = "rightPanelTab", READ, WRITE = set_right_panel_tab, NOTIFY = right_panel_tab_changed)]
        #[qproperty(bool, sidebar_collapsed, cxx_name = "sidebarCollapsed", READ, WRITE = set_sidebar_collapsed, NOTIFY = sidebar_collapsed_changed)]
        #[qproperty(i32, window_width, cxx_name = "windowWidth", READ, WRITE = set_window_width, NOTIFY = window_width_changed)]
        #[qproperty(i32, window_height, cxx_name = "windowHeight", READ, WRITE = set_window_height, NOTIFY = window_height_changed)]
        #[qproperty(i32, window_x, cxx_name = "windowX", READ, WRITE = set_window_x, NOTIFY = window_x_changed)]
        #[qproperty(i32, window_y, cxx_name = "windowY", READ, WRITE = set_window_y, NOTIFY = window_y_changed)]
        #[qproperty(bool, window_maximized, cxx_name = "windowMaximized", READ, WRITE = set_window_maximized, NOTIFY = window_maximized_changed)]
        #[qproperty(QString, library_view, cxx_name = "libraryView", READ, WRITE = set_library_view, NOTIFY = library_view_changed)]
        #[qproperty(QString, library_sort, cxx_name = "librarySort", READ, WRITE = set_library_sort, NOTIFY = library_sort_changed)]
        #[qproperty(QString, home_hidden, cxx_name = "homeHidden", READ, WRITE = set_home_hidden, NOTIFY = home_hidden_changed)]
        /// сцена «Радио» на главной
        #[qproperty(bool, home_radio_hidden, cxx_name = "homeRadioHidden", READ, WRITE = set_home_radio_hidden, NOTIFY = home_radio_hidden_changed)]
        /// в боковой панели вместо плейлистов — последние добавленные/обновлённые
        #[qproperty(bool, sidebar_library, cxx_name = "sidebarLibrary", READ, WRITE = set_sidebar_library, NOTIFY = sidebar_library_changed)]
        #[qproperty(i32, bitrate, READ, WRITE = set_bitrate, NOTIFY = bitrate_changed)]
        #[qproperty(bool, normalisation, READ, WRITE = set_normalisation, NOTIFY = normalisation_changed)]
        #[qproperty(bool, gapless, READ, WRITE = set_gapless, NOTIFY = gapless_changed)]
        #[qproperty(bool, autoplay, READ, WRITE = set_autoplay, NOTIFY = autoplay_changed)]
        #[qproperty(f64, volume, READ, WRITE = set_volume, NOTIFY = volume_changed)]
        #[qproperty(QString, audio_device, cxx_name = "audioDevice", READ, WRITE = set_audio_device, NOTIFY = audio_device_changed)]
        #[qproperty(bool, audio_cache, cxx_name = "audioCache", READ, WRITE = set_audio_cache, NOTIFY = audio_cache_changed)]
        #[qproperty(i32, audio_cache_mb, cxx_name = "audioCacheMb", READ, WRITE = set_audio_cache_mb, NOTIFY = audio_cache_mb_changed)]
        #[qproperty(bool, playback_authorized, cxx_name = "playbackAuthorized", READ, WRITE = set_playback_authorized, NOTIFY = playback_authorized_changed)]
        #[qproperty(f64, playback_speed, cxx_name = "playbackSpeed", READ, WRITE = set_playback_speed, NOTIFY = playback_speed_changed)]
        #[qproperty(i32, crossfade, READ, WRITE = set_crossfade, NOTIFY = crossfade_changed)]
        #[qproperty(bool, dj_voice, cxx_name = "djVoice", READ, WRITE = set_dj_voice, NOTIFY = dj_voice_changed)]
        #[qproperty(QString, device_name, cxx_name = "deviceName", READ, WRITE = set_device_name, NOTIFY = device_name_changed)]
        #[qproperty(QString, web_client_id, cxx_name = "webClientId", READ, WRITE = set_web_client_id, NOTIFY = web_client_id_changed)]
        /// окно «Web API ограничен» с инструкцией больше не показывать
        #[qproperty(bool, api_limit_tip_dismissed, cxx_name = "apiLimitTipDismissed", READ, WRITE = set_api_limit_tip_dismissed, NOTIFY = api_limit_tip_dismissed_changed)]
        #[qproperty(QString, proxy_mode, cxx_name = "proxyMode", READ, NOTIFY = proxy_mode_changed)]
        #[qproperty(QString, proxy_host, cxx_name = "proxyHost", READ, NOTIFY = proxy_host_changed)]
        #[qproperty(QString, proxy_port, cxx_name = "proxyPort", READ, NOTIFY = proxy_port_changed)]
        #[qproperty(QString, proxy_username, cxx_name = "proxyUsername", READ, NOTIFY = proxy_username_changed)]
        /// обложки мимо прокси (применяется сразу, без «Применить»)
        #[qproperty(bool, covers_bypass_proxy, cxx_name = "coversBypassProxy", READ, WRITE = set_covers_bypass_proxy, NOTIFY = covers_bypass_proxy_changed)]
        #[qproperty(bool, close_to_tray, cxx_name = "closeToTray", READ, WRITE = set_close_to_tray, NOTIFY = close_to_tray_changed)]
        #[qproperty(i32, wheel_step, cxx_name = "wheelStep", READ, WRITE = set_wheel_step, NOTIFY = wheel_step_changed)]
        /// "off" | "island" | "taskbar" — плеер вне главного окна (FloatingIsland / TaskbarPlayer)
        #[qproperty(QString, floating_player, cxx_name = "floatingPlayer", READ, WRITE = set_floating_player, NOTIFY = floating_player_changed)]
        /// "black" | "color" — вид островка и виджета
        #[qproperty(QString, floating_player_style, cxx_name = "floatingPlayerStyle", READ, WRITE = set_floating_player_style, NOTIFY = floating_player_style_changed)]
        /// место виджета в панели задач: "tray" | "left" | "appsLeft" | "appsRight" | доля "0.42"
        #[qproperty(QString, taskbar_widget_pos, cxx_name = "taskbarWidgetPos", READ, WRITE = set_taskbar_widget_pos, NOTIFY = taskbar_widget_pos_changed)]
        /// сколько раз показана подсказка с клавишами под островком (0..3)
        #[qproperty(i32, floating_key_hints, cxx_name = "floatingKeyHints", READ, WRITE = set_floating_key_hints, NOTIFY = floating_key_hints_changed)]
        /// островок прячется, когда на его мониторе приложение на весь экран
        #[qproperty(bool, floating_hide_fullscreen, cxx_name = "floatingHideFullscreen", READ, WRITE = set_floating_hide_fullscreen, NOTIFY = floating_hide_fullscreen_changed)]
        /// островок прячется, когда на его мониторе окно развёрнуто
        #[qproperty(QString, floating_edge, cxx_name = "floatingEdge", READ, WRITE = set_floating_edge, NOTIFY = floating_edge_changed)]
        #[qproperty(i32, floating_free_x, cxx_name = "floatingFreeX", READ, WRITE = set_floating_free_x, NOTIFY = floating_free_x_changed)]
        #[qproperty(i32, floating_free_y, cxx_name = "floatingFreeY", READ, WRITE = set_floating_free_y, NOTIFY = floating_free_y_changed)]
        #[qproperty(i32, floating_offset_x, cxx_name = "floatingOffsetX", READ, WRITE = set_floating_offset_x, NOTIFY = floating_offset_x_changed)]
        #[qproperty(bool, floating_hide_maximized, cxx_name = "floatingHideMaximized", READ, WRITE = set_floating_hide_maximized, NOTIFY = floating_hide_maximized_changed)]
        #[qproperty(QString, floating_island_screens, cxx_name = "floatingIslandScreens", READ, WRITE = set_floating_island_screens, NOTIFY = floating_island_screens_changed)]
        #[qproperty(QString, taskbar_screens, cxx_name = "taskbarScreens", READ, WRITE = set_taskbar_screens, NOTIFY = taskbar_screens_changed)]
        #[qproperty(QString, taskbar_positions, cxx_name = "taskbarPositions", READ, WRITE = set_taskbar_positions, NOTIFY = taskbar_positions_changed)]
        #[qproperty(QStringList, audio_devices, cxx_name = "audioDevices", READ, NOTIFY = audio_devices_changed)]
        #[qproperty(QMap_QString_QVariant, cache_usage, cxx_name = "cacheUsage", READ, NOTIFY = cache_usage_changed)]
        #[qproperty(bool, engine_restart_pending, cxx_name = "engineRestartPending", READ, NOTIFY = engine_restart_pending_changed)]
        #[qproperty(QString, proxy_state, cxx_name = "proxyState", READ, NOTIFY = proxy_state_changed)]
        #[qproperty(QString, proxy_error, cxx_name = "proxyError", READ, NOTIFY = proxy_state_changed)]
        /// «Web API ограничен…» (429/QUOTA_EXCEEDED); пусто — ограничений нет.
        #[qproperty(QString, web_api_limit, cxx_name = "webApiLimit", READ, NOTIFY = web_api_limit_changed)]
        #[qproperty(bool, system_reduced_motion, cxx_name = "systemReducedMotion", READ, CONSTANT)]
        /// тема панели задач для значка трея: "dark", "light" или "" (неизвестно — QML смотрит на тему Qt)
        #[qproperty(QString, system_taskbar_theme, cxx_name = "systemTaskbarTheme", READ, CONSTANT)]
        type Settings = super::SettingsRust;

        fn window_buttons_value(self: &Settings) -> bool;

        fn set_theme_mode(self: Pin<&mut Settings>, value: QString);
        fn set_scheme_variant(self: Pin<&mut Settings>, value: QString);
        fn set_accent_from_cover(self: Pin<&mut Settings>, value: bool);
        fn set_custom_seed(self: Pin<&mut Settings>, value: QString);
        fn set_window_buttons(self: Pin<&mut Settings>, value: bool);
        fn set_track_transition(self: Pin<&mut Settings>, value: QString);
        fn set_backdrop_mode(self: Pin<&mut Settings>, value: QString);
        fn set_now_playing_blur(self: Pin<&mut Settings>, value: f64);
        fn set_now_playing_drift(self: Pin<&mut Settings>, value: f64);
        fn set_ambient_background(self: Pin<&mut Settings>, value: bool);
        fn set_cover_spin(self: Pin<&mut Settings>, value: bool);
        fn set_cover_pulse(self: Pin<&mut Settings>, value: bool);
        fn set_calm_when_inactive(self: Pin<&mut Settings>, value: bool);
        fn set_lyrics_mode(self: Pin<&mut Settings>, value: QString);
        fn set_lyrics_font_scale(self: Pin<&mut Settings>, value: f64);
        fn set_lyrics_scrim(self: Pin<&mut Settings>, value: QString);
        fn set_lyrics_text_align(self: Pin<&mut Settings>, value: QString);
        fn set_lyrics_fill_mode(self: Pin<&mut Settings>, value: QString);
        fn set_lyrics_visible_lines(self: Pin<&mut Settings>, value: QString);
        fn set_lyrics_highlight(self: Pin<&mut Settings>, value: f64);
        fn set_lyrics_active_scale(self: Pin<&mut Settings>, value: f64);
        fn set_lyrics_idle_scale(self: Pin<&mut Settings>, value: f64);
        fn set_lyrics_word_bounce(self: Pin<&mut Settings>, value: f64);
        fn set_lyrics_dim_opacity(self: Pin<&mut Settings>, value: f64);
        fn set_lyrics_scroll_ms(self: Pin<&mut Settings>, value: i32);
        fn set_lyrics_stagger_ms(self: Pin<&mut Settings>, value: i32);
        fn set_lyrics_blur_amount(self: Pin<&mut Settings>, value: f64);
        fn set_lyrics_line_spacing(self: Pin<&mut Settings>, value: f64);
        fn set_lyrics_animate(self: Pin<&mut Settings>, value: bool);
        fn set_lyrics_show_source(self: Pin<&mut Settings>, value: bool);
        fn set_lyrics_sync_ms(self: Pin<&mut Settings>, value: i32);
        fn set_animations(self: Pin<&mut Settings>, value: QString);
        fn set_low_power_mode(self: Pin<&mut Settings>, value: QString);
        fn set_gpu_preference(self: Pin<&mut Settings>, value: QString);
        fn set_language(self: Pin<&mut Settings>, value: QString);
        fn set_ui_scale(self: Pin<&mut Settings>, value: i32);
        fn set_right_panel_open(self: Pin<&mut Settings>, value: bool);
        fn set_right_panel_tab(self: Pin<&mut Settings>, value: QString);
        fn set_sidebar_collapsed(self: Pin<&mut Settings>, value: bool);
        fn set_window_width(self: Pin<&mut Settings>, value: i32);
        fn set_window_height(self: Pin<&mut Settings>, value: i32);
        fn set_window_x(self: Pin<&mut Settings>, value: i32);
        fn set_window_y(self: Pin<&mut Settings>, value: i32);
        fn set_window_maximized(self: Pin<&mut Settings>, value: bool);
        fn set_library_view(self: Pin<&mut Settings>, value: QString);
        fn set_library_sort(self: Pin<&mut Settings>, value: QString);
        fn set_home_hidden(self: Pin<&mut Settings>, value: QString);
        fn set_home_radio_hidden(self: Pin<&mut Settings>, value: bool);
        fn set_sidebar_library(self: Pin<&mut Settings>, value: bool);
        fn set_bitrate(self: Pin<&mut Settings>, value: i32);
        fn set_normalisation(self: Pin<&mut Settings>, value: bool);
        fn set_gapless(self: Pin<&mut Settings>, value: bool);
        fn set_autoplay(self: Pin<&mut Settings>, value: bool);
        fn set_volume(self: Pin<&mut Settings>, value: f64);
        fn set_audio_device(self: Pin<&mut Settings>, value: QString);
        fn set_audio_cache(self: Pin<&mut Settings>, value: bool);
        fn set_audio_cache_mb(self: Pin<&mut Settings>, value: i32);
        fn set_playback_authorized(self: Pin<&mut Settings>, value: bool);
        fn set_playback_speed(self: Pin<&mut Settings>, value: f64);
        fn set_crossfade(self: Pin<&mut Settings>, value: i32);
        fn set_dj_voice(self: Pin<&mut Settings>, value: bool);
        fn set_device_name(self: Pin<&mut Settings>, value: QString);
        fn set_web_client_id(self: Pin<&mut Settings>, value: QString);
        fn set_api_limit_tip_dismissed(self: Pin<&mut Settings>, value: bool);
        fn set_covers_bypass_proxy(self: Pin<&mut Settings>, value: bool);
        fn set_close_to_tray(self: Pin<&mut Settings>, value: bool);
        fn set_wheel_step(self: Pin<&mut Settings>, value: i32);
        fn set_floating_player(self: Pin<&mut Settings>, value: QString);
        fn set_floating_player_style(self: Pin<&mut Settings>, value: QString);
        fn set_taskbar_widget_pos(self: Pin<&mut Settings>, value: QString);
        fn set_floating_key_hints(self: Pin<&mut Settings>, value: i32);
        fn set_floating_hide_fullscreen(self: Pin<&mut Settings>, value: bool);
        fn set_floating_hide_maximized(self: Pin<&mut Settings>, value: bool);
        fn set_floating_offset_x(self: Pin<&mut Settings>, value: i32);
        fn set_floating_edge(self: Pin<&mut Settings>, value: QString);
        fn set_floating_free_x(self: Pin<&mut Settings>, value: i32);
        fn set_floating_free_y(self: Pin<&mut Settings>, value: i32);
        fn set_floating_island_screens(self: Pin<&mut Settings>, value: QString);
        fn set_taskbar_screens(self: Pin<&mut Settings>, value: QString);
        fn set_taskbar_positions(self: Pin<&mut Settings>, value: QString);

        #[qsignal]
        #[cxx_name = "themeModeChanged"]
        fn theme_mode_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "schemeVariantChanged"]
        fn scheme_variant_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "accentFromCoverChanged"]
        fn accent_from_cover_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "customSeedChanged"]
        fn custom_seed_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "windowButtonsChanged"]
        fn window_buttons_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "trackTransitionChanged"]
        fn track_transition_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "backdropModeChanged"]
        fn backdrop_mode_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "nowPlayingBlurChanged"]
        fn now_playing_blur_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "nowPlayingDriftChanged"]
        fn now_playing_drift_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "ambientBackgroundChanged"]
        fn ambient_background_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "coverSpinChanged"]
        fn cover_spin_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "coverPulseChanged"]
        fn cover_pulse_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "calmWhenInactiveChanged"]
        fn calm_when_inactive_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "lyricsModeChanged"]
        fn lyrics_mode_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "lyricsFontScaleChanged"]
        fn lyrics_font_scale_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "lyricsScrimChanged"]
        fn lyrics_scrim_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "lyricsTextAlignChanged"]
        fn lyrics_text_align_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "lyricsFillModeChanged"]
        fn lyrics_fill_mode_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "lyricsVisibleLinesChanged"]
        fn lyrics_visible_lines_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "lyricsHighlightChanged"]
        fn lyrics_highlight_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "lyricsActiveScaleChanged"]
        fn lyrics_active_scale_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "lyricsIdleScaleChanged"]
        fn lyrics_idle_scale_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "lyricsWordBounceChanged"]
        fn lyrics_word_bounce_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "lyricsDimOpacityChanged"]
        fn lyrics_dim_opacity_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "lyricsScrollMsChanged"]
        fn lyrics_scroll_ms_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "lyricsStaggerMsChanged"]
        fn lyrics_stagger_ms_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "lyricsBlurAmountChanged"]
        fn lyrics_blur_amount_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "lyricsLineSpacingChanged"]
        fn lyrics_line_spacing_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "lyricsAnimateChanged"]
        fn lyrics_animate_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "lyricsShowSourceChanged"]
        fn lyrics_show_source_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "lyricsSyncMsChanged"]
        fn lyrics_sync_ms_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "animationsChanged"]
        fn animations_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "lowPowerModeChanged"]
        fn low_power_mode_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "gpuPreferenceChanged"]
        fn gpu_preference_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "languageChanged"]
        fn language_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "uiScaleChanged"]
        fn ui_scale_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "rightPanelOpenChanged"]
        fn right_panel_open_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "rightPanelTabChanged"]
        fn right_panel_tab_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "sidebarCollapsedChanged"]
        fn sidebar_collapsed_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "windowWidthChanged"]
        fn window_width_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "windowHeightChanged"]
        fn window_height_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "windowXChanged"]
        fn window_x_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "windowYChanged"]
        fn window_y_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "windowMaximizedChanged"]
        fn window_maximized_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "libraryViewChanged"]
        fn library_view_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "librarySortChanged"]
        fn library_sort_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "homeHiddenChanged"]
        fn home_hidden_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "homeRadioHiddenChanged"]
        fn home_radio_hidden_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "sidebarLibraryChanged"]
        fn sidebar_library_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "bitrateChanged"]
        fn bitrate_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "normalisationChanged"]
        fn normalisation_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "gaplessChanged"]
        fn gapless_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "autoplayChanged"]
        fn autoplay_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "volumeChanged"]
        fn volume_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "audioDeviceChanged"]
        fn audio_device_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "audioCacheChanged"]
        fn audio_cache_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "audioCacheMbChanged"]
        fn audio_cache_mb_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "playbackAuthorizedChanged"]
        fn playback_authorized_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "playbackSpeedChanged"]
        fn playback_speed_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "crossfadeChanged"]
        fn crossfade_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "djVoiceChanged"]
        fn dj_voice_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "deviceNameChanged"]
        fn device_name_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "webClientIdChanged"]
        fn web_client_id_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "apiLimitTipDismissedChanged"]
        fn api_limit_tip_dismissed_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "proxyModeChanged"]
        fn proxy_mode_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "proxyHostChanged"]
        fn proxy_host_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "proxyPortChanged"]
        fn proxy_port_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "proxyUsernameChanged"]
        fn proxy_username_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "coversBypassProxyChanged"]
        fn covers_bypass_proxy_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "closeToTrayChanged"]
        fn close_to_tray_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "wheelStepChanged"]
        fn wheel_step_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "floatingPlayerChanged"]
        fn floating_player_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "floatingPlayerStyleChanged"]
        fn floating_player_style_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "taskbarWidgetPosChanged"]
        fn taskbar_widget_pos_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "floatingKeyHintsChanged"]
        fn floating_key_hints_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "floatingHideFullscreenChanged"]
        fn floating_hide_fullscreen_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "floatingHideMaximizedChanged"]
        fn floating_hide_maximized_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "floatingOffsetXChanged"]
        fn floating_offset_x_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "floatingEdgeChanged"]
        fn floating_edge_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "floatingFreeXChanged"]
        fn floating_free_x_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "floatingFreeYChanged"]
        fn floating_free_y_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "floatingIslandScreensChanged"]
        fn floating_island_screens_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "taskbarScreensChanged"]
        fn taskbar_screens_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "taskbarPositionsChanged"]
        fn taskbar_positions_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "audioDevicesChanged"]
        fn audio_devices_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "cacheUsageChanged"]
        fn cache_usage_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "engineRestartPendingChanged"]
        fn engine_restart_pending_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "proxyStateChanged"]
        fn proxy_state_changed(self: Pin<&mut Settings>);
        #[qsignal]
        #[cxx_name = "webApiLimitChanged"]
        fn web_api_limit_changed(self: Pin<&mut Settings>);

        /// Перечитать устройства вывода (sf_player::output_devices в spawn_blocking).
        #[qinvokable]
        #[cxx_name = "refreshAudioDevices"]
        fn refresh_audio_devices(self: Pin<&mut Settings>);
        /// Пересчитать занятое место (mf_platform::dirs::cache_usage в spawn_blocking).
        #[qinvokable]
        #[cxx_name = "refreshCacheUsage"]
        fn refresh_cache_usage(self: Pin<&mut Settings>);
        /// Очистить кэш (вход не трогается), затем refreshCacheUsage.
        #[qinvokable]
        #[cxx_name = "clearCache"]
        fn clear_cache(self: Pin<&mut Settings>);
        /// Открыть каталог кэша в проводнике.
        #[qinvokable]
        #[cxx_name = "openCacheFolder"]
        fn open_cache_folder(self: &Settings);
        /// «Применить» в ApplyBar: перезапуск движка с новыми настройками (auth.restart_engine).
        #[qinvokable]
        #[cxx_name = "applyEngineSettings"]
        fn apply_engine_settings(self: Pin<&mut Settings>);
        /// Reopen the app: the interface scale is read only when Qt starts.
        #[qinvokable]
        #[cxx_name = "relaunch"]
        fn relaunch(self: &Settings);
        /// The scale this process started with: a relaunch is needed while uiScale differs from it.
        #[qinvokable]
        #[cxx_name = "uiScaleAtLaunch"]
        fn ui_scale_at_launch(self: &Settings) -> i32;
        /// Применить прокси (пароль — в хранилище секретов, не в настройки).
        #[qinvokable]
        #[cxx_name = "applyProxy"]
        fn apply_proxy(self: Pin<&mut Settings>, mode: &QString, host: &QString, port: &QString, username: &QString, password: &QString);
    }

    impl cxx_qt::Threading for Settings {}
    impl cxx_qt::Initialize for Settings {}
}

/// Тип свойства QML ↔ значение хранилища.
trait Field: Sized + PartialEq + Clone {
    fn load(store: &SettingsStore, key: SettingKey) -> Self;
    fn to_value(&self) -> SettingValue;
}

impl Field for QString {
    fn load(store: &SettingsStore, key: SettingKey) -> Self {
        if key == SettingKey::CustomSeed {
            return QString::from(super::theme::custom_seed(store).as_str());
        }
        QString::from(store.get_str(key).as_str())
    }
    fn to_value(&self) -> SettingValue {
        SettingValue::Str(self.to_string())
    }
}

impl Field for bool {
    fn load(store: &SettingsStore, key: SettingKey) -> Self {
        store.get_bool(key)
    }
    fn to_value(&self) -> SettingValue {
        SettingValue::Bool(*self)
    }
}

impl Field for i32 {
    fn load(store: &SettingsStore, key: SettingKey) -> Self {
        store.get_int(key).clamp(i32::MIN as i64, i32::MAX as i64) as i32
    }
    fn to_value(&self) -> SettingValue {
        SettingValue::Int(*self as i64)
    }
}

impl Field for f64 {
    fn load(store: &SettingsStore, key: SettingKey) -> Self {
        store.get_f64(key)
    }
    fn to_value(&self) -> SettingValue {
        SettingValue::Float(*self)
    }
}

fn store() -> &'static SettingsStore {
    &ctx().services.settings
}

// (поле, тип, ключ, сеттер, сигнал) — одна строка на свойство
// readonly — только чтение из QML (пишет сервис, например прокси после проверки в AuthService::apply_proxy)
macro_rules! settings {
    ($( $field:ident : $ty:ty = $key:ident, $setter:ident, $signal:ident; )*
     @readonly { $( $rfield:ident : $rty:ty = $rkey:ident, $rsignal:ident; )* }) => {
        pub struct SettingsRust {
            $( $field: $ty, )*
            $( $rfield: $rty, )*
            audio_devices: QStringList,
            cache_usage: QVariantMap,
            engine_restart_pending: bool,
            proxy_state: QString,
            proxy_error: QString,
            web_api_limit: QString,
            system_reduced_motion: bool,
            system_taskbar_theme: QString,
        }

        impl Default for SettingsRust {
            fn default() -> Self {
                let store = store();
                Self {
                    $( $field: <$ty as Field>::load(store, SettingKey::$key), )*
                    $( $rfield: <$rty as Field>::load(store, SettingKey::$rkey), )*
                    audio_devices: QStringList::default(),
                    cache_usage: QVariantMap::default(),
                    engine_restart_pending: false,
                    proxy_state: QString::default(),
                    proxy_error: QString::default(),
                    web_api_limit: QString::default(),
                    system_reduced_motion: mf_platform::system::reduced_motion(),
                    system_taskbar_theme: QString::from(match mf_platform::system::taskbar_dark() {
                        Some(true) => "dark",
                        Some(false) => "light",
                        None => "",
                    }),
                }
            }
        }

        impl qobject::Settings {
            $(
                fn $setter(mut self: Pin<&mut Self>, value: $ty) {
                    if self.$field == value {
                        return;
                    }
                    self.as_mut().rust_mut().$field = value.clone();
                    store().set(SettingKey::$key, value.to_value());
                    self.$signal();
                }
            )*

            /// Значение изменилось в хранилище (не через этот мост) — перечитать и оповестить.
            fn reload(mut self: Pin<&mut Self>, key: SettingKey) {
                match key {
                    $(
                        SettingKey::$key => {
                            let value = <$ty as Field>::load(store(), key);
                            if self.$field != value {
                                self.as_mut().rust_mut().$field = value;
                                self.$signal();
                            }
                        }
                    )*
                    $(
                        SettingKey::$rkey => {
                            let value = <$rty as Field>::load(store(), key);
                            if self.$rfield != value {
                                self.as_mut().rust_mut().$rfield = value;
                                self.$rsignal();
                            }
                        }
                    )*
                    // служебные ключи без свойства QML (Pins, HiddenTracks)
                    _ => {}
                }
            }
        }
    };
}

settings! {
    theme_mode: QString = ThemeMode, set_theme_mode, theme_mode_changed;
    scheme_variant: QString = SchemeVariant, set_scheme_variant, scheme_variant_changed;
    accent_from_cover: bool = AccentFromCover, set_accent_from_cover, accent_from_cover_changed;
    custom_seed: QString = CustomSeed, set_custom_seed, custom_seed_changed;
    window_buttons: bool = WindowButtons, set_window_buttons, window_buttons_changed;
    track_transition: QString = TrackTransition, set_track_transition, track_transition_changed;
    backdrop_mode: QString = BackdropMode, set_backdrop_mode, backdrop_mode_changed;
    now_playing_blur: f64 = NowPlayingBlur, set_now_playing_blur, now_playing_blur_changed;
    now_playing_drift: f64 = NowPlayingDrift, set_now_playing_drift, now_playing_drift_changed;
    ambient_background: bool = AmbientBackground, set_ambient_background, ambient_background_changed;
    cover_spin: bool = CoverSpin, set_cover_spin, cover_spin_changed;
    cover_pulse: bool = CoverPulse, set_cover_pulse, cover_pulse_changed;
    calm_when_inactive: bool = CalmWhenInactive, set_calm_when_inactive, calm_when_inactive_changed;
    lyrics_mode: QString = LyricsMode, set_lyrics_mode, lyrics_mode_changed;
    lyrics_font_scale: f64 = LyricsFontScale, set_lyrics_font_scale, lyrics_font_scale_changed;
    lyrics_scrim: QString = LyricsScrim, set_lyrics_scrim, lyrics_scrim_changed;
    lyrics_text_align: QString = LyricsTextAlign, set_lyrics_text_align, lyrics_text_align_changed;
    lyrics_fill_mode: QString = LyricsFillMode, set_lyrics_fill_mode, lyrics_fill_mode_changed;
    lyrics_visible_lines: QString = LyricsVisibleLines, set_lyrics_visible_lines, lyrics_visible_lines_changed;
    lyrics_highlight: f64 = LyricsHighlight, set_lyrics_highlight, lyrics_highlight_changed;
    lyrics_active_scale: f64 = LyricsActiveScale, set_lyrics_active_scale, lyrics_active_scale_changed;
    lyrics_idle_scale: f64 = LyricsIdleScale, set_lyrics_idle_scale, lyrics_idle_scale_changed;
    lyrics_word_bounce: f64 = LyricsWordBounce, set_lyrics_word_bounce, lyrics_word_bounce_changed;
    lyrics_dim_opacity: f64 = LyricsDimOpacity, set_lyrics_dim_opacity, lyrics_dim_opacity_changed;
    lyrics_scroll_ms: i32 = LyricsScrollMs, set_lyrics_scroll_ms, lyrics_scroll_ms_changed;
    lyrics_stagger_ms: i32 = LyricsStaggerMs, set_lyrics_stagger_ms, lyrics_stagger_ms_changed;
    lyrics_blur_amount: f64 = LyricsBlurAmount, set_lyrics_blur_amount, lyrics_blur_amount_changed;
    lyrics_line_spacing: f64 = LyricsLineSpacing, set_lyrics_line_spacing, lyrics_line_spacing_changed;
    lyrics_animate: bool = LyricsAnimate, set_lyrics_animate, lyrics_animate_changed;
    lyrics_show_source: bool = LyricsShowSource, set_lyrics_show_source, lyrics_show_source_changed;
    lyrics_sync_ms: i32 = LyricsSyncMs, set_lyrics_sync_ms, lyrics_sync_ms_changed;
    animations: QString = Animations, set_animations, animations_changed;
    low_power_mode: QString = LowPowerMode, set_low_power_mode, low_power_mode_changed;
    gpu_preference: QString = GpuPreference, set_gpu_preference, gpu_preference_changed;
    language: QString = Language, set_language, language_changed;
    ui_scale: i32 = UiScale, set_ui_scale, ui_scale_changed;
    right_panel_open: bool = RightPanelOpen, set_right_panel_open, right_panel_open_changed;
    right_panel_tab: QString = RightPanelTab, set_right_panel_tab, right_panel_tab_changed;
    sidebar_collapsed: bool = SidebarCollapsed, set_sidebar_collapsed, sidebar_collapsed_changed;
    window_width: i32 = WindowWidth, set_window_width, window_width_changed;
    window_height: i32 = WindowHeight, set_window_height, window_height_changed;
    window_x: i32 = WindowX, set_window_x, window_x_changed;
    window_y: i32 = WindowY, set_window_y, window_y_changed;
    window_maximized: bool = WindowMaximized, set_window_maximized, window_maximized_changed;
    library_view: QString = LibraryView, set_library_view, library_view_changed;
    library_sort: QString = LibrarySort, set_library_sort, library_sort_changed;
    home_hidden: QString = HomeHidden, set_home_hidden, home_hidden_changed;
    home_radio_hidden: bool = HomeRadioHidden, set_home_radio_hidden, home_radio_hidden_changed;
    sidebar_library: bool = SidebarLibrary, set_sidebar_library, sidebar_library_changed;
    bitrate: i32 = Bitrate, set_bitrate, bitrate_changed;
    normalisation: bool = Normalisation, set_normalisation, normalisation_changed;
    gapless: bool = Gapless, set_gapless, gapless_changed;
    autoplay: bool = Autoplay, set_autoplay, autoplay_changed;
    volume: f64 = Volume, set_volume, volume_changed;
    audio_device: QString = AudioDevice, set_audio_device, audio_device_changed;
    audio_cache: bool = AudioCache, set_audio_cache, audio_cache_changed;
    audio_cache_mb: i32 = AudioCacheMb, set_audio_cache_mb, audio_cache_mb_changed;
    playback_authorized: bool = PlaybackAuthorized, set_playback_authorized, playback_authorized_changed;
    playback_speed: f64 = PlaybackSpeed, set_playback_speed, playback_speed_changed;
    crossfade: i32 = Crossfade, set_crossfade, crossfade_changed;
    dj_voice: bool = DjVoice, set_dj_voice, dj_voice_changed;
    device_name: QString = DeviceName, set_device_name, device_name_changed;
    web_client_id: QString = WebClientId, set_web_client_id, web_client_id_changed;
    api_limit_tip_dismissed: bool = ApiLimitTipDismissed, set_api_limit_tip_dismissed, api_limit_tip_dismissed_changed;
    covers_bypass_proxy: bool = CoversBypassProxy, set_covers_bypass_proxy, covers_bypass_proxy_changed;
    close_to_tray: bool = CloseToTray, set_close_to_tray, close_to_tray_changed;
    wheel_step: i32 = WheelStep, set_wheel_step, wheel_step_changed;
    floating_player: QString = FloatingPlayer, set_floating_player, floating_player_changed;
    floating_player_style: QString = FloatingPlayerStyle, set_floating_player_style, floating_player_style_changed;
    taskbar_widget_pos: QString = TaskbarWidgetPos, set_taskbar_widget_pos, taskbar_widget_pos_changed;
    floating_key_hints: i32 = FloatingKeyHints, set_floating_key_hints, floating_key_hints_changed;
    floating_hide_fullscreen: bool = FloatingHideFullscreen, set_floating_hide_fullscreen, floating_hide_fullscreen_changed;
    floating_hide_maximized: bool = FloatingHideMaximized, set_floating_hide_maximized, floating_hide_maximized_changed;
    floating_offset_x: i32 = FloatingOffsetX, set_floating_offset_x, floating_offset_x_changed;
    floating_edge: QString = FloatingEdge, set_floating_edge, floating_edge_changed;
    floating_free_x: i32 = FloatingFreeX, set_floating_free_x, floating_free_x_changed;
    floating_free_y: i32 = FloatingFreeY, set_floating_free_y, floating_free_y_changed;
    floating_island_screens: QString = FloatingIslandScreens, set_floating_island_screens, floating_island_screens_changed;
    taskbar_screens: QString = TaskbarScreens, set_taskbar_screens, taskbar_screens_changed;
    taskbar_positions: QString = TaskbarPositions, set_taskbar_positions, taskbar_positions_changed;
    @readonly {
        proxy_mode: QString = ProxyMode, proxy_mode_changed;
        proxy_host: QString = ProxyHost, proxy_host_changed;
        proxy_port: QString = ProxyPort, proxy_port_changed;
        proxy_username: QString = ProxyUsername, proxy_username_changed;
    }
}

impl qobject::Settings {
    /// Кнопки окна приложение рисует на всех ОС; настройки больше нет (старое значение читается, но не влияет).
    fn window_buttons_value(&self) -> bool {
        true
    }

    fn refresh_audio_devices(self: Pin<&mut Self>) {
        let thread = self.qt_thread();
        ctx().rt.spawn_blocking(move || {
            let devices = sf_player::output_devices();
            let _ = thread.queue(move |settings| settings.set_audio_devices(devices));
        });
    }

    fn set_audio_devices(mut self: Pin<&mut Self>, devices: Vec<String>) {
        let list: QStringList = devices.iter().map(|d| QString::from(d.as_str())).collect();
        if self.audio_devices != list {
            self.as_mut().rust_mut().audio_devices = list;
            self.audio_devices_changed();
        }
    }

    fn refresh_cache_usage(self: Pin<&mut Self>) {
        let thread = self.qt_thread();
        ctx().rt.spawn_blocking(move || {
            let usage = mf_platform::dirs::cache_usage();
            let _ = thread.queue(move |settings| settings.set_cache_usage(usage));
        });
    }

    fn set_cache_usage(mut self: Pin<&mut Self>, usage: mf_platform::dirs::CacheUsage) {
        let map = cache_usage_map(usage);
        if self.cache_usage != map {
            self.as_mut().rust_mut().cache_usage = map;
            self.cache_usage_changed();
        }
    }

    fn clear_cache(self: Pin<&mut Self>) {
        let thread = self.qt_thread();
        ctx().rt.spawn_blocking(move || {
            // занятые файлы (аудио играющего трека) пропускаются — остальное уже удалено, пользователю не мешаем
            if let Err(error) = mf_platform::dirs::clear_cache() {
                tracing::info!("cache only partially cleared: {error}");
            }
            let usage = mf_platform::dirs::cache_usage();
            let _ = thread.queue(move |settings| settings.set_cache_usage(usage));
        });
    }

    fn open_cache_folder(&self) {
        let dir = mf_platform::dirs::cache_dir();
        if let Err(error) = std::fs::create_dir_all(&dir) {
            tracing::warn!("cache directory {}: {error}", dir.display());
        }
        if !crate::qt::open_local_folder(&dir) {
            tracing::warn!("could not open {}", dir.display());
        }
    }

    fn apply_engine_settings(self: Pin<&mut Self>) {
        ctx().services.auth.restart_engine();
    }

    /// Reopen the app after a setting that Qt reads only at startup (the interface scale).
    /// The AppImage is the executable; a plain build relaunches its own binary. The shell
    /// waits for this process to exit, so the new one finds the config and the MPRIS name free.
    fn ui_scale_at_launch(&self) -> i32 {
        // engine::apply_ui_scale sets the marker; without it QT_SCALE_FACTOR came from the user and the setting does nothing
        std::env::var("MATERIALFY_UI_SCALE").ok().and_then(|v| v.parse().ok()).unwrap_or(*self.ui_scale())
    }

    fn relaunch(&self) {
        if cfg!(target_os = "android") {
            return;
        }
        let exe = std::env::var_os("APPIMAGE")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::current_exe().ok());
        let Some(exe) = exe else { return };
        let pid = std::process::id();
        #[cfg(unix)]
        {
            let script =
                format!("while kill -0 {pid} 2>/dev/null; do sleep 0.1; done; exec \"$0\"");
            let _ = std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(script)
                .arg(&exe)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let path = exe.display().to_string().replace('\'', "''");
            let script = format!(
                "Wait-Process -Id {pid} -ErrorAction SilentlyContinue; Start-Process -FilePath '{path}'"
            );
            // like the updater: powershell from System32, not from PATH
            let powershell = std::env::var_os("SystemRoot")
                .map(|root| std::path::PathBuf::from(root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe"))
                .filter(|p| p.is_file())
                .unwrap_or_else(|| "powershell.exe".into());
            let _ = std::process::Command::new(powershell)
                .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script])
                .creation_flags(0x0000_0200 | 0x0800_0000) // CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW
                .spawn();
        }
        crate::qt::ffi::ffi::post_quit();
    }

    fn apply_proxy(
        self: Pin<&mut Self>,
        mode: &QString,
        host: &QString,
        port: &QString,
        username: &QString,
        password: &QString,
    ) {
        ctx().services.auth.apply_proxy(sf_services::ProxyInput {
            mode: mode.to_string(),
            host: host.to_string(),
            port: port.to_string(),
            username: username.to_string(),
            password: password.to_string(),
        });
    }

    /// engineRestartPending и proxyState/proxyError — из AuthSnapshot (флаг ведёт AuthService).
    fn on_auth(mut self: Pin<&mut Self>, snapshot: sf_services::AuthSnapshot) {
        if self.engine_restart_pending != snapshot.engine_restart_pending {
            self.as_mut().rust_mut().engine_restart_pending = snapshot.engine_restart_pending;
            self.as_mut().engine_restart_pending_changed();
        }
        let state = QString::from(snapshot.proxy_state.as_str());
        let error = QString::from(snapshot.proxy_error.as_str());
        if self.proxy_state != state || self.proxy_error != error {
            self.as_mut().rust_mut().proxy_state = state;
            self.as_mut().rust_mut().proxy_error = error;
            self.as_mut().proxy_state_changed();
        }
    }

    /// Охлаждение Web API после 429/квоты (sf_api::limits) — строка в «Аккаунте».
    fn on_web_api_limits(mut self: Pin<&mut Self>, limits: sf_api::WebApiLimits) {
        let text = QString::from(limits.user_text().as_str());
        if self.web_api_limit != text {
            self.as_mut().rust_mut().web_api_limit = text;
            self.as_mut().web_api_limit_changed();
        }
    }

    fn on_store(mut self: Pin<&mut Self>, delivery: Delivery<SettingKey>) {
        match delivery {
            Delivery::Event(key) => self.reload(key),
            Delivery::Lagged => {
                for key in SettingKey::ALL {
                    self.as_mut().reload(key);
                }
            }
        }
    }
}

/// {audio, covers, playlists, lyrics, total} в байтах (числа QML — double: размеры больше i32).
fn cache_usage_map(usage: mf_platform::dirs::CacheUsage) -> QVariantMap {
    let total = usage.total();
    let mut map = QVariantMap::default();
    for (key, bytes) in
        [("audio", usage.audio), ("covers", usage.covers), ("playlists", usage.playlists), ("lyrics", usage.lyrics), ("total", total)]
    {
        map.insert(QString::from(key), QVariant::from(&(bytes as f64)));
    }
    map
}

impl cxx_qt::Initialize for qobject::Settings {
    fn initialize(mut self: Pin<&mut Self>) {
        // нули до первого подсчёта: QML не видит undefined
        self.as_mut().rust_mut().cache_usage = cache_usage_map(Default::default());
        if !ctx().flags.screenshot {
            self.as_mut().refresh_audio_devices();
            self.as_mut().refresh_cache_usage();
        }
        forward_events(self.qt_thread(), store().subscribe(), Self::on_store);
        let auth = ctx().services.auth.watch();
        let current = auth.borrow().clone();
        self.as_mut().on_auth(current);
        forward_watch(self.qt_thread(), auth, Self::on_auth);
        let limits = ctx().services.auth.api().web_api_limits();
        let current = limits.borrow().clone();
        self.as_mut().on_web_api_limits(current);
        forward_watch(self.qt_thread(), limits, Self::on_web_api_limits);
    }
}
