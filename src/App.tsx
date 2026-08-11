import { useEffect, useMemo, useState } from "react";
import { desktopAvailable, loadCharacter, restoreLatestBackup, saveCharacter, scanCharacters } from "./lib/api";
import { ItemEditor } from "./components/ItemEditor";
import { NumericField } from "./components/NumericField";
import { ClassIcon } from "./components/ClassIcon";
import type {
  CharacterItem,
  CharacterDocument,
  CharacterPatch,
  CharacterSummary,
  CoreStats,
  FactionValue,
  NewInventoryItem,
  SaveSource,
} from "./types";

const sourceLabels: Record<SaveSource, string> = {
  local: "Local",
  steamCloud: "Steam Cloud",
  custom: "Custom",
  unknown: "Unknown",
};

const navItems = [
  ["overview", "Overview"],
  ["attributes", "Attributes"],
  ["inventory", "Inventory"],
  ["equipment", "Equipment"],
  ["factions", "Factions"],
] as const;

function formatModified(value: number | null) {
  if (!value) return "unavailable";
  return new Intl.DateTimeFormat("en-US", {
    dateStyle: "short",
    timeStyle: "short",
  }).format(new Date(value));
}

function EmptyState({ scanning, error }: { scanning: boolean; error: string | null }) {
  return (
    <main className="empty-state">
      <div className="sigil" aria-hidden="true"><span /></div>
      <p className="eyebrow">CAIRN ARCHIVES</p>
      <h1>{scanning ? "Scanning for characters…" : "No characters found"}</h1>
      <p>
        {error ??
          "Dreeg automatically scans the default Grim Dawn local and Steam Cloud save folders."}
      </p>
      {!desktopAvailable && (
        <div className="notice">Save discovery is available in the desktop application.</div>
      )}
    </main>
  );
}

function reputationTier(value: number) {
  if (value <= -20_000) return "Nemesis";
  if (value <= -8_000) return "Hated";
  if (value <= -1_500) return "Despised";
  if (value < 0) return "Hostile";
  if (value >= 25_000) return "Revered";
  if (value >= 10_000) return "Honored";
  if (value >= 5_000) return "Respected";
  if (value >= 1_500) return "Friendly";
  return "Tolerated";
}

