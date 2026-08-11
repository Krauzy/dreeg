import { useEffect, useMemo, useState } from "react";
import { gameDatabaseStatus, loadItemIcons, searchItemCatalog } from "../lib/api";
import { NumericField } from "./NumericField";
import type {
  CatalogItem,
  CatalogKind,
  CharacterItem,
  GameDatabaseInfo,
  NewInventoryItem,
} from "../types";

type NewItemKind = Extract<CatalogKind, "base" | "component" | "augment">;

const equipmentSlots = [
  "Head", "Amulet", "Chest", "Legs", "Feet", "Hands",
  "Ring I", "Ring II", "Waist", "Shoulders", "Medal", "Relic",
];

const emptySelection: CharacterItem = {
  id: "empty-selection", container: "inventory", containerIndex: 0, slotIndex: 0,
  x: 0, y: 0, displayName: "No existing item", baseRecord: "", prefixRecord: "",
  componentDisplayName: null, augmentDisplayName: null,
  suffixRecord: "", modifierRecord: "", transmuteRecord: "", seed: 0,
  componentRecord: "", componentBonusRecord: "", componentSeed: 0, augmentRecord: "",
  ascendantAffixRecord: "", ascendantAffixTwoHandedRecord: "", augmentSeed: 0,
  componentCombines: 0, stackCount: 1, ascendantRerolls: 0,
};

function shortRecord(record: string) {
  if (!record) return "Empty slot";
  return record.split("/").at(-1)?.replace(/\.dbr$/i, "") ?? record;
}

function gridPosition(item: CharacterItem) {
  if (item.x == null || item.y == null) return `Entry ${item.slotIndex + 1}`;
  return `Row ${item.y + 1}, column ${item.x + 1} (X ${item.x}, Y ${item.y})`;
}

function itemLocation(item: CharacterItem) {
  switch (item.container) {
    case "inventory": return `Bag ${item.containerIndex + 1} · ${gridPosition(item)}`;
    case "stash": return `Personal stash ${item.containerIndex + 1} · ${gridPosition(item)}`;
    case "equipment": return equipmentSlots[item.slotIndex] ?? `Equipment ${item.slotIndex + 1}`;
    case "weaponSetOne": return `Weapon set I · ${item.slotIndex === 0 ? "main hand" : "off hand"}`;
    case "weaponSetTwo": return `Weapon set II · ${item.slotIndex === 0 ? "main hand" : "off hand"}`;
  }
}

function RecordField({ label, value }: { label: string; value: string }) {
  return (
    <label className="field">
      <span>{label}</span>
      <input value={value} disabled readOnly spellCheck={false} />
    </label>
  );
}

function kindLabel(kind: NewItemKind) {
  if (kind === "component") return "component";
  if (kind === "augment") return "augment";
  return "item";
}

function ItemArtwork({ record, name, image, kind = "base", large = false }: {
  record: string;
  name: string;
  image: string | null | undefined;
  kind?: NewItemKind;
  large?: boolean;
}) {
  return <span className={`item-artwork ${kind}${large ? " large" : ""}${!record ? " empty" : ""}${record && image === undefined ? " loading" : ""}`} aria-hidden="true">
    {image
      ? <img src={image} alt="" />
      : <svg viewBox="0 0 32 32">
          {kind === "component" && <><path d="m16 3 10 13-10 13L6 16Z" /><path d="m16 8 5 8-5 8-5-8Z" /></>}
          {kind === "augment" && <><path d="M12 3h8v5l5 13c1 4-2 7-6 7h-6c-4 0-7-3-6-7l5-13Z" /><path d="M9 20h14" /></>}
          {kind === "base" && <><path d="m8 25 6-8-3-3-6 11Z" /><path d="m24 7-6 8 3 3 6-11Z" /><path d="M11 9l12 12" /></>}
        </svg>}
    <span className="sr-only">{name}</span>
  </span>;
}

