<p align="center"><a href="README.md">English</a> · <b>Русский</b> · <a href="README.pt-BR.md">Português</a></p>

<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/brand/wordmark-on-dark.svg">
  <img src="docs/brand/wordmark.svg" width="420" alt="Materialfy">
</picture>

[![Release build](https://github.com/makdren/materialfy/actions/workflows/release.yml/badge.svg)](https://github.com/makdren/materialfy/actions/workflows/release.yml)
[![Release](https://img.shields.io/github/v/release/makdren/materialfy?label=release)](https://github.com/makdren/materialfy/releases/latest)
[![Build](https://github.com/makdren/materialfy/actions/workflows/build.yml/badge.svg)](https://github.com/makdren/materialfy/actions/workflows/build.yml)

Клиент Spotify для Android на Rust и Qt Quick, интерфейс в стиле Material 3 Expressive.<br>
Порт настольного Materialfy под телефон.

[Скачать](#скачать) ·
[Android](#android) ·
[Скриншоты](#скриншоты) ·
[Возможности](#возможности) ·
[Ограничения](#чего-нет-и-почему) ·
[Установка](#установка) ·
[Сборка](#сборка-из-исходников) ·
[Данные и приватность](#где-хранятся-данные)

</div>

<br>

Materialfy ставят вместо официального настольного приложения Spotify, чтобы слушать музыку в обычном
нативном окне. Интерфейс написан на QML (Qt 6.12) без браузерного движка, логика на Rust. Звук играет
[librespot](https://github.com/librespot-org/librespot) внутри процесса программы, поэтому для воспроизведения
на компьютере нужен Spotify Premium. Цвета интерфейса подстраиваются под обложку текущего трека. Язык интерфейса
по умолчанию берётся из системы: в русской системе программа на русском, в остальных на английском.
Выбрать язык вручную можно в «Настройки» → «Язык».

Проект собран из двух MIT-проектов: Spotify-часть взята из [spotifast](https://github.com/crmne/spotifast)
(crmne), интерфейс из [yamusic-md3](https://github.com/yaskorinov/yamusic-md3) (yaskorinov). Подробнее в разделе
[Происхождение и лицензия](#происхождение-и-лицензия).

> [!NOTE]
> Это форк [fatyzzz/materialfy](https://github.com/fatyzzz/materialfy) с версией для Android: та же программа
> с раскладкой для телефона, фоновым воспроизведением, плеером в «острове» системы и обновлением APK прямо из
> приложения. Здесь выпускается только APK; версии для Windows, Linux и macOS — в
> [релизах оригинала](https://github.com/fatyzzz/materialfy/releases).

## Скачать

APK лежит в [последнем релизе](https://github.com/makdren/materialfy/releases/latest) (все версии — на [странице Releases](https://github.com/makdren/materialfy/releases)):

| Система | Файл |
| --- | --- |
| Android 9+, arm64 | `Materialfy-<версия>-arm64-v8a.apk` |

Контрольная сумма лежит в том же релизе в `SHA256SUMS.txt`. Программа сама проверяет релизы на новые версии
(«Настройки» → «Обновления»): новый APK скачивается, сверяется с контрольной суммой и передаётся системному
установщику. Сборки для компьютера и их обновления — в [fatyzzz/materialfy](https://github.com/fatyzzz/materialfy/releases).

## Android

<p align="center"><img src="docs/screenshots/android/phone-ru.png" width="100%" alt="Materialfy на телефоне: главная, альбом, «Сейчас играет», меню трека"></p>
<p align="center"><sub>Главная, альбом, «Сейчас играет» и меню трека. Снято на тестовых данных в окне 412×915.</sub></p>

На телефоне работают те же функции, что и на компьютере, в раскладке под одну руку: нижняя навигация, мини-плеер
над ней, полноэкранный плеер (закрывается свайпом вниз), меню нижними листами, долгое нажатие вместо правой кнопки
мыши, системная кнопка «Назад».

- Музыка играет в фоне: уведомление плеера с обложкой и перемоткой, экран блокировки, кнопки гарнитуры и Bluetooth,
  пауза при звонке и при отключении наушников.
- Плеер в «острове» системы: на Android 16 — как Live Update (чип в строке состояния, Samsung Now Bar, HyperOS
  Super Island, ColorOS Fluid Cloud), на vivo — атомный остров OriginOS. Выключается в «Настройки» →
  «Воспроизведение».
- Телефон виден в Spotify Connect как смартфон: на него можно перенести музыку с компьютера и наоборот.
- Статус «Слушает Spotify» в Discord ставит связка Spotify ↔ Discord, на телефоне он работает так же.
- Обновления: «Настройки» → «Обновления» скачивает APK свежего релиза и ставит его поверх.

**Установка.** Скачайте `Materialfy-<версия>-arm64-v8a.apk` из [последнего релиза](https://github.com/makdren/materialfy/releases/latest)
и откройте его. Android попросит разрешить установку из браузера или файлового менеджера — это один раз. Если
стояла тестовая сборка, подписанная другим ключом (`-debugsigned.apk`), сначала удалите её: поверх неё APK из
релиза не встанет. Вход такой же, как на компьютере: откроется браузер, после входа вернитесь в приложение.

Нужен телефон на arm64 с Android 9 или новее, для звука — Spotify Premium. Версия для Android пока
экспериментальная: о проблемах пишите в [Issues](https://github.com/makdren/materialfy/issues), лучше с
моделью телефона и версией Android.

## Скриншоты

Сняты с аккаунтом Premium во время воспроизведения; цвета берутся из обложки текущего трека.

<table>
  <tr>
    <td width="50%"><img src="docs/screenshots/materialfy/ru/home-dark.png" alt="Главная, тёмная тема"></td>
    <td width="50%"><img src="docs/screenshots/materialfy/ru/home-light.png" alt="Главная, светлая тема"></td>
  </tr>
  <tr>
    <td><img src="docs/screenshots/materialfy/ru/now-playing.png" alt="«Сейчас играет» с синхронным текстом"></td>
    <td><img src="docs/screenshots/materialfy/ru/artist.png" alt="Страница исполнителя"></td>
  </tr>
  <tr>
    <td><img src="docs/screenshots/materialfy/ru/releases.png" alt="Новинки"></td>
    <td><img src="docs/screenshots/materialfy/ru/settings.png" alt="Настройки"></td>
  </tr>
  <tr>
    <td><img src="docs/screenshots/materialfy/ru/account.png" alt="Аккаунт Spotify"></td>
    <td><img src="docs/screenshots/materialfy/ru/playback.png" alt="Воспроизведение на этом компьютере"></td>
  </tr>
  <tr>
    <td><img src="docs/screenshots/materialfy/ru/compact-900x640.png" alt="Окно 900×640 с узкой боковой панелью"></td>
    <td><img src="docs/screenshots/materialfy/ru/compact-640x720.png" alt="Компактная раскладка 640×720"></td>
  </tr>
  <tr>
    <td valign="top"><img src="docs/screenshots/materialfy/ru/mini-300x120.png" alt="Мини-плеер 300×120"></td>
    <td valign="top"><sub>Мини-плеер: в него окно переходит, если сделать его уже 480 или ниже 400 пикселей.</sub></td>
  </tr>
</table>

## Возможности

- Вход через браузер по OAuth PKCE. Пароль вы вводите на сайте Spotify, программа получает только одноразовый
  код на `127.0.0.1`.
- Воспроизведение на этом компьютере через librespot (нужен Premium): 96, 160 или 320 кбит/с, нормализация
  громкости, воспроизведение без пауз между треками, кроссфейд до 12 секунд, скорость от 0,5× до 2× без
  смены высоты голоса, похожая музыка после конца альбома или плейлиста, выбор устройства вывода, аудиокэш
  размером от 256 МБ до 10 ГБ.
- Компьютер появляется в Spotify Connect под именем из настроек. Можно перенести
  воспроизведение на телефон или колонку и обратно, а когда музыка играет на другом устройстве, окно работает
  как пульт с его громкостью. Колонки в локальной сети, которые ещё не вошли в аккаунт, находятся через mDNS
  и подключаются из меню устройств.
- Очередь: ручная очередь отдельно от контекста, «Играть следующим», перетаскивание треков в очередь и
  перестановка строк, удаление с отменой.
- Радио по треку, исполнителю, альбому или плейлисту и по любому треку из поиска. Станцию можно пересобрать
  или сохранить как плейлист.
- Библиотека: плейлисты с папками, закрепление наверху (только в этом приложении), «Любимые треки», альбомы,
  исполнители, подкасты. Вид сеткой или списком, сортировка.
- Редактирование своих плейлистов: название, описание, обложка, публичность, перестановка треков мышью,
  удаление с кнопкой «Вернуть», добавление треков из недавних, любимых и поиска, создание и удаление плейлистов.
- Поиск по трекам, альбомам, исполнителям, плейлистам и подкастам. Ссылку `open.spotify.com` можно вставить
  или перетащить в окно.
- Страница исполнителя с числом слушателей в месяц, биографией, популярными треками и релизами. Раздел
  «Новинки» собирает свежие релизы исполнителей, на которых вы подписаны.
- Подкасты: страницы шоу и выпусков, прогресс прослушивания, главы, перемотка на 10 секунд назад и 30 вперёд.
- Синхронный текст песни. Сначала программа берёт текст у Spotify, при его отсутствии ищет в LRCLIB, NetEase
  и Musixmatch. Три стиля на выбор (классический, Spicy, караоке), размер текста
  и подложка под текстом.
- Схема Material You из обложки текущего трека или из выбранного акцентного цвета, светлая и тёмная тема.
  Анимации можно укоротить или взять настройку системы.
- Три раскладки окна: полная, компактная для узкого окна и мини-плеер с пунктом «поверх всех окон»
  (на Wayland этого пункта нет).
- Своя рамка окна на всех ОС: кнопки «свернуть», «развернуть» и «закрыть» в стиле Material 3, скруглённые углы
  (в Windows 11 их рисует система, в Windows 10 углы прямые), у развёрнутого окна углы прямые.
- Медиаклавиши, плитка SMTC в Windows и MPRIS в Linux, значок в трее со сворачиванием окна при закрытии,
  один запущенный экземпляр.
- Интерфейс на английском, русском или португальском (Бразилия). По умолчанию язык как в системе, переключается в «Настройки» → «Язык»
  («Settings» → «Language»).
- Прокси: системный, HTTP или SOCKS5, с логином и паролем. Через него идут вход, Web API, воспроизведение,
  тексты и картинки.

## Чего нет и почему

- librespot работает только с Premium-аккаунтами. С бесплатным аккаунтом музыка на компьютере не играет,
  а радио, которое строится через librespot, заблокировано. Web API Spotify тоже разрешает управлять
  другими устройствами только с Premium. Без него остаются библиотека, поиск и правка плейлистов.
- Spotify отдаёт librespot до 320 кбит/с Ogg Vorbis, так что lossless в программе нет.
- Кроссфейд и скорость работают только при воспроизведении на этом компьютере. На колонку или телефон
  через Connect они не передаются, а другие устройства Spotify видят позицию трека с поправками раз в секунду.
- Скачать треки для офлайна нельзя, DRM программа не обходит. Аудиокэш только ускоряет повторное
  воспроизведение.
- Общий Client ID для Web API один на всех пользователей программы, и Spotify часто его ограничивает.
  Поэтому программа сначала ходит через вашу сессию Spotify, а Web API держит запасным путём. Подробности и
  настройка своего Client ID в разделе [Сессия Spotify и Web API](#сессия-spotify-и-web-api).
- Часть ограничений идёт от самого Web API. Очередь контекста показывает около 20 ближайших треков: общего
  числа API не отдаёт. Удаление трека из плейлиста убирает все его повторы, потому что метод удаления не
  принимает позицию. История Spotify ограничена последними 50 прослушиваниями.
- Программа неофициальная. Spotify может ограничивать сторонние клиенты, и за использование аккаунта отвечаете вы.

## Требования

- Аккаунт Spotify. Для звука на этом компьютере и для радио нужен Premium.
- Windows 10 1809 или новее (x64); Linux x86_64 или aarch64; macOS 13 или новее (сборка экспериментальная, медиаклавиши на macOS не проверены); Android 9 или новее (arm64).
- Свободные локальные порты 8989 и 8898: на них Spotify возвращает браузер после входа. Если брандмауэр спросит,
  разрешите подключения на `127.0.0.1`.

## Установка

Быстрее всего взять готовую сборку из раздела [Скачать](#скачать). Собрать из исходников можно так:

### Windows

Нужны Rust, Qt 6.12 и Build Tools для Visual Studio (подробности в разделе [Сборка](#windows-1)). Скрипт
собирает программу и инсталлер:

```powershell
git clone https://github.com/makdren/materialfy
cd materialfy
.\build_windows.ps1 -QtDir C:\Qt\6.12.0\msvc2022_64
```

В `target\dist\` появятся `Materialfy-<версия>-Setup.exe` и портативная папка `materialfy\`. Инсталлер ставит
программу без прав администратора в `%LocalAppData%\Programs\Materialfy`.

### Linux

```bash
git clone https://github.com/makdren/materialfy
cd materialfy
./install.sh
```

Скрипт работает с apt, pacman, dnf и zypper. Сначала он показывает, какие системные пакеты поставит через
sudo, и ждёт подтверждения. Rust он ставит через rustup в `~/.cargo`. Если в системе нет Qt 6.12 с QML-модулями,
скрипт скачивает его рядом с исходниками, около 250 МБ. Первая сборка занимает от 5 до 15 минут, после неё
появляются команда `~/.local/bin/materialfy` и значок в меню приложений. Повторный запуск скрипта обновляет
программу.

```bash
./install.sh --uninstall   # убрать команду и значок; настройки, данные и вход остаются
```

### macOS

Инсталлера нет, соберите `.app` по инструкции в разделе [Сборка](#macos-экспериментально).

### Android

APK из релиза — в разделе [Android](#android), сборка APK из исходников — в разделе
[Сборка](#android-экспериментально).

## Первый запуск

Spotify выдаёт два отдельных разрешения, и программа просит их по очереди.

1. Вход в аккаунт: «Аккаунт» → «Войти через Spotify». Откроется браузер, после подтверждения Spotify вернёт
   его на `http://127.0.0.1:8989/login`. Этого достаточно для библиотеки, поиска и плейлистов.
2. Воспроизведение на этом компьютере: экран «Воспроизведение» или меню устройств. Второе подтверждение
   в браузере приходит на `http://127.0.0.1:8898/login`. После него компьютер появляется в Spotify Connect.

Без второго шага Materialfy работает как пульт для других устройств (с Premium).

## Сессия Spotify и Web API

У программы два пути к данным Spotify.

- Сессия librespot. Это тот же протокол, на котором работает официальное приложение; она появляется после
  второго шага входа (воспроизведение на этом компьютере). Сессия работает от вашего аккаунта, квоты Web API
  на неё не действуют.
- Web API (`api.spotify.com`) через Client ID. По умолчанию это общий Client ID программы, один на всех её
  пользователей. Spotify часто отвечает ему 429 («слишком много запросов»), а иногда `QUOTA_EXCEEDED`,
  и тогда до сброса квоты, бывает до суток, Web API не отвечает вовсе.

Чтение сначала идёт через сессию, а Web API спрашивается, только если сессии нет, она не ответила вовремя
или такого запроса не умеет. Через сессию сейчас читаются плейлисты (список, содержимое, правка состава),
«Любимые треки», сохранённые альбомы и исполнители, страницы альбома, исполнителя и трека, поиск и
«Новинки». Пока только через Web API работают подкасты в библиотеке и их страницы, история прослушиваний,
полки главной «Топ» и «Сделано для вас», подписи сидов радио, создание плейлиста, смена названия, обложки и
подписка на чужой плейлист, а также пульт для другого устройства: команды и опрос его состояния.

Чтобы общий Client ID расходовался медленнее, клиент Web API:

- делит запросы на три очереди: страница, которую вы открыли, фоновое обновление и опрос состояния плеера.
  Фону достаётся до двух третей бюджета (у общего ключа 40 запросов за 30 секунд), опросу четверть,
  страницам весь;
- склеивает одинаковые запросы, которые уже в пути, и 20 секунд держит ответы GET (после любой записи
  кэш сбрасывается и минуту не наполняется);
- после 429 или исчерпанной квоты не пускает в сеть фон и опрос, а страница пробует раз в 15 секунд, при
  квоте раз в минуту. Опрос плеера после каждого 429 подряд ждёт дольше: минуту, две, четыре, до десяти минут.

Пока ограничение действует, в «Настройки» → «Аккаунт» видна плашка «Web API ограничен» с примерным временем
окончания.

### Свой Client ID

Если общий ключ часто упирается в лимиты, зарегистрируйте своё приложение Spotify. Его лимиты делятся только
на вас.

1. Откройте [developer.spotify.com/dashboard](https://developer.spotify.com/dashboard), войдите и нажмите
   «Create app».
2. Название и описание любые. В Redirect URIs добавьте `http://127.0.0.1:8989/login`, в разделе API
   отметьте Web API. Сохраните.
3. Новое приложение работает в режиме разработки и пускает только пользователей из списка. Добавьте свой
   email от аккаунта Spotify в «User Management».
4. Скопируйте Client ID (32 символа) со страницы приложения.
5. В Materialfy: «Настройки» → «Аккаунт» → «Личное приложение Spotify» → «Настроить». В форме показан redirect
   URI из шага 2 с кнопкой копирования. Вставьте Client ID, нажмите «Сохранить» и подтвердите доступ в браузере.

Часть запросов Web API и со своим Client ID идёт через общий: чужие плейлисты, список ваших плейлистов,
поиск плейлистов, популярные треки и похожие исполнители на странице исполнителя.

## Где хранятся данные

| Что | Windows | Linux | macOS |
|---|---|---|---|
| Настройки | реестр `HKCU\Software\materialfy\Materialfy` | `~/.config/materialfy/Materialfy.conf` | `~/Library/Preferences/materialfy/Materialfy.conf` |
| Данные (сессия плеера, история) | `%APPDATA%\materialfy\Materialfy` | `~/.local/share/materialfy/Materialfy` | `~/Library/Application Support/materialfy/Materialfy` |
| Кэш (аудио, обложки, тексты) | `%LOCALAPPDATA%\materialfy\Materialfy\cache` | `~/.cache/materialfy/Materialfy` | `~/Library/Caches/materialfy/Materialfy` |
| Токены и пароль прокси | Диспетчер учётных данных | Secret Service (связка ключей) | Связка ключей |

Секреты лежат только в системном хранилище под именем `materialfy`.
«Выйти» в настройках удаляет все разрешения Spotify из хранилища. Кэш при этом остаётся, его можно очистить
в «Настройки» → «Хранилище».

## Приватность

Своей телеметрии и сбора статистики в программе нет. Сетевые запросы идут к серверам Spotify (вход, Web API,
воспроизведение, обложки) и к сервисам текстов LRCLIB, NetEase и Musixmatch. В локальной сети программа ищет
устройства Spotify Connect через mDNS.

Пароль от Spotify вы вводите только на сайте `accounts.spotify.com` в своём браузере. Программа запрашивает
доступ к библиотеке, плейлистам и управлению воспроизведением, а тариф (Free или Premium) читает через
`user-read-private`. Доступа к почте она не просит.

## Горячие клавиши

| Клавиши | Действие |
|---|---|
| <kbd>Space</kbd> | играть или пауза |
| <kbd>Ctrl</kbd> + <kbd>→</kbd> / <kbd>←</kbd>, медиаклавиши | следующий или предыдущий трек |
| <kbd>Ctrl</kbd> + <kbd>↑</kbd> / <kbd>↓</kbd> | громкость |
| <kbd>Shift</kbd> + <kbd>→</kbd> / <kbd>←</kbd> | перемотка на 10 секунд |
| <kbd>Ctrl</kbd> + <kbd>P</kbd> | полноэкранный плеер |
| <kbd>Ctrl</kbd> + <kbd>L</kbd> | текст песни |
| <kbd>Ctrl</kbd> + <kbd>F</kbd> | поиск в списке, повторное нажатие открывает общий поиск |
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>L</kbd> | библиотека |
| <kbd>Ctrl</kbd> + <kbd>B</kbd> | свернуть или развернуть боковую панель |
| <kbd>Ctrl</kbd> + <kbd>,</kbd> | настройки |
| <kbd>Alt</kbd> + <kbd>←</kbd>, кнопка мыши «назад» | назад |
| <kbd>F11</kbd> | на весь экран |
| <kbd>Ctrl</kbd> + <kbd>Q</kbd> | выход в Linux (в macOS <kbd>⌘</kbd> + <kbd>Q</kbd>) |

## Сборка из исходников

Общие требования:

- Rust 1.98 или новее ([rustup](https://rustup.rs); в Windows toolchain `stable-x86_64-pc-windows-msvc`).
- Qt 6.12 с модулями qtdeclarative, qtsvg, qtshadertools и qtimageformats. Его можно поставить через
  Qt Online Installer или скриптом [`install_qt.py`](.github/scripts/install_qt.py), которым пользуется CI:
  он качает архивы с download.qt.io и проверяет контрольные суммы.
- Python 3 для `install_qt.py`.

cxx-qt находит Qt по переменной `QMAKE` (путь к `qmake6`). Собранная программа ищет QML в таком порядке:
`$MATERIALFY_QML_DIR`, затем `qml/` рядом с exe, затем `src/materialfy/qml` в репозитории, поэтому `cargo run`
работает из исходного дерева.

Собирайте с `--locked`. В `Cargo.lock` зафиксирован `vergen 9.0.6`, версия 9.1 ломает build-скрипт
`librespot-core`. Не запускайте `cargo update` без `-p`.

### Windows

Понадобятся [Build Tools for Visual Studio 2022](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
с нагрузкой «Разработка классических приложений на C++», Rust (msvc), Qt 6.12 `msvc2022_64`, PowerShell 7 и для
инсталлера [Inno Setup 6](https://jrsoftware.org/isinfo.php) (`winget install JRSoftware.InnoSetup`).

```powershell
python .github\scripts\install_qt.py --out C:\Qt\6.12.0\msvc2022_64   # если Qt ещё нет
.\build_windows.ps1 -QtDir C:\Qt\6.12.0\msvc2022_64
```

[`build_windows.ps1`](build_windows.ps1) запускает `cargo build --locked --release -p materialfy`, собирает
в `target\dist\materialfy\` exe, `qml\`, `assets\`, библиотеки Qt (через windeployqt) и рантайм MSVC, затем
вызывает Inno Setup, если найдёт его. Если `-QtDir` не указан, скрипт ищет Qt в `$env:QTDIR`, рядом с
`$env:QMAKE`, в `C:\Qt\6.12.0\msvc2022_64` и `~\Qt\6.12.0\msvc2022_64`.

| Ключ | Что делает |
|---|---|
| `-SkipInstaller` | без шага Inno Setup |
| `-Portable` | дополнительно `Materialfy-<версия>-windows-x64-portable.zip` |
| `-SkipBuild` | взять готовый `target\<профиль>\materialfy.exe` |
| `-CargoProfile debug` | отладочная сборка |
| `-Version 1.2.3` | версия для инсталлера (по умолчанию из `Cargo.toml`) |
| `-OutDir DIR` | куда класть результат (по умолчанию `target\dist`) |

Для разработки без упаковки:

```powershell
.\run_windows.bat --new-instance      # cargo run --release с найденным Qt; аргументы идут программе
```

или вручную:

```powershell
$env:QMAKE = "C:\Qt\6.12.0\msvc2022_64\bin\qmake6.exe"
$env:PATH = "C:\Qt\6.12.0\msvc2022_64\bin;$env:PATH"
cargo run --locked -p materialfy
```

### Linux

Проще всего запустить `./install.sh` (см. [Установка](#linux)). Вручную на Debian или Ubuntu:

```bash
sudo apt install build-essential pkg-config lld python3 7zip libasound2-dev libpulse-dev \
    libgl-dev libegl-dev libxkbcommon-dev libfontconfig1-dev libdbus-1-dev libxcb-cursor0 libxkbcommon-x11-0

python3 .github/scripts/install_qt.py --out ~/Qt/6.12.0/gcc_64
export QMAKE=~/Qt/6.12.0/gcc_64/bin/qmake6 LD_LIBRARY_PATH=~/Qt/6.12.0/gcc_64/lib

cargo run --locked --release -p materialfy
```

В старых Ubuntu пакет `7zip` называется `p7zip-full`. На Arch: `sudo pacman -S base-devel lld alsa-lib libpulse 7zip`.
cxx-qt не линкуется через GNU ld.bfd, нужен `lld`, `mold` или `gold`.

AppImage и `tar.gz` собирает [`package_linux.sh`](.github/scripts/package_linux.sh), результат в `target/dist/`:

```bash
bash .github/scripts/package_linux.sh 0.1.0
```

### macOS (экспериментально)

```bash
brew install sevenzip
python3 .github/scripts/install_qt.py --host mac --out ~/Qt/6.12.0/macos
export QMAKE=~/Qt/6.12.0/macos/bin/qmake6
cargo build --locked --release -p materialfy
bash .github/scripts/package_macos.sh 0.1.0   # target/dist/Materialfy.app и .dmg
```

### Android (экспериментально)

Те же ядро на Rust и QML работают на Android (arm64-v8a, Android 9+) с раскладкой для телефона: нижняя навигация,
мини-плеер над ней и полноэкранный плеер, меню нижними листами, касания вместо наведения. Музыка играет в фоне
через уведомление плеера (экран блокировки, кнопки гарнитуры, аудиофокус). Вход — как на компьютере: открывается
браузер, Spotify возвращает его на `127.0.0.1`. Токены хранятся файлами в закрытом каталоге приложения (связки
ключей для нативного кода в Android нет), резервное копирование приложения выключено.

Нужны Rust с целью `aarch64-linux-android`, JDK 17+, Android SDK (platform 36, build-tools 36) с NDK r28 и две
установки Qt 6.12: для Android и для хоста (его инструменты собирают пакет).

```bash
rustup target add aarch64-linux-android
python3 .github/scripts/install_qt.py --host android --out ~/Qt/6.12.0/android_arm64_v8a
python3 .github/scripts/install_qt.py --out ~/Qt/6.12.0/gcc_64
export QT_ANDROID=~/Qt/6.12.0/android_arm64_v8a QT_HOST=~/Qt/6.12.0/gcc_64
export ANDROID_SDK_ROOT=~/Android/Sdk ANDROID_NDK_ROOT=~/Android/Sdk/ndk/28.2.13676358
android/build_apk.sh --release   # target/android/Materialfy-<версия>-arm64-v8a-debugsigned.apk
```

Плеер умеет жить и в «острове» системы: на Android 16 — как Live Update (чип в статус-баре, Samsung Now Bar,
HyperOS Super Island, ColorOS Fluid Cloud), на телефонах vivo — в атомном острове OriginOS (Настройки →
Воспроизведение → «Плеер в острове системы»). Музыкальную карточку острова vivo открывает только разрешённым
прошивкой приложениям, поэтому на части прошивок останется обычное уведомление плеера. Статус «Слушает
Spotify» в Discord ставит сама связка Spotify ↔ Discord: Materialfy играет как устройство Spotify Connect, так что
на Android это работает так же.

Обновления приходят из GitHub Releases этого форка (`makdren/materialfy`): Настройки → Обновления скачивает
`-arm64-v8a.apk` свежего релиза, сверяет его с `SHA256SUMS.txt` и отдаёт системному установщику. Android ставит
обновление только поверх APK, подписанного тем же ключом, поэтому сборка в CI (`build.yml`, задача `android`)
подписывает его ключом из секретов репозитория `ANDROID_KEYSTORE_BASE64` и `ANDROID_KEYSTORE_PASS` (псевдоним
`materialfy` или переменная `ANDROID_KEY_ALIAS`). Тег `v*` публикует релиз с APK; без ключа релиз не
создаётся.

Без ключа APK подписан отладочным ключом SDK: он ставится как есть, но сборку с другим ключом поверх него не
поставить. Для своего ключа задайте `ANDROID_KEYSTORE`, `ANDROID_KEY_ALIAS` и `ANDROID_KEYSTORE_PASS`. Часть для
Android — в [`android/`](android) (манифест, активность, сервисы воспроизведения и входа) и
[`crates/materialfy/cpp/mobile.cpp`](crates/materialfy/cpp/mobile.cpp); раскладка телефона —
[`MobileMain.qml`](src/materialfy/qml/MobileMain.qml). На компьютере ключ `--mobile` открывает её же в окне,
например `--mobile --size 412x915`.

### Ключи запуска и переменные

| Ключ | Что делает |
|---|---|
| `--offline` | не восстанавливать вход из хранилища секретов |
| `--new-instance` | запустить второй экземпляр, даже если программа уже открыта |
| `--light` | светлая тема |
| `--size 1280x800` | размер окна |
| `--screenshot shot.png`, `--delay 1500` | сохранить снимок окна через указанное число миллисекунд и выйти |
| `--config-dir DIR` | отдельный каталог настроек |
| `--set ключ=значение` | начальное свойство окна, например `--set startPage=settings` |
| `--gallery` | галерея компонентов Material 3 |
| `--mobile` | раскладка телефона (как на Android) |

Переменные окружения: `MATERIALFY_QML_DIR` задаёт свой каталог QML, `MATERIALFY_SESSION_FILE` задаёт файл
сессии плеера, `RUST_LOG` управляет журналом (например, `RUST_LOG=materialfy=debug,sf_player=debug`).

### Тесты и CI

```bash
cargo test --locked --workspace
```

[`build.yml`](.github/workflows/build.yml) запускается на push и pull request в `main` и вручную. В этом форке
он собирает только APK; задачи Windows, Linux и macOS пропускаются, пока переменная репозитория `DESKTOP_BUILDS`
не равна `true`.

## Как устроено

Интерфейс написан на QML, компоненты Material 3 Expressive лежат в `src/materialfy/qml/Md3`. Ядро на Rust
связано с QML через [cxx-qt](https://github.com/KDAB/cxx-qt). Асинхронный код работает в одном рантайме tokio,
в поток интерфейса Qt приходят готовые снимки состояния.

| Крейт | Что делает |
|---|---|
| `md3-model` | типы данных для QML: треки, альбомы, плейлисты, устройства |
| `sf-auth` | вход по OAuth PKCE, хранение разрешений в системном хранилище секретов, прокси |
| `sf-api` | клиент Spotify Web API: очереди по приоритету, склейка и кэш GET, охлаждение после 429 и квоты |
| `sf-player` | librespot и Spotify Connect, очередь, согласование локального и удалённого воспроизведения |
| `sf-services` | вход, библиотека, каталог, радио, главная, тексты, устройства, история |
| `mf-theme` | схемы Material You из обложки или выбранного цвета |
| `mf-lyrics` | тексты из нескольких источников, разбор LRC, кэш |
| `mf-platform` | каталоги, настройки, MPRIS, единственный экземпляр |
| `mf-i18n` | перевод интерфейса: английские строки в коде, русский и португальский словари из `i18n/ru/*.json` и `i18n/pt/*.json` |
| `materialfy` | исполняемый файл: мосты в QML, ключи запуска, кэш картинок, рамка окна в Windows |

## Происхождение и лицензия

Этот репозиторий — форк [fatyzzz/materialfy](https://github.com/fatyzzz/materialfy), в нём добавлена версия
для Android (`android/`, `MobileMain.qml`, `crates/materialfy/cpp/mobile.cpp`). Лицензия та же.

Materialfy основан на двух проектах под лицензией MIT:

- [spotifast](https://github.com/crmne/spotifast) (Carmine Paolino, crmne): вход, клиент Web API, воспроизведение
  через librespot, Spotify Connect, очередь, библиотека и радио (`crates/sf-*`).
- [yamusic-md3](https://github.com/yaskorinov/yamusic-md3) (yaskorinov и участники): QML-интерфейс и библиотека
  компонентов Material 3 Expressive, оболочка на cxx-qt, тема из обложки, тексты песен, платформенные службы,
  скрипты сборки (`src/materialfy/qml`, `crates/materialfy`, `crates/mf-*`).

Сторонние компоненты: [librespot](https://github.com/librespot-org/librespot) (MIT, свой форк [fatyzzz/librespot](https://github.com/fatyzzz/librespot) с патчами crmne),
[fastframe](https://github.com/dennisgr7/fastframe) (MIT, вывод звука и медиаклавиши),
[cxx-qt](https://github.com/KDAB/cxx-qt) (MIT или Apache-2.0), [Qt 6](https://www.qt.io) (LGPL-3.0, подключается
динамически), шрифт Google Sans (SIL Open Font License 1.1) и иконки Material Symbols Rounded (Apache License 2.0).

Materialfy распространяется по лицензии GNU GPL версии 3 или любой более поздней (GPL-3.0-or-later), текст в
[LICENSE](LICENSE). Версии до 0.2.1 включительно вышли под MIT, а код из spotifast и yamusic-md3 сохраняет
уведомление MIT в [LICENSES/MIT.txt](LICENSES/MIT.txt). Авторы, источники и лицензии зависимостей
перечислены в [NOTICE](NOTICE). Логотип «Волна-M» лежит в [`docs/brand/`](docs/brand/).

<br>

<p align="center"><sub>Materialfy не связан со Spotify AB и не одобрен ею. Spotify является товарным знаком Spotify AB.</sub></p>
