// M7-F4 (t_c3367dfb): Rarity-Farbskala an EINEM Ort — der Mining-Mode
// (Filter-Badges, Ort-Detail-Kacheln) und die Pin-Chips (Content-Typ
// „scan-signatures") teilen sich dieselbe Rampe, damit die Farbcodierung
// DE/EN und Mode/Pin identisch lesbar ist (Konsistenz-Regel der Spec).
// Live-Contract (M7-F1): ores[].tier ist String-Rarity — kein Strata-
// Vokabular hier, die Werte sind die Roh-Werte der API.
export type Rarity = 'common' | 'uncommon' | 'rare' | 'epic' | 'legendary';

export const RARITIES: Rarity[] = ['common', 'uncommon', 'rare', 'epic', 'legendary'];

/// Kachel/Chip-Rampe (Spec P2): common grau → uncommon hellgrün-grau →
/// rare cyan → epic violett → legendary gold (Overlay-Theme-Töne).
export const RARITY_COLORS: Record<Rarity, string> = {
	common: '#8b949e',
	uncommon: '#a8a23e',
	rare: '#4db8c9',
	epic: '#b48be8',
	legendary: '#e2c14e'
};

/// Farbe zu einem Rarity-String oder null (fehlende Daten — Caller
/// entscheidet den Fallback, z. B. var(--lineb)).
export function rarityColor(rarity: string | null | undefined): string | null {
	if (!rarity) return null;
	return (RARITY_COLORS as Record<string, string>)[rarity] ?? null;
}