function Editor({
  character,
  onSaved,
}: {
  character: CharacterDocument;
  onSaved: (result: CharacterDocument) => void;
}) {
  const [section, setSection] = useState<(typeof navItems)[number][0]>("overview");
  const [name, setName] = useState(character.name);
  const [level, setLevel] = useState(character.level);
  const [hardcore, setHardcore] = useState(character.hardcore);
  const [iron, setIron] = useState(character.iron ?? 0);
  const [stats, setStats] = useState<CoreStats | null>(character.coreStats);
  const [items, setItems] = useState<CharacterItem[]>(character.items);
  const [newItems, setNewItems] = useState<NewInventoryItem[]>([]);
  const [factions, setFactions] = useState<FactionValue[]>(character.factions);
  const [saving, setSaving] = useState(false);
  const [restoring, setRestoring] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  function resetDraft(source: CharacterDocument) {
    setName(source.name);
    setLevel(source.level);
    setHardcore(source.hardcore);
    setIron(source.iron ?? 0);
    setStats(source.coreStats);
    setItems(source.items);
    setNewItems([]);
    setFactions(source.factions);
  }

  useEffect(() => {
    resetDraft(character);
    setSection("overview");
    setMessage(null);
  }, [character.id]);

  const dirty =
    name !== character.name ||
    level !== character.level ||
    hardcore !== character.hardcore ||
    iron !== (character.iron ?? 0) ||
    JSON.stringify(stats) !== JSON.stringify(character.coreStats) ||
    newItems.length > 0 ||
    JSON.stringify(factions) !== JSON.stringify(character.factions);

  function updateStat<K extends keyof CoreStats>(key: K, value: CoreStats[K]) {
    setStats((current) => (current ? { ...current, [key]: value } : current));
  }

  async function persist() {
    const patch: CharacterPatch = {
      characterName: name.trim(),
      characterLevel: level,
      hardcore,
      iron,
      coreStats: stats,
      items: [],
      newItems,
      factions: factions
        .filter((faction) => faction.value !== character.factions.find((original) => original.index === faction.index)?.value)
        .map(({ index, value }) => ({ index, value })),
    };
    setSaving(true);
    setMessage(null);
    try {
      const result = await saveCharacter(character.id, patch);
      onSaved(result.character);
      resetDraft(result.character);
      setMessage(`Saved safely. Backup: ${result.backupPath}`);
    } catch (reason) {
      setMessage(String(reason));
    } finally {
      setSaving(false);
    }
  }

  function discardChanges() {
    if (!dirty || !window.confirm("Discard every unsaved change for this character?")) return;
    resetDraft(character);
    setMessage("All unsaved changes were discarded.");
  }

  async function restoreBackup() {
    const confirmed = window.confirm(
      "Restore the latest backup for this character? The current save will be backed up first.",
    );
    if (!confirmed) return;
    setRestoring(true);
    setMessage(null);
    try {
      const result = await restoreLatestBackup(character.id);
      onSaved(result.character);
      resetDraft(result.character);
      setMessage(`Backup restored safely. Previous save backed up at: ${result.safetyBackupPath}`);
    } catch (reason) {
      setMessage(String(reason));
    } finally {
      setRestoring(false);
    }
  }

  return (
    <main className="editor-shell">
      <header className="editor-header">
        <div>
          <p className="eyebrow">SELECTED CHARACTER</p>
          <div className="title-row">
            <ClassIcon classTag={character.classTag} className={character.className} large />
            <h1>{character.name}</h1>
            {character.hardcore && <span className="danger-chip">HARDCORE</span>}
          </div>
          <p className="character-meta">
            {character.className || "No class"} · Level {character.level} · {sourceLabels[character.source]}
          </p>
        </div>
        <div className="header-actions">
          <span className={dirty ? "change-state dirty" : "change-state"}>
            {dirty ? "Unsaved changes" : "No changes"}
          </span>
          <button className="secondary-button" disabled={!dirty || saving || restoring} onClick={discardChanges}>
            Discard changes
          </button>
          <button className="secondary-button restore-button" disabled={saving || restoring} onClick={restoreBackup}>
            {restoring ? "Restoring…" : "Restore backup"}
          </button>
          <button className="primary-button" disabled={!character.writeSupported || !dirty || saving || restoring} onClick={persist} title={character.writeSupported ? undefined : "Writing is disabled for this save format"}>
            {saving ? "Validating…" : "Save character"}
          </button>
        </div>
      </header>

      <nav className="section-nav" aria-label="Character sections">
        {navItems.map(([id, label]) => (
          <button
            key={id}
            className={section === id ? "active" : ""}
            onClick={() => setSection(id)}
          >
            {label}
          </button>
        ))}
      </nav>

      {message && <div className="status-message">{message}</div>}

      {!character.writeSupported && (
        <div className="safety-lock" role="alert">
          <div><strong>Read-only safety mode</strong><span>Dreeg detected encrypted blocks whose Grim Dawn 1.3 layout is not fully mapped. Editing is disabled so this character cannot be corrupted.</span></div>
          <small>Blocked sections: {character.writeBlockers.join(", ")} · Your save has not been changed.</small>
        </div>
      )}

      {character.databaseWarnings.length > 0 && (
        <div className="database-warning" role="alert">
          <div><strong>Grim Dawn database needs attention</strong><span>Dreeg loaded the remaining valid game databases, but some names or artwork may be unavailable. Verify the installed game files in Steam.</span></div>
          {character.databaseWarnings.map((warning) => <small key={warning}>{warning}</small>)}
        </div>
      )}

      {section === "overview" && (
        <div className="editor-grid">
          <section className="panel span-two">
            <div className="panel-heading">
              <div><p className="eyebrow">IDENTITY</p><h2>Basic information</h2></div>
              <span className="version-chip">GDC v{character.dataVersion}</span>
            </div>
            <div className="field-grid three">
              <label className="field">
                <span>Name</span>
                <input value={name} maxLength={32} disabled title="Renaming requires a separate safe folder-move workflow" onChange={(event) => setName(event.target.value)} />
              </label>
              <NumericField disabled={!character.writeSupported} label="Level" value={level} onChange={(value) => {
                setLevel(value);
                setStats((current) => current ? { ...current, levelInBio: value } : current);
              }} />
              <NumericField disabled={!character.writeSupported} label="Iron" value={iron} onChange={setIron} />
              <label className="toggle-field">
                  <span><strong>Hardcore mode</strong><small>Dead hardcore characters cannot be loaded.</small></span>
                <input type="checkbox" disabled={!character.writeSupported} checked={hardcore} onChange={(event) => setHardcore(event.target.checked)} />
              </label>
            </div>
          </section>

          <section className="panel">
            <p className="eyebrow">FILE</p>
            <h2>Save source</h2>
            <dl className="details-list">
              <div><dt>Source</dt><dd>{sourceLabels[character.source]}</dd></div>
              <div><dt>Modified</dt><dd>{formatModified(character.modifiedAt)}</dd></div>
              <div><dt>Blocks</dt><dd>{character.blockCount}</dd></div>
            </dl>
            <p className="path-text" title={character.path}>{character.path}</p>
          </section>

          <section className="panel safety-panel">
            <p className="eyebrow">ACTIVE PROTECTION</p>
            <h2>Safe writing</h2>
            <ul>
              <li>Automatic backup before every save</li>
              <li>Block-by-block checksum validation</li>
              <li>Unsafe typed blocks stop the write before backup or replacement</li>
            </ul>
          </section>
        </div>
      )}

      {section === "attributes" && stats && (
        <div className="editor-grid">
          <section className="panel span-two">
            <p className="eyebrow">PROGRESSION</p>
            <h2>Level, currency and available points</h2>
            <div className="field-grid three">
              <NumericField disabled={!character.writeSupported} label="Internal level" value={stats.levelInBio} onChange={(v) => updateStat("levelInBio", v)} />
              <NumericField disabled={!character.writeSupported} label="Experience" value={stats.experience} onChange={(v) => updateStat("experience", v)} />
              <NumericField disabled={!character.writeSupported} label="Iron" value={iron} onChange={setIron} />
              <NumericField disabled={!character.writeSupported} label="Attribute points" value={stats.attributePoints} onChange={(v) => updateStat("attributePoints", v)} />
              <NumericField disabled={!character.writeSupported} label="Skill points" value={stats.skillPoints} onChange={(v) => updateStat("skillPoints", v)} />
              <NumericField disabled={!character.writeSupported} label="Devotion points" value={stats.devotionPoints} onChange={(v) => updateStat("devotionPoints", v)} />
              <NumericField disabled={!character.writeSupported} label="Unlocked devotion" value={stats.totalDevotionPointsUnlocked} onChange={(v) => updateStat("totalDevotionPointsUnlocked", v)} />
            </div>
          </section>
          <section className="panel span-two">
            <p className="eyebrow">ESSENCE</p>
            <h2>Base attributes</h2>
            <div className="field-grid five">
              <NumericField disabled={!character.writeSupported} label="Physique" value={stats.physique} step={0.01} onChange={(v) => updateStat("physique", v)} />
              <NumericField disabled={!character.writeSupported} label="Cunning" value={stats.cunning} step={0.01} onChange={(v) => updateStat("cunning", v)} />
              <NumericField disabled={!character.writeSupported} label="Spirit" value={stats.spirit} step={0.01} onChange={(v) => updateStat("spirit", v)} />
              <NumericField disabled={!character.writeSupported} label="Health" value={stats.health} step={0.01} onChange={(v) => updateStat("health", v)} />
              <NumericField disabled={!character.writeSupported} label="Energy" value={stats.energy} step={0.01} onChange={(v) => updateStat("energy", v)} />
            </div>
          </section>
        </div>
      )}

      {section === "attributes" && !stats && (
        <div className="status-message">This save does not contain a recognized attribute block.</div>
      )}

      {section === "inventory" && <ItemEditor
        items={items}
        mode="inventory"
        inventoryBagCount={character.inventoryBagCount}
        pendingItems={newItems}
        onAddItem={(item) => setNewItems((current) => [...current, item])}
        onRemovePending={(index) => setNewItems((current) => current.filter((_, candidate) => candidate !== index))}
        readOnly={!character.writeSupported}
      />}

      {section === "equipment" && <ItemEditor items={items} mode="equipment" readOnly={!character.writeSupported} />}

      {section === "factions" && (
        <div className="faction-layout">
          {factions.map((faction) => {
            const percent = ((faction.value + 20_000) / 45_000) * 100;
            return <article className="panel faction-card" key={faction.index}>
              <div className="faction-title"><div><p className="eyebrow">FACTION {faction.index}</p><h2>{faction.name}</h2></div><span className={faction.value < 0 ? "tier hostile" : "tier"}>{faction.unlocked ? reputationTier(faction.value) : `Locked · ${reputationTier(faction.value)}`}</span></div>
              <div className="reputation-track"><span style={{ width: `${Math.max(0, Math.min(100, percent))}%` }} /></div>
              <NumericField disabled={!character.writeSupported} label="Reputation" value={faction.value} min={-20_000} max={25_000} step={250} onChange={(value) => setFactions((current) => current.map((entry) => entry.index === faction.index ? { ...entry, value } : entry))} />
              <div className="faction-meta"><span>Positive boost {faction.positiveBoost.toFixed(2)}</span><span>Negative boost {faction.negativeBoost.toFixed(2)}</span></div>
            </article>;
          })}
        </div>
      )}

    </main>
  );
}

