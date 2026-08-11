export type SaveSource = "local" | "steamCloud" | "custom" | "unknown";

export interface CharacterSummary {
  id: string;
  path: string;
  name: string;
  className: string;
  classTag: string;
  level: number;
  male: boolean;
  hardcore: boolean;
  expansionCharacter: boolean;
  source: SaveSource;
  modifiedAt: number | null;
  dataVersion: number;
}

export interface CoreStats {
  levelInBio: number;
  experience: number;
  attributePoints: number;
  skillPoints: number;
  devotionPoints: number;
  totalDevotionPointsUnlocked: number;
  physique: number;
  cunning: number;
  spirit: number;
  health: number;
  energy: number;
}

export interface CharacterDocument extends CharacterSummary {
  coreStats: CoreStats | null;
  iron: number | null;
  items: CharacterItem[];
  inventoryBagCount: number;
  factions: FactionValue[];
  blockCount: number;
  writeSupported: boolean;
  writeBlockers: number[];
  databaseWarnings: string[];
}

export interface FactionValue {
  index: number;
  name: string;
  changed: boolean;
  unlocked: boolean;
  value: number;
  positiveBoost: number;
  negativeBoost: number;
}

export interface FactionPatch {
  index: number;
  value: number;
}

export interface NewInventoryItem {
  bagIndex: number;
  x: number;
  y: number;
  baseRecord: string;
  displayName: string;
  stackCount: number;
}

export type ItemContainer = "inventory" | "equipment" | "weaponSetOne" | "weaponSetTwo" | "stash";

export interface CharacterItem {
  id: string;
  container: ItemContainer;
  containerIndex: number;
  slotIndex: number;
  x: number | null;
  y: number | null;
  displayName: string;
  componentDisplayName: string | null;
  augmentDisplayName: string | null;
  baseRecord: string;
  prefixRecord: string;
  suffixRecord: string;
  modifierRecord: string;
  transmuteRecord: string;
  seed: number;
  componentRecord: string;
  componentBonusRecord: string;
  componentSeed: number;
  augmentRecord: string;
  ascendantAffixRecord: string;
  ascendantAffixTwoHandedRecord: string;
  augmentSeed: number;
  componentCombines: number;
  stackCount: number;
  ascendantRerolls: number;
}

export type ItemPatch = Pick<
  CharacterItem,
  | "id"
  | "baseRecord"
  | "prefixRecord"
  | "suffixRecord"
  | "modifierRecord"
  | "transmuteRecord"
  | "componentRecord"
  | "componentBonusRecord"
  | "augmentRecord"
  | "ascendantAffixRecord"
  | "ascendantAffixTwoHandedRecord"
  | "stackCount"
>;

export interface CharacterPatch {
  characterName: string;
  characterLevel: number;
  hardcore: boolean;
  iron: number;
  coreStats: CoreStats | null;
  items: ItemPatch[];
  newItems: NewInventoryItem[];
  factions: FactionPatch[];
}

export interface SaveResult {
  character: CharacterDocument;
  backupPath: string;
}

export interface RestoreResult {
  character: CharacterDocument;
  restoredBackupPath: string;
  safetyBackupPath: string;
}

export type CatalogKind = "base" | "prefix" | "suffix" | "component" | "augment" | "ascendant";

export interface CatalogItem {
  record: string;
  name: string;
  className: string;
  kind: CatalogKind;
  iconPath: string | null;
  levelRequirement: number | null;
  itemLevel: number | null;
}

export interface GameDatabaseInfo {
  installPath: string;
  databaseFiles: string[];
  localizationFiles: string[];
  resourceFiles: string[];
}

export interface CatalogSearchResult {
  database: GameDatabaseInfo;
  total: number;
  items: CatalogItem[];
}
