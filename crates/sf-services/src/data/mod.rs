//! Внутренние модули «services-core» (кэш Liked Songs, кэш плейлистов, постраничные списки, хранилище истории…).
//! Новые файлы части 1 sf-services создавать только здесь; объявлять в этом mod.rs.

/// альбом и трек через GraphQL-шлюз (getAlbum, getTrack): прослушивания, лейбл, правообладатели, дата
pub mod album_overview;
/// исполнитель через GraphQL-шлюз (queryArtistOverview): подписчики, слушатели, похожие, биография
pub mod artist_overview;
/// коллекция через сессию: «Любимые треки», альбомы, подписки, сохранение и проверка
pub mod collection_session;
/// атомарная запись файлов
pub mod fsio;
/// локальная история прослушиваний (перенос history.rs)
pub mod journal;
/// полки главной аккаунта на диске: главная видна, пока сессия поднимается
pub mod home_cache;
/// главная, «Для тебя», топ, недавнее и подписи по URI через сессию (GraphQL и spclient), без Web API
pub mod home_feed;
/// реестр известных треков и «записей» (ISRC)
pub mod known;
/// дерево плейлистов аккаунта на диске: библиотека видна, пока сессия поднимается
pub mod library_cache;
/// «Любимые треки»: кэш на диске и оптимистичные намерения (перенос liked.rs)
pub mod liked;
/// постраничный список (перенос PagedList из model.rs)
pub mod paged;
/// поиск через GraphQL-шлюз клиентов Spotify (pathfinder) с токеном сессии
pub mod pathfinder;
/// запрос к pathfinder по хэшу сохранённого запроса; хэши из бандла веб-плеера, кэш на диске
pub mod persisted;
/// профиль аккаунта через сессию librespot (имя, аватар, тип аккаунта) без Web API
pub mod session_profile;
/// кэш строк плейлистов по аккаунту
pub mod playlist_cache;
/// плейлисты через сессию: список библиотеки из rootlist и все правки (Web API — запасной путь)
pub mod playlist_session;
/// подкасты через сессию: шоу, выпуски, сохранённые; места остановки — из GraphQL-шлюза
pub mod podcast_session;
/// маршрутизация Web API / сессии и тексты ошибок
pub mod route;
/// подписи, склонения, разница списков
pub mod text;
