<div align="center">

# ◇ DREEG ◇

### Grim Dawn Save Editor

**A local desktop forge for shaping your characters safely.**

![Version](https://img.shields.io/badge/version-1.0.0-c9a857?style=for-the-badge)
![Platform](https://img.shields.io/badge/platform-Windows_x64-20251f?style=for-the-badge)
![Grim Dawn](https://img.shields.io/badge/Grim_Dawn-1.3-7b4b2a?style=for-the-badge)
![Mode](https://img.shields.io/badge/data-local_only-42564a?style=for-the-badge)

</div>

> **Your save never leaves your machine.** Dreeg runs as a self-contained desktop application and reads the game data directly from your Grim Dawn installation.

---

## ⚔ Character Hall

| Feature | What it does |
|:--|:--|
| **Automatic discovery** | Finds characters in local saves, OneDrive-redirected Documents folders and Steam Cloud directories. |
| **Character search** | Filters the local roster instantly by character name. |
| **True class names** | Resolves mastery combinations into their localized class names instead of displaying internal IDs. |
| **Mastery emblems** | Uses color-coded minimalist icons for every mastery and all 55 possible class combinations. |
| **Character overview** | Presents level, game mode, source, progression and write-compatibility at a glance. |

## ✦ Character Shaping

| Feature | Editable values |
|:--|:--|
| **Progression** | Level, experience and Hardcore mode. |
| **Resources** | Iron, health and energy. |
| **Attributes** | Physique, Cunning and Spirit. |
| **Available points** | Attribute, skill and devotion points. |
| **Factions** | Reputation from hostile to revered, with styled progress and reputation tiers. |
| **Numeric controls** | Consistent custom increment and decrement controls with field-specific limits. |

## 🛡 Arsenal & Inventory

| Feature | What it does |
|:--|:--|
| **Inventory browser** | Shows every inventory bag and personal stash with bag, row, column and raw coordinates. |
| **Equipment viewer** | Displays equipped armor, accessories and both weapon sets. |
| **Protected existing items** | Existing inventory, stash and equipped items remain read-only to preserve save integrity. |
| **Item inspection** | Shows the real item name, base record, quantity, prefix, suffix, component, augment and Ascendant data. |
| **Item creation** | Adds new equipment, consumables, components and augments to a selected inventory bag and position. |
| **Visual catalog** | Provides searchable, illustrated item selection using artwork extracted from the installed game archives. |
| **GrimTools shortcut** | Opens the selected record in GrimTools for extended item details. |

## ◆ Native Game Data

| Feature | What it does |
|:--|:--|
| **Local item database** | Builds the catalog from the base game and installed expansion databases. |
| **Real names** | Resolves classes, items, affixes, factions, components and augments through the installed English localization. |
| **Real artwork** | Extracts item, equipment, component and augment textures from `Items.arc` and `UI.arc`. |
| **Fast image cache** | Converts artwork to reusable local PNGs for responsive browsing. |
| **Database diagnostics** | Isolates invalid archives, keeps available data usable and reports integrity warnings in the interface. |

## ⛨ Save Protection

| Feature | What it does |
|:--|:--|
| **Game-process guard** | Refuses to write while Grim Dawn is running. |
| **Compatibility gate** | Enables only mutations supported by the detected save layout and fails closed for unknown formats. |
| **Pre-write validation** | Validates values, structure and checksums before replacing the active save. |
| **Versioned backups** | Creates a recoverable backup before every successful write. |
| **Atomic replacement** | Writes through a temporary file and rolls back if the final replacement fails. |
| **Restore backup** | Restores the newest valid backup after confirmation and creates a safety copy of the current save. |
| **Discard changes** | Reverts every unsaved field and pending item to the last loaded character state. |

## ◈ Desktop Experience

| Feature | What it does |
|:--|:--|
| **English-first interface** | Keeps the application, classes and item catalog aligned with the game's English terminology. |
| **Embedded backend** | Integrates the Rust backend directly with the HTML/CSS interface through Tauri. |
| **No separate server** | Runs as a portable executable or installed Windows application without starting a service. |
| **Offline operation** | Discovers, reads, validates and edits saves locally without uploading character data. |

---

<div align="center">

**Dreeg 1.0.0 · Forged for Grim Dawn 1.3**

</div>
