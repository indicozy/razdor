---
title: Вопросы
sidebar:
  order: 0
---

## Razdor не находит мою игру. Что делать?

Нужна своя копия «Времён раздора» версии 1.8 с Community Update 1.2. Razdor ищет её через `RAZDOR_DT_DIR`, рядом со своей программой, по запомненному пути и в `~/Games`, `~/Downloads` и домашней папке. Причина записана в `razdor.log`. См. [Установка игры](/razdor/wiki/start/install-game/).

## Как поставить мод?

В окне «Настройки звука, графики и игры» нажмите «Обновить игру из архива…» и выберите архив `.zip` или `.7z`. Архив со всей игрой заменит её, архив с частью файлов ляжет поверх. На компьютере можно указать папку установки с модом в `RAZDOR_DT_DIR`. См. [Установка модов и патчей](/razdor/wiki/mods/install-mod/).

## Где лежат сохранения?

На Windows: `%APPDATA%\razdor\saves`. На Linux: `~/.local/share/razdor/saves`. На macOS: `~/Library/Application Support/razdor/saves`. Свою папку можно задать в `RAZDOR_SAVE_DIR`. См. [Сохранение и загрузка](/razdor/wiki/tutorials/saves/).

## Android не видит папку игры. Что делать?

Дайте Razdor «Доступ ко всем файлам»: на титульном экране для этого есть кнопка «Выдать доступ». Положите папку игры в `DiscordTimes` и нажмите «Проверить снова». См. [Razdor на Android](/razdor/wiki/phones/android/).

## Как обновить Razdor?

Razdor сам проверяет новые выпуски на GitHub и предлагает обновление, когда вы не в бою. Кнопка «Обновить» скачивает новую версию, которая запустится при следующем открытии. Нужна программа `curl`. Режим задаётся в «Ещё…» → «Обновления». См. [Обновления](/razdor/wiki/start/updates/).

## macOS не даёт запустить razdor-macos. Почему?

Программа не подписана разработчиком. Выполните `xattr -d com.apple.quarantine razdor-macos` или откройте её через правую кнопку → Open в Finder. См. [Установка Razdor](/razdor/wiki/start/install-razdor/).

## Как сменить язык или шрифт?

Язык меняется в углу главного меню, кнопкой «Язык» в «Ещё…» или клавишей F2. Шрифт выбирается в окне настроек звука, графики и игры. См. [Язык и шрифты](/razdor/wiki/start/language-fonts/).

## Как сообщить об ошибке?

Напишите в [Discord](https://discord.gg/wmMksmWFDM) или заведите задачу в [GitHub issues](https://github.com/indicozy/razdor/issues).