import type { CatalogItem, CharacterDocument } from "../types";

const previewNames: Record<string, string> = {
  "records/items/gearweapons/axe1h/c001_axe.dbr": "Asterkarn Engraved Axe",
  "records/items/crafting/materials/craft_ugdenbloom.dbr": "Ugdenbloom",
  "records/items/gearaccessories/rings/c001_ring.dbr": "Dreeg Seer's Seal",
  "records/items/gearhead/c001_head.dbr": "Asterkarn Battle Helm",
  "records/items/geartorso/c001_torso.dbr": "Asterkarn Battleplate",
  "records/items/gearweapons/axe2h/c001_axe2h.dbr": "Asterkarn Great Axe",
};

function item(id: string, container: CharacterDocument["items"][number]["container"], slotIndex: number, baseRecord = "") {
  return {
    id,
    container,
    containerIndex: 0,
    slotIndex,
    x: container === "inventory" || container === "stash" ? slotIndex % 8 : null,
    y: container === "inventory" || container === "stash" ? Math.floor(slotIndex / 8) : null,
    displayName: previewNames[baseRecord] ?? (baseRecord ? "Unknown item" : "Empty slot"),
    componentDisplayName: null,
    augmentDisplayName: null,
    baseRecord,
    prefixRecord: "",
    suffixRecord: "",
    modifierRecord: "",
    transmuteRecord: "",
    seed: 1_337,
    componentRecord: "",
    componentBonusRecord: "",
    componentSeed: 0,
    augmentRecord: "",
    ascendantAffixRecord: "",
    ascendantAffixTwoHandedRecord: "",
    augmentSeed: 0,
    componentCombines: 0,
    stackCount: 1,
    ascendantRerolls: 0,
  };
}

export let previewCharacter: CharacterDocument = {
  writeSupported: true,
  writeBlockers: [],
  databaseWarnings: ["Skipped database C:\\Grim Dawn\\database\\database.arz: data file is truncated at position 27"],
  id: "preview-character",
  path: "C:\\Steam\\userdata\\000000\\219990\\remote\\save\\main\\_Arkovian\\player.gdc",
  name: "Arkovian",
  className: "Warlord",
  classTag: "tagSkillClassName0109",
  level: 72,
  male: false,
  hardcore: false,
  expansionCharacter: true,
  source: "steamCloud",
  modifiedAt: Date.now(),
  dataVersion: 8,
  blockCount: 18,
  coreStats: {
    levelInBio: 72,
    experience: 18_450_200,
    attributePoints: 12,
    skillPoints: 4,
    devotionPoints: 3,
    totalDevotionPointsUnlocked: 55,
    physique: 620,
    cunning: 415,
    spirit: 370,
    health: 9_850,
    energy: 2_840,
  },
  iron: 248_730,
  inventoryBagCount: 5,
  factions: [
    { index: 1, name: "Devil's Crossing", changed: true, unlocked: true, value: 12_450, positiveBoost: 0, negativeBoost: 0 },
    { index: 4, name: "Cronley's Gang", changed: true, unlocked: true, value: -8_000, positiveBoost: 0, negativeBoost: 0 },
    { index: 6, name: "Rovers", changed: true, unlocked: true, value: 9_200, positiveBoost: 0, negativeBoost: 0 },
    { index: 7, name: "Homestead", changed: true, unlocked: true, value: 6_400, positiveBoost: 0, negativeBoost: 0 },
    { index: 11, name: "Coven of Ugdenbog", changed: true, unlocked: true, value: 25_000, positiveBoost: 0, negativeBoost: 0 },
  ],
  items: [
    item("inventory:0:0", "inventory", 0, "records/items/gearweapons/axe1h/c001_axe.dbr"),
    { ...item("inventory:0:1", "inventory", 1, "records/items/crafting/materials/craft_ugdenbloom.dbr"), stackCount: 18 },
    item("inventory:0:2", "inventory", 2, "records/items/gearaccessories/rings/c001_ring.dbr"),
    ...Array.from({ length: 12 }, (_, index) => item(
      `equipment:0:${index}`,
      "equipment",
      index,
      index === 0 ? "records/items/gearhead/c001_head.dbr" : index === 2 ? "records/items/geartorso/c001_torso.dbr" : "",
    )),
    item("weapon-set-one:0:0", "weaponSetOne", 0, "records/items/gearweapons/axe2h/c001_axe2h.dbr"),
    item("weapon-set-one:0:1", "weaponSetOne", 1),
    item("weapon-set-two:0:0", "weaponSetTwo", 0),
    item("weapon-set-two:0:1", "weaponSetTwo", 1),
  ],
};

export const previewCatalog: CatalogItem[] = [
  { record: "records/items/gearweapons/axe1h/c001_axe.dbr", name: "Asterkarn Engraved Axe", className: "WeaponMelee_Axe", kind: "base", iconPath: null, levelRequirement: 70, itemLevel: 75 },
  { record: "records/items/gearweapons/swords1h/c001_sword.dbr", name: "Kurn Stormblade", className: "WeaponMelee_Sword", kind: "base", iconPath: null, levelRequirement: 65, itemLevel: 70 },
  { record: "records/items/gearaccessories/rings/c001_ring.dbr", name: "Dreeg Seer's Seal", className: "ArmorJewelry_Ring", kind: "base", iconPath: null, levelRequirement: 72, itemLevel: 75 },
  { record: "records/items/materia/comp_aether_01.dbr", name: "Aether Soul", className: "ItemRelic", kind: "component", iconPath: null, levelRequirement: 15, itemLevel: 20 },
  { record: "records/items/enchantments/augment_aether_01.dbr", name: "Aetherward Oil", className: "ItemEnchantment", kind: "augment", iconPath: null, levelRequirement: 40, itemLevel: 40 },
];

export function updatePreviewCharacter(character: CharacterDocument) {
  previewCharacter = character;
}
