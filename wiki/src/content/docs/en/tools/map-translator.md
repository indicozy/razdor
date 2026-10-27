---
title: Card Translator
description: Instructions for using the razdor-translate utility to translate cards and game data.
sidebar:
  order: 2
---

`razdor-translate` is a standalone program for creating localized versions of your cards and game data using Large Language Models (LLM).

## Key Features

The program allows you to create copies of cards (e.g., from `Name.DTm` to `Name_en.DTm`) and translate game data.

*   **API Support:** Works with any OpenAI-compatible API. You can use local servers (Ollama, LM Studio) or cloud services (OpenAI, OpenRouter, DeepSeek).
*   **Card Translation:** Creates copies of cards for the selected language. In the "Letters" field choose "Latin or Cyrillic": these cards can also be opened by the original game. "Any script" (hieroglyphs, Devanagari, etc.) gives cards that work only in Razdor.
*   **Game Data Translation:** Translates names and descriptions of warriors, items, spells, and game texts. The result is saved into separate `.ini` files (e.g., `Eng_Units.ini`).
*   **Retelling (Russian):** When selecting the Russian language, the program does not just translate, but rewrites texts in Russian in a style of your choosing.

## Configuration and Parameters

You can use either a graphical interface or the command line to run the program.

### Interface Parameters
*   **"Check"**: Verifies the API connection and the availability of the required model.
*   **"Context (tokens)"**: Specifies the number of tokens the model remembers at one time.
*   **"Translate to"**: Selection of the target language.
*   **"Letters"**: Choice between Latin or Cyrillic (for compatibility with the original game) and the language's own letters.
*   **"Translation Style"**: Your instructions for the model (e.g., "like a chronicle").
*   **"Context"** (notes on the cards): what the model should know about the cards: who is who, the plot, which names not to change.
*   **"Request Log"**: Opens a file containing all model requests and responses.

:::tip
The API address can be entered without `/v1`. The translator will append it automatically.
:::

### Running without a GUI
If you are running the program via the terminal, use the following flags:
*   `--map "Card Name"` — translate a specific card.
*   `--data` — translate game data.
*   `--language`, `--style`, `--notes`, `--fresh` — set language, style, context, and force a translation restart.
*   `--show-prompt` — output the instructions sent to the model into the log.

## Translation Process

1.  Select a card or campaign from the list on the left.
2.  Configure the language and style parameters.
3.  Click "Translate".

The program first translates names and then texts, using names from a dictionary to ensure consistency. Each model response is checked to ensure markup and variables (e.g., `#HERONAME`) remain intact. If an error occurs, the program will attempt to retry the request.

:::caution
If you change the model, style, or context, the old translation will not update automatically. You must use the "Translate again" function.
:::

## Saving Results

Upon completion, the program can write the finished files to the cards folder.
*   The **"Write English Cards"** button writes what has already been translated.
*   To overwrite existing copies, you need to click the button a second time.

:::note
Translated cards are created based on your files. Do not add them to the project's main repository.
:::
