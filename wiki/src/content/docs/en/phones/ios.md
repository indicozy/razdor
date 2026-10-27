---
title: Razdor on iOS
description: Instructions for installing and configuring the Razdor engine on iPhone and iPad devices.
sidebar:
  order: 2
---

## Installation (Experimental)

For iPhone and iPad with iOS 14 and newer, the **razdor.ipa** file is available in the GitHub releases. This is an experimental version. A new IPA is installed over the old one.

The file is unsigned, so to install it, use **AltStore** or **Sideloadly** with your Apple ID. You do not need to sign the file yourself.

### Method 1: AltStore
1. Run AltServer on your computer.
2. Install AltStore on your phone via AltServer.
3. In the AltStore app, go to the "My Apps" tab and tap "+".
4. Select the downloaded **razdor.ipa** file.

### Method 2: Sideloadly
1. Connect your phone to your computer.
2. Drag the **razdor.ipa** file into the Sideloadly window.
3. Enter your Apple ID.

:::caution
When using a free Apple ID, the app will only work for **7 days**. After that, it must be resigned.
:::

:::tip
If AltServer and your phone are on the same Wi-Fi network, AltStore can refresh the signature automatically. If not, reinstall the IPA via Sideloadly. Your saves will not be deleted during this process.
:::

## System Configuration

If you have iOS 16 or newer, you must perform two actions:
1. Enable **Developer Mode**: "Settings" → "Privacy & Security" → "Developer Mode".
2. Trust the developer profile: "Settings" → "General" → "VPN & Device Management".

## Installing Game Files

To ensure the game works correctly, you need to transfer the content folder to your device.

1. Copy the "Discord Times" game folder (which contains the `Maps_Rus` folder and the `Rus_Units.ini` file).
2. Place it at the following path: "Files" → **On My iPhone** → **Razdor** → **DiscordTimes** folder.

:::note
On a computer, you can find this path via Finder (or iTunes): select your device → "Files" → "Razdor".
:::

After copying, open Razdor and press the **"Check Again"** button. If you don't press it, the engine will check the files itself after a few seconds on the title screen.

## Data Storage

Saves, image cache, and logs are located in the folder:
"Files" → On My iPhone → Razdor → **Razdor** (the `saves` folder is located inside).

No additional permissions are required to access this data.

## Controls

On iOS, controls are performed via touch only. The same gestures available on Android are supported. Touchpad mode is switched on in the same settings window as on Android.
