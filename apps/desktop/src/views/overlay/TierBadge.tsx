/** One colour per grade letter, shared by champ select and the augment panels so they never disagree.
 *  Which letter a number earns is decided in the backend (`mayhem_core::ranking`); this only says
 *  what a letter looks like. */
const GRADE_COLOUR: Record<string, string> = {
  S: "#f0a53a",
  A: "#57d38c",
  B: "#4da3e0",
  C: "#9aa4b2",
  D: "#e2685f",
};

/** The colour for a grade or tier letter; grey for `?` and anything unknown. */
export function gradeColour(grade: string): string {
  return GRADE_COLOUR[grade] ?? "#6a7382";
}

/**
 * The tier badge: a hexagon shield with the letter inside, drawn as SVG so it stays crisp at any
 * resolution rather than being a bitmap sized for one.
 *
 * Shared by the augment panels and the champ-select blocks. One symbol and one set of colours across
 * both, so the shape means the same thing wherever it appears.
 */
export function TierBadge({
  tier,
  className = "ov-tier",
  solid = false,
}: {
  tier: string;
  className?: string;
  /** Opaque backing and a stronger tint, for champ select, where the badge sits on client artwork. */
  solid?: boolean;
}) {
  const color = gradeColour(tier);
  // The gradient id has to distinguish colour, letter and variant: two badges sharing an id would
  // share whichever gradient rendered first.
  const grad = `tier-${tier === "?" ? "unknown" : tier}-${color.replace("#", "")}${solid ? "-solid" : ""}`;
  const shape = "M20 1 L38 11 L38 35 L20 45 L2 35 L2 11 Z";
  return (
    <svg className={className} viewBox="0 0 40 46" aria-label={`Tier ${tier}`}>
      <defs>
        <linearGradient id={grad} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor={color} stopOpacity={solid ? "0.5" : "0.38"} />
          <stop offset="100%" stopColor={color} stopOpacity={solid ? "0.2" : "0.08"} />
        </linearGradient>
      </defs>
      {/* The tint alone can't be raised much: the letter is the same colour, so it would wash out.
          An opaque dark base under it is what stops the artwork showing through. */}
      {solid && <path d={shape} fill="rgb(8, 12, 22)" />}
      {/* Pointed top and bottom, flat sides — the shield shape from your sketch. */}
      <path
        d={shape}
        fill={`url(#${grad})`}
        stroke={color}
        strokeWidth="2.5"
        strokeLinejoin="round"
      />
      {/* Placed on an explicit baseline: `dominant-baseline` centres the em box, which leaves a
          capital letter sitting visibly low. The hexagon's centre is y=23 and the cap height of a
          24px glyph is about 17, so the baseline belongs at 23 + 17/2. */}
      <text x="20" y="31.5" textAnchor="middle" fill={color} fontSize="24" fontWeight="700">
        {tier}
      </text>
    </svg>
  );
}
