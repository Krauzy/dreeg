const masteries = {
  "01": { name: "Soldier", color: "#c99a57" },
  "02": { name: "Demolitionist", color: "#e46845" },
  "03": { name: "Occultist", color: "#9e6bca" },
  "04": { name: "Nightblade", color: "#65a9c7" },
  "05": { name: "Arcanist", color: "#6d83da" },
  "06": { name: "Shaman", color: "#77a65a" },
  "07": { name: "Inquisitor", color: "#d4bb62" },
  "08": { name: "Necromancer", color: "#78a89a" },
  "09": { name: "Oathkeeper", color: "#d9844f" },
  "10": { name: "Berserker", color: "#c45562" },
} as const;

type MasteryCode = keyof typeof masteries;

function parseClassTag(classTag: string): MasteryCode[] {
  const match = /tagSkillClassName(\d{2})(\d{2})?$/i.exec(classTag);
  if (!match) return [];
  const primary = match[1] as MasteryCode;
  const secondary = match[2] as MasteryCode;
  if (!masteries[primary]) return [];
  if (!secondary || !masteries[secondary] || secondary === primary) return [primary];
  return [primary, secondary];
}

function MasteryGlyph({ code }: { code: MasteryCode }) {
  switch (code) {
    case "01": return <><path d="M12 3 19 6v5c0 5-3 8-7 10-4-2-7-5-7-10V6Z" /><path d="M12 7v9" /></>;
    case "02": return <path d="M13 2c1 5-3 6-1 10 1-2 3-3 4-5 3 3 4 6 2 10-2 4-8 5-11 1-3-4 0-8 3-11 0 4 2 6 3 7-3-6 1-7 4-13Z" />;
    case "03": return <><path d="M2.5 12s3.5-6 9.5-6 9.5 6 9.5 6-3.5 6-9.5 6-9.5-6-9.5-6Z" /><circle cx="12" cy="12" r="3" /></>;
    case "04": return <><path d="m5 19 5-6-2-2-5 8Z" /><path d="m19 5-5 6 2 2 5-8Z" /><path d="M8 8l8 8" /></>;
    case "05": return <><path d="m12 2 7 7-7 13L5 9Z" /><path d="m5 9 7 3 7-3M12 12v10" /></>;
    case "06": return <><path d="M13 2 5 13h6l-1 9 9-13h-6Z" /><path d="M4 5h3M17 19h3" /></>;
    case "07": return <><circle cx="12" cy="12" r="8" /><path d="M12 3v5m0 8v5M3 12h5m8 0h5" /><circle cx="12" cy="12" r="2" /></>;
    case "08": return <><path d="M5 11a7 7 0 1 1 14 0c0 3-1 5-3 6v4H8v-4c-2-1-3-3-3-6Z" /><circle cx="9" cy="11" r="1.4" /><circle cx="15" cy="11" r="1.4" /><path d="m10 17 2-2 2 2" /></>;
    case "09": return <><circle cx="12" cy="12" r="5" /><path d="M12 1v4m0 14v4M1 12h4m14 0h4M4 4l3 3m10 10 3 3M20 4l-3 3M7 17l-3 3" /></>;
    case "10": return <><path d="M6 20 10 4M11 20l3-17M16 20l3-14" /><path d="M4 17c5 2 10 2 16 0" /></>;
  }
}

function MasteryMark({ code }: { code: MasteryCode }) {
  const mastery = masteries[code];
  return <span className="mastery-mark" style={{ "--mastery-color": mastery.color } as React.CSSProperties} title={mastery.name}>
    <svg viewBox="0 0 24 24" aria-hidden="true"><MasteryGlyph code={code} /></svg>
  </span>;
}

export function ClassIcon({ classTag, className, large = false }: { classTag: string; className: string; large?: boolean }) {
  const codes = parseClassTag(classTag);
  return <span className={`class-icon ${codes.length === 2 ? "dual" : "single"}${large ? " large" : ""}`} role="img" aria-label={`${className || "No class"} class icon`}>
    {codes.length ? codes.map((code) => <MasteryMark code={code} key={code} />) : <span className="class-icon-fallback">◇</span>}
  </span>;
}