export function App() {
  const [characters, setCharacters] = useState<CharacterSummary[]>([]);
  const [selected, setSelected] = useState<CharacterDocument | null>(null);
  const [query, setQuery] = useState("");
  const [scanning, setScanning] = useState(true);
  const [loadingId, setLoadingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    scanCharacters()
      .then(setCharacters)
      .catch((reason) => setError(String(reason)))
      .finally(() => setScanning(false));
  }, []);

  const visibleCharacters = useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase("en-US");
    if (!normalized) return characters;
    return characters.filter((character) =>
      `${character.name} ${character.className}`.toLocaleLowerCase("en-US").includes(normalized),
    );
  }, [characters, query]);

  async function selectCharacter(character: CharacterSummary) {
    setLoadingId(character.id);
    setError(null);
    try {
      setSelected(await loadCharacter(character.id));
    } catch (reason) {
      setError(String(reason));
    } finally {
      setLoadingId(null);
    }
  }

  function handleSaved(updated: CharacterDocument) {
    setSelected(updated);
    setCharacters((current) => current.map((item) => (item.id === updated.id ? updated : item)));
  }

  return (
    <div className="app-frame">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-mark" aria-hidden="true">D</div>
          <div><strong>Dreeg</strong><span>Save editor</span></div>
        </div>
        <label className="search-box">
          <span aria-hidden="true">⌕</span>
          <input
            aria-label="Search characters"
            placeholder="Search character"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
        </label>
        <div className="sidebar-caption">
          <span>CHARACTERS</span><span>{visibleCharacters.length}</span>
        </div>
        <div className="character-list">
          {visibleCharacters.map((character) => (
            <button
              key={character.id}
              className={selected?.id === character.id ? "character-card selected" : "character-card"}
              disabled={loadingId === character.id}
              onClick={() => selectCharacter(character)}
            >
              <ClassIcon classTag={character.classTag} className={character.className} />
              <span className="character-card-copy">
                <strong>{character.name}</strong>
                <small>{loadingId === character.id ? "Opening…" : `${character.className || "No class"} · Lv. ${character.level}`}</small>
              </span>
              {character.hardcore && <span className="hardcore-dot" title="Hardcore" />}
            </button>
          ))}
        </div>
        <div className="sidebar-footer">
          <span>Default folders scanned</span>
          <span>Local only · no server</span>
        </div>
      </aside>
      {selected ? <Editor character={selected} onSaved={handleSaved} /> : <EmptyState scanning={scanning} error={error} />}
    </div>
  );
}
