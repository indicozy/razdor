---
title: Свои спрайты
description: Как заменить заглушки графики в Razdor своими картинками.
sidebar:
  order: 2
---

## Замена графики

Вся графика Razdor сейчас состоит из жетонов-заглушек. Их можно заменить своими картинками.

1. Положите PNG-файлы в папку, например `assets-local/`.
2. Назовите каждый файл по ключу `Key=` воина или предмета из данных игры: `knight.png`, `archmage.png`, `ranger.png`, `spearman.png`, `archer.png`, `swordsman.png`, `healer.png`, `bandit.png`, `bandit_archer.png`, `bandit_chief.png`, `short_sword.png` и так далее.

:::tip
Папка `assets-local/` не попадает в git. Храните там стороннюю графику.
:::

## Запуск с локальными картинками

Чтобы Razdor взял картинки из своей папки, задайте переменную `RAZDOR_ASSETS` с путём к ней. Из исходников это выглядит так:

```sh
RAZDOR_ASSETS=./assets-local cargo run --release
```