export function ItemEditor({
  items,
  mode,
  inventoryBagCount = 0,
  pendingItems = [],
  onAddItem,
  onRemovePending,
  readOnly = false,
}: {
  items: CharacterItem[];
  mode: "inventory" | "equipment";
  inventoryBagCount?: number;
  pendingItems?: NewInventoryItem[];
  onAddItem?: (item: NewInventoryItem) => void;
  onRemovePending?: (index: number) => void;
  readOnly?: boolean;
}) {
  const visible = useMemo(
    () => items.filter((item) => mode === "inventory"
      ? item.container === "inventory" || item.container === "stash"
      : item.container === "equipment" || item.container.startsWith("weaponSet")),
    [items, mode],
  );
  const [selectedId, setSelectedId] = useState(visible[0]?.id ?? "");
  const [query, setQuery] = useState("");
  const [catalog, setCatalog] = useState<CatalogItem[]>([]);
  const [catalogTotal, setCatalogTotal] = useState(0);
  const [database, setDatabase] = useState<GameDatabaseInfo | null>(null);
  const [catalogLoading, setCatalogLoading] = useState(false);
  const [catalogError, setCatalogError] = useState<string | null>(null);
  const [advanced, setAdvanced] = useState(false);
  const [adding, setAdding] = useState(false);
  const [newItemKind, setNewItemKind] = useState<NewItemKind>("base");
  const [newItem, setNewItem] = useState<NewInventoryItem>({ bagIndex: 0, x: 0, y: 0, baseRecord: "", displayName: "", stackCount: 1 });
  const [newItemName, setNewItemName] = useState("");
  const [icons, setIcons] = useState<Record<string, string | null>>({});
  const [iconError, setIconError] = useState<string | null>(null);
  const selected = visible.find((item) => item.id === selectedId) ?? visible[0] ?? emptySelection;
  const newItemKindLabel = kindLabel(newItemKind);
  const iconRecords = useMemo(() => [...new Set([
    ...visible.flatMap((item) => [item.baseRecord, item.componentRecord, item.augmentRecord]),
    ...catalog.map((item) => item.record),
    ...pendingItems.map((item) => item.baseRecord),
    newItem.baseRecord,
  ].filter(Boolean).map((record) => record.toLocaleLowerCase("en-US")))], [catalog, newItem.baseRecord, pendingItems, visible]);

  useEffect(() => {
    if (!visible.some((item) => item.id === selectedId)) setSelectedId(visible[0]?.id ?? "");
  }, [selectedId, visible]);

  useEffect(() => {
    gameDatabaseStatus().then(setDatabase).catch(() => setDatabase(null));
  }, []);

  useEffect(() => {
    let cancelled = false;
    const chunks = Array.from({ length: Math.ceil(iconRecords.length / 200) }, (_, index) =>
      iconRecords.slice(index * 200, index * 200 + 200));
    Promise.all(chunks.map(loadItemIcons))
      .then((loaded) => {
        if (!cancelled) {
          setIcons((current) => Object.assign({}, current, ...loaded));
          setIconError(null);
        }
      })
      .catch((reason) => {
        if (!cancelled) setIconError(String(reason));
      });
    return () => { cancelled = true; };
  }, [iconRecords]);

  useEffect(() => {
    if (!adding || !query.trim()) {
      setCatalog([]);
      setCatalogTotal(0);
      return;
    }
    let cancelled = false;
    const timeout = window.setTimeout(() => {
      setCatalogLoading(true);
      setCatalogError(null);
      searchItemCatalog(query, newItemKind, 80)
        .then((result) => {
          if (cancelled) return;
          setCatalog(result.items);
          setCatalogTotal(result.total);
          setDatabase(result.database);
        })
        .catch((reason) => {
          if (!cancelled) setCatalogError(String(reason));
        })
        .finally(() => {
          if (!cancelled) setCatalogLoading(false);
        });
    }, 250);
    return () => {
      cancelled = true;
      window.clearTimeout(timeout);
    };
  }, [adding, newItemKind, query]);

  function chooseCatalogItem(candidate: CatalogItem) {
    if (readOnly || !adding) return;
    setNewItem((current) => ({ ...current, baseRecord: candidate.record, displayName: candidate.name }));
    setNewItemName(candidate.name);
    setQuery("");
    setCatalog([]);
  }

  function changeNewItemKind(kind: NewItemKind) {
    setNewItemKind(kind);
    setNewItem((current) => ({ ...current, baseRecord: "", displayName: "", stackCount: 1 }));
    setNewItemName("");
    setQuery("");
    setCatalog([]);
    setCatalogTotal(0);
  }

  function queueNewItem() {
    if (readOnly || !onAddItem || !newItem.baseRecord) return;
    onAddItem(newItem);
    setNewItem({ bagIndex: newItem.bagIndex, x: 0, y: 0, baseRecord: "", displayName: "", stackCount: 1 });
    setNewItemName("");
    setQuery("");
    setAdding(false);
  }

  function toggleAdding() {
    setAdding((current) => !current);
    setQuery("");
    setCatalog([]);
    setCatalogError(null);
  }

  function changeQuery(value: string) {
    setQuery(value);
    setNewItem((current) => ({ ...current, baseRecord: "", displayName: "" }));
    setNewItemName("");
  }

  const grimToolsQuery = encodeURIComponent(adding
    ? newItemName || query
    : selected.displayName || shortRecord(selected.baseRecord));
  const iconFor = (record: string) => icons[record.toLocaleLowerCase("en-US")];

  return (
    <div className="items-layout">
      <section className="panel item-list-panel">
        <div className="panel-heading">
          <div><p className="eyebrow">SAVE CONTENTS</p><h2>{mode === "inventory" ? "Inventory and stash" : "Equipped slots"}</h2></div>
          <div className="panel-actions">
            <span className="version-chip">{visible.length} slots</span>
            {mode === "inventory" && <button className="add-item-button" disabled={readOnly} onClick={toggleAdding}>{adding ? "Cancel" : "+ Add item"}</button>}
          </div>
        </div>
        {iconError && <div className="inline-warning" role="status">Item artwork is unavailable: {iconError} Names and save data remain available.</div>}
        <div className="item-list">
          {visible.map((item) => (
            <button key={item.id} className={item.id === selected.id ? "item-row active" : "item-row"} onClick={() => setSelectedId(item.id)}>
              <ItemArtwork record={item.baseRecord} name={item.displayName} image={iconFor(item.baseRecord)} />
              <span><strong>{item.displayName}</strong><small>{itemLocation(item)}{item.stackCount > 1 ? ` · x${item.stackCount}` : ""}</small></span>
            </button>
          ))}
        </div>
        {pendingItems.length > 0 && <div className="pending-items"><p className="eyebrow">PENDING ADDITIONS</p>{pendingItems.map((item, index) => <div key={`${item.baseRecord}-${index}`}><ItemArtwork record={item.baseRecord} name={item.displayName} image={iconFor(item.baseRecord)} /><span><strong>{item.displayName || previewName(item.baseRecord)}</strong><small>Bag {item.bagIndex + 1} · row {item.y + 1}, column {item.x + 1}</small></span><button aria-label="Remove pending item" onClick={() => onRemovePending?.(index)}>×</button></div>)}</div>}
      </section>

      <section className="panel item-detail-panel">
        <p className="eyebrow">{adding ? "NEW INVENTORY ITEM" : "ITEM DETAILS · READ-ONLY"}</p>
        <h2>{adding ? "Add an inventory item" : visible.length ? itemLocation(selected) : "No existing items"}</h2>

        {adding && <>
          <label className="field item-type-selector">
            <span>Item type</span>
            <select value={newItemKind} onChange={(event) => changeNewItemKind(event.currentTarget.value as NewItemKind)}>
              <option value="base">Item</option>
              <option value="component">Component</option>
              <option value="augment">Augment</option>
            </select>
          </label>
          <label className="field catalog-search">
            <span>Search the local {newItemKindLabel} database</span>
            <input placeholder={`Type a ${newItemKindLabel} name…`} value={query} onChange={(event) => changeQuery(event.currentTarget.value)} />
          </label>
          <div className="database-line">
            {catalogLoading ? "Indexing and searching…" : database
              ? `${database.databaseFiles.length} local databases · ${catalogTotal || `ready to search ${newItemKindLabel}s`}`
              : "Grim Dawn database not found"}
            <a href={`https://www.grimtools.com/db/search?query=${grimToolsQuery}`} target="_blank" rel="noreferrer">Open in GrimTools ↗</a>
          </div>
          {catalogError && <div className="inline-error">{catalogError}</div>}
        </>}

        {adding && catalog.length > 0 && (
          <div className="catalog-results">
            {catalog.map((candidate) => (
              <button key={candidate.record} onClick={() => chooseCatalogItem(candidate)}>
                <ItemArtwork record={candidate.record} name={candidate.name} image={iconFor(candidate.record)} kind={candidate.kind === "component" || candidate.kind === "augment" ? candidate.kind : "base"} />
                <span><strong>{candidate.name}</strong><small>{candidate.className} · {candidate.record}</small></span>
                <em>{candidate.levelRequirement ? `Lv. ${candidate.levelRequirement}` : "Select"}</em>
              </button>
            ))}
          </div>
        )}

        {adding && <div className="new-item-form">
          <div className="selected-catalog-item"><ItemArtwork record={newItem.baseRecord} name={newItemName} image={iconFor(newItem.baseRecord)} kind={newItemKind} large /><div><small>SELECTED {newItemKind.toUpperCase()}</small><strong>{newItemName || `Choose a ${newItemKindLabel} from the search results`}</strong></div></div>
          <div className="field-grid new-item-fields">
            <label className="field"><span>Target bag</span><select value={newItem.bagIndex} onChange={(event) => {
              const bagIndex = Number(event.currentTarget.value);
              setNewItem((current) => ({ ...current, bagIndex }));
            }}>{Array.from({ length: inventoryBagCount }, (_, index) => <option value={index} key={index}>Bag {index + 1}</option>)}</select></label>
            <NumericField compact label="Column" value={newItem.x} min={0} max={255} onChange={(x) => setNewItem((current) => ({ ...current, x }))} />
            <NumericField compact label="Row" value={newItem.y} min={0} max={255} onChange={(y) => setNewItem((current) => ({ ...current, y }))} />
            <NumericField compact label="Quantity" value={newItem.stackCount} min={1} max={1_000_000} onChange={(stackCount) => setNewItem((current) => ({ ...current, stackCount }))} />
          </div>
          <button className="primary-button queue-item" disabled={readOnly || !newItem.baseRecord || inventoryBagCount === 0} onClick={queueNewItem}>Queue {newItemKindLabel} for save</button>
        </div>}

        {!adding && visible.length > 0 && <>
          <div className="readonly-item-note">Existing inventory and equipped items are protected. You can inspect them, but only newly added inventory items are written to the save.</div>
          <div className="item-visual-summary">
            <ItemArtwork record={selected.baseRecord} name={selected.displayName} image={iconFor(selected.baseRecord)} large />
            <div><small>SAVED ITEM</small><strong>{selected.displayName}</strong><span>{itemLocation(selected)}</span></div>
          </div>
          {(selected.componentRecord || selected.augmentRecord) && <div className="item-attachments">
            {selected.componentRecord && <div><ItemArtwork record={selected.componentRecord} name={selected.componentDisplayName ?? "Component"} image={iconFor(selected.componentRecord)} kind="component" /><span><small>COMPONENT</small><strong>{selected.componentDisplayName ?? shortRecord(selected.componentRecord)}</strong></span></div>}
            {selected.augmentRecord && <div><ItemArtwork record={selected.augmentRecord} name={selected.augmentDisplayName ?? "Augment"} image={iconFor(selected.augmentRecord)} kind="augment" /><span><small>AUGMENT</small><strong>{selected.augmentDisplayName ?? shortRecord(selected.augmentRecord)}</strong></span></div>}
          </div>}
          <div className="field-grid item-primary-fields">
            <RecordField label="Base record (.dbr)" value={selected.baseRecord} />
            <NumericField disabled compact label="Quantity" min={0} max={1_000_000} value={selected.stackCount} onChange={() => undefined} />
          </div>
          <div className="database-line existing-item-link"><span>Saved item data</span><a href={`https://www.grimtools.com/db/search?query=${grimToolsQuery}`} target="_blank" rel="noreferrer">Open in GrimTools ↗</a></div>
          <button className="advanced-toggle" onClick={() => setAdvanced((value) => !value)}>
            {advanced ? "Hide saved fields" : "View affixes, component and augment"}
          </button>
          {advanced && (
            <div className="field-grid advanced-item-fields">
              <RecordField label="Prefix" value={selected.prefixRecord} />
              <RecordField label="Suffix" value={selected.suffixRecord} />
              <RecordField label="Modifier" value={selected.modifierRecord} />
              <RecordField label="Transmute" value={selected.transmuteRecord} />
              <RecordField label="Component" value={selected.componentRecord} />
              <RecordField label="Component bonus" value={selected.componentBonusRecord} />
              <RecordField label="Augment" value={selected.augmentRecord} />
              <RecordField label="Ascendant affix" value={selected.ascendantAffixRecord} />
              <RecordField label="Ascendant (two-handed)" value={selected.ascendantAffixTwoHandedRecord} />
            </div>
          )}
        </>}
        {!adding && visible.length === 0 && <div className="empty-inventory"><p>This inventory has no existing entries.</p>{mode === "inventory" && <button className="add-item-button" disabled={readOnly} onClick={toggleAdding}>+ Add the first item</button>}</div>}
      </section>
    </div>
  );
}

function previewName(record: string) {
  return record.split("/").at(-1)?.replace(/\.dbr$/i, "").replaceAll("_", " ") ?? record;
}
