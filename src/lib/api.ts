import { invoke } from "@tauri-apps/api/core";
import type {
  CharacterDocument,
  CharacterPatch,
  CharacterSummary,
  CatalogKind,
  CatalogSearchResult,
  GameDatabaseInfo,
  RestoreResult,
  SaveResult,
} from "../types";
import { previewCatalog, previewCharacter, updatePreviewCharacter } from "./preview";

const desktopAvailable = "__TAURI_INTERNALS__" in window;
const previewMode = import.meta.env.DEV && new URLSearchParams(window.location.search).has("preview");
let previewBackup: CharacterDocument | null = null;

export async function scanCharacters(): Promise<CharacterSummary[]> {
  if (previewMode) return [previewCharacter];
  if (!desktopAvailable) return [];
  return invoke<CharacterSummary[]>("scan_characters");
}

export async function loadCharacter(id: string): Promise<CharacterDocument> {
  if (previewMode) return structuredClone(previewCharacter);
  return invoke<CharacterDocument>("load_character", { id });
}

export async function saveCharacter(
  id: string,
  patch: CharacterPatch,
): Promise<SaveResult> {
  if (previewMode) {
    previewBackup = structuredClone(previewCharacter);
    const character: CharacterDocument = {
      ...previewCharacter,
      name: patch.characterName,
      level: patch.characterLevel,
      hardcore: patch.hardcore,
      iron: patch.iron,
      coreStats: patch.coreStats,
      items: previewCharacter.items.concat(patch.newItems.map((newItem, index) => {
        const catalog = previewCatalog.find((candidate) => candidate.record === newItem.baseRecord);
        return {
          ...previewCharacter.items[0],
          id: `inventory:${newItem.bagIndex}:preview-${index}`,
          container: "inventory" as const,
          containerIndex: newItem.bagIndex,
          slotIndex: previewCharacter.items.length + index,
          x: newItem.x,
          y: newItem.y,
          baseRecord: newItem.baseRecord,
          displayName: catalog?.name ?? "Unknown item",
          stackCount: newItem.stackCount,
        };
      })),
      factions: previewCharacter.factions.map((faction) => {
        const changedFaction = patch.factions.find((candidate) => candidate.index === faction.index);
        return changedFaction ? { ...faction, value: changedFaction.value } : faction;
      }),
    };
    updatePreviewCharacter(character);
    return { character, backupPath: "C:\\Dreeg\\backups\\preview\\player.gdc" };
  }
  return invoke<SaveResult>("save_character", { id, patch });
}

export async function restoreLatestBackup(id: string): Promise<RestoreResult> {
  if (previewMode) {
    if (!previewBackup) throw new Error("No backup is available for this character yet.");
    const current = structuredClone(previewCharacter);
    const character = structuredClone(previewBackup);
    previewBackup = current;
    updatePreviewCharacter(character);
    return {
      character,
      restoredBackupPath: "C:\\Dreeg\\backups\\preview\\player.gdc",
      safetyBackupPath: "C:\\Dreeg\\backups\\preview-safety\\player.gdc",
    };
  }
  return invoke<RestoreResult>("restore_latest_backup", { id });
}

export async function gameDatabaseStatus(): Promise<GameDatabaseInfo | null> {
  if (previewMode) return { installPath: "C:\\Steam\\Grim Dawn", databaseFiles: ["database.arz", "GDX1.arz", "GDX2.arz", "GDX3.arz"], localizationFiles: ["Text_EN.arc"], resourceFiles: ["Items.arc", "UI.arc"] };
  return invoke<GameDatabaseInfo | null>("game_database_status");
}

export async function loadItemIcons(records: string[]): Promise<Record<string, string | null>> {
  const unique = [...new Set(records.filter(Boolean).map((record) => record.toLocaleLowerCase("en-US")))];
  if (!unique.length) return {};
  if (previewMode) return Object.fromEntries(unique.map((record) => [record, null]));
  return invoke<Record<string, string | null>>("load_item_icons", { records: unique });
}

export async function searchItemCatalog(
  query: string,
  kind: CatalogKind | null = "base",
  limit = 100,
): Promise<CatalogSearchResult> {
  if (previewMode) {
    const normalized = query.toLocaleLowerCase("en-US");
    const items = previewCatalog.filter((item) =>
      (kind === null || item.kind === kind)
      && item.name.toLocaleLowerCase("en-US").includes(normalized));
    return { database: (await gameDatabaseStatus())!, total: items.length, items: items.slice(0, limit) };
  }
  return invoke<CatalogSearchResult>("search_item_catalog", { query, kind, limit });
}

export { desktopAvailable };
