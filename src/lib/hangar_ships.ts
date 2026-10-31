// Static Ship-/Vehicle-Data-Paket for the Hangar-Autocomplete (P10, Karte
// t_aa6cc070 / feature/hangar-ux).
//
// Data source decision (Card: "pruef what is available, else static package"):
// Neither UEX (api.uexcorp.uk/2.0 — verified live: only commodities/items/
// terminals endpoints, no ship route) nor the Wiki ships endpoints
// (api.star-citizen.wiki/api/ships + variants verified 404 live) offer a
// ship list. Hence a manually curated static package, checked into the repo.
// Manufacturer is the commonly used studio name; empty string = unknown/not
// curated (the field then stays empty and the user fills it).
//
// UH (t_424660aa): Konzept-Schiffe ergänzt aus der SC-Wiki SHIPMATRIX
// (api.star-citizen.wiki/api/shipmatrix/vehicles — DIESE Route existiert und
// liefert ALLE 265 vehicles inkl. 31 in-concept, paginiert Seite 1-9;
// production_status 'in-concept' → concept: true). Kuratierte Bestands-
// einträge gehen vor (Shipmatrix füllt nur Lücken).
//
// Snub detection: the hangar schema has no ship_category column, so we use
// the FIXED list from the Star Citizen Wiki category "Snub ships"
// (starcitizen.tools/Category:Snub_ships, 14 entries, fetched live on
// 2026-10-25). Snub = parasite craft that needs a base ship (85X, Fury,
// MPUV, …) — hence those show the Basisschiff field even with
// entity_type = ship/vehicle.

export type ShipDatum = {
	name: string;
	manufacturer: string;
	kind: 'ship' | 'vehicle';
	snub?: boolean;
	// UH (t_424660aa): Shipmatrix production_status 'in-concept' → dezenter
	// 'Konzept'-Chip im Hangar-Autocomplete (RoutesTypeahead badge).
	concept?: boolean;
};

// Canonical Snub list (Wiki category "Snub ships" — keep in sync, single
// source for both the catalog flags and isSnubShip()).
export const SNUB_SHIP_NAMES: string[] = [
	'85X',
	'Cleaver',
	'Fury',
	'Fury LX',
	'Fury MX',
	'HoverQuad',
	'MPUV Cargo',
	'MPUV Personnel',
	'MPUV Tractor',
	'P-52 Merlin',
	'P-72 Archimedes',
	'P-72 Archimedes Emerald',
	'Pitbull',
	'Skylark',
];

export const SHIP_CATALOG: ShipDatum[] = [
// v0.3.2 (t_aa6cc070): generated live from https://citizenhq.space/api/v1/ships
// (2026-10-25, 292 ships -> 240 curated entries; Wikelo/Teach's specials and
// duplicate pledge-paint variants filtered). Manufacturer short-names mapped
// from the API manufacturerCode. kind=vehicle for Tumbril/Greycat/Arca-MPUV
// + hoverbikes (Nox/Pulse/Razor/HoverQuad).
	// --- Aegis ---
	{ name: 'Avenger Stalker', manufacturer: 'Aegis', kind: 'ship' },
	{ name: 'Avenger Titan', manufacturer: 'Aegis', kind: 'ship' },
	{ name: 'Avenger Titan Renegade', manufacturer: 'Aegis', kind: 'ship' },
	{ name: 'Avenger Warlock', manufacturer: 'Aegis', kind: 'ship' },
	{ name: 'Eclipse', manufacturer: 'Aegis', kind: 'ship' },
	{ name: 'Gladius', manufacturer: 'Aegis', kind: 'ship' },
	{ name: 'Gladius Dunlevy', manufacturer: 'Aegis', kind: 'ship' },
	{ name: 'Gladius Pirate', manufacturer: 'Aegis', kind: 'ship' },
	{ name: 'Gladius Valiant', manufacturer: 'Aegis', kind: 'ship' },
	{ name: 'Hammerhead', manufacturer: 'Aegis', kind: 'ship' },
	{ name: 'Hammerhead 2949 Best In Show Edition', manufacturer: 'Aegis', kind: 'ship' },
	{ name: 'Idris-M', manufacturer: 'Aegis', kind: 'ship' },
	{ name: 'Idris-P', manufacturer: 'Aegis', kind: 'ship' },
	{ name: 'Javelin', manufacturer: 'Aegis', kind: 'ship' },
	{ name: 'Reclaimer', manufacturer: 'Aegis', kind: 'ship' },
	{ name: 'Reclaimer 2949 Best In Show Edition', manufacturer: 'Aegis', kind: 'ship' },
	{ name: 'Redeemer', manufacturer: 'Aegis', kind: 'ship' },
	{ name: 'Retaliator', manufacturer: 'Aegis', kind: 'ship' },
	{ name: 'Sabre', manufacturer: 'Aegis', kind: 'ship' },
	{ name: 'Sabre Comet', manufacturer: 'Aegis', kind: 'ship' },
	{ name: 'Sabre Firebird', manufacturer: 'Aegis', kind: 'ship' },
	{ name: 'Sabre Peregrine', manufacturer: 'Aegis', kind: 'ship' },
	{ name: 'Sabre Raven', manufacturer: 'Aegis', kind: 'ship' },
	{ name: 'Sabre Raven EX', manufacturer: 'Aegis', kind: 'ship' },
	{ name: 'Tiburon', manufacturer: 'Aegis', kind: 'ship' },
	{ name: 'Vanguard Harbinger', manufacturer: 'Aegis', kind: 'ship' },
	{ name: 'Vanguard Hoplite', manufacturer: 'Aegis', kind: 'ship' },
	{ name: 'Vanguard Sentinel', manufacturer: 'Aegis', kind: 'ship' },
	{ name: 'Vanguard Warden', manufacturer: 'Aegis', kind: 'ship' },
	// --- Anvil ---
	{ name: 'Arrow', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'Asgard', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'Ballista', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'Ballista Dunestalker', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'Ballista Snowblind', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'C8 Pisces', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'C8R Pisces Rescue', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'C8X Pisces Expedition', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'Carrack', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'Carrack Expedition', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'Centurion', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'F7A Hornet Mk I', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'F7A Hornet Mk II', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'F7C Hornet Mk I', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'F7C Hornet Mk II', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'F7C Hornet Wildfire Mk I', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'F7C-M Hornet Heartseeker Mk I', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'F7C-M Hornet Heartseeker Mk II', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'F7C-M Super Hornet Mk I', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'F7C-M Super Hornet Mk II', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'F7C-R Hornet Tracker Mk I', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'F7C-R Hornet Tracker Mk II', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'F7C-S Hornet Ghost Mk I', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'F7C-S Hornet Ghost Mk II', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'F8A Lightning', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'F8C Lightning', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'F8C Lightning Executive Edition', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'Gladiator', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'Hawk', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'Hurricane', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'Paladin', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'Spartan', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'Terrapin', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'Terrapin Medic', manufacturer: 'Anvil', kind: 'ship' },
	{ name: 'Valkyrie', manufacturer: 'Anvil', kind: 'ship' },
	// --- Aopoa ---
	{ name: 'Khartu-al', manufacturer: 'Aopoa', kind: 'ship' },
	{ name: 'Nox', manufacturer: 'Aopoa', kind: 'vehicle' },
	{ name: 'Nox Kue', manufacturer: 'Aopoa', kind: 'vehicle' },
	{ name: 'San\'tok.yāi', manufacturer: 'Aopoa', kind: 'ship' },
	// --- Argo ---
	{ name: 'ARGO MOLE Alliance', manufacturer: 'Argo', kind: 'ship' },
	{ name: 'ATLS', manufacturer: 'Argo', kind: 'vehicle' },
	{ name: 'ATLS GEO', manufacturer: 'Argo', kind: 'vehicle' },
	{ name: 'ATLS GEO IKTI', manufacturer: 'Argo', kind: 'ship' },
	{ name: 'ATLS IKTI', manufacturer: 'Argo', kind: 'ship' },
	{ name: 'ATLS IKTI Rad', manufacturer: 'Argo', kind: 'ship' },
	{ name: 'CSV-SM', manufacturer: 'Argo', kind: 'ship' },
	{ name: 'MOLE', manufacturer: 'Argo', kind: 'ship' },
	{ name: 'MOTH', manufacturer: 'Argo', kind: 'ship' },
	{ name: 'MPUV Cargo', manufacturer: 'Argo', kind: 'vehicle', snub: true },
	{ name: 'MPUV Personnel', manufacturer: 'Argo', kind: 'vehicle', snub: true },
	{ name: 'MPUV Tractor', manufacturer: 'Argo', kind: 'vehicle', snub: true },
	{ name: 'RAFT', manufacturer: 'Argo', kind: 'ship' },
	{ name: 'SRV', manufacturer: 'Argo', kind: 'ship' },
	// --- Banu ---
	{ name: 'Banu Defender', manufacturer: 'Banu', kind: 'ship' },
	// --- Consolidated Outland ---
	{ name: 'HoverQuad', manufacturer: 'Consolidated Outland', kind: 'vehicle', snub: true },
	{ name: 'Mustang Alpha', manufacturer: 'Consolidated Outland', kind: 'ship' },
	{ name: 'Mustang Beta', manufacturer: 'Consolidated Outland', kind: 'ship' },
	{ name: 'Mustang CitizenCon 2948 Edition', manufacturer: 'Consolidated Outland', kind: 'ship' },
	{ name: 'Mustang Delta', manufacturer: 'Consolidated Outland', kind: 'ship' },
	{ name: 'Mustang Gamma', manufacturer: 'Consolidated Outland', kind: 'ship' },
	{ name: 'Mustang Omega', manufacturer: 'Consolidated Outland', kind: 'ship' },
	{ name: 'Nomad', manufacturer: 'Consolidated Outland', kind: 'ship' },
	// --- Crusader ---
	{ name: 'A1 Spirit', manufacturer: 'Crusader', kind: 'ship' },
	{ name: 'A2 Hercules Starlifter', manufacturer: 'Crusader', kind: 'ship' },
	{ name: 'Ares Star Fighter Inferno', manufacturer: 'Crusader', kind: 'ship' },
	{ name: 'Ares Star Fighter Ion', manufacturer: 'Crusader', kind: 'ship' },
	{ name: 'C1 Spirit', manufacturer: 'Crusader', kind: 'ship' },
	{ name: 'C2 Hercules Starlifter', manufacturer: 'Crusader', kind: 'ship' },
	{ name: 'Intrepid', manufacturer: 'Crusader', kind: 'ship' },
	{ name: 'M2 Hercules Starlifter', manufacturer: 'Crusader', kind: 'ship' },
	{ name: 'Mercury Star Runner', manufacturer: 'Crusader', kind: 'ship' },
	// --- Drake ---
	{ name: 'Buccaneer', manufacturer: 'Drake', kind: 'ship' },
	{ name: 'Caterpillar', manufacturer: 'Drake', kind: 'ship' },
	{ name: 'Caterpillar Pirate', manufacturer: 'Drake', kind: 'ship' },
	{ name: 'Clipper', manufacturer: 'Drake', kind: 'ship' },
	{ name: 'Command Module', manufacturer: 'Drake', kind: 'ship' },
	{ name: 'Corsair', manufacturer: 'Drake', kind: 'ship' },
	{ name: 'Cutlass Black', manufacturer: 'Drake', kind: 'ship' },
	{ name: 'Cutlass Blue', manufacturer: 'Drake', kind: 'ship' },
	{ name: 'Cutlass Red', manufacturer: 'Drake', kind: 'ship' },
	{ name: 'Cutlass Steel', manufacturer: 'Drake', kind: 'ship' },
	{ name: 'Cutter', manufacturer: 'Drake', kind: 'ship' },
	{ name: 'Cutter Rambler', manufacturer: 'Drake', kind: 'ship' },
	{ name: 'Cutter Scout', manufacturer: 'Drake', kind: 'ship' },
	{ name: 'Dragonfly', manufacturer: 'Drake', kind: 'vehicle' },
	{ name: 'Dragonfly Star Kitten', manufacturer: 'Drake', kind: 'vehicle' },
	{ name: 'Dragonfly Yellowjacket', manufacturer: 'Drake', kind: 'vehicle' },
	{ name: 'Golem', manufacturer: 'Drake', kind: 'ship' },
	{ name: 'Golem Alliance', manufacturer: 'Drake', kind: 'ship' },
	{ name: 'Golem OX', manufacturer: 'Drake', kind: 'ship' },
	{ name: 'Herald', manufacturer: 'Drake', kind: 'ship' },
	{ name: 'Ironclad', manufacturer: 'Drake', kind: 'ship' },
	{ name: 'Ironclad Assault', manufacturer: 'Drake', kind: 'ship' },
	{ name: 'Mule', manufacturer: 'Drake', kind: 'vehicle' },
	{ name: 'Pitbull', manufacturer: 'Drake', kind: 'ship', snub: true },
	{ name: 'Vulture', manufacturer: 'Drake', kind: 'ship' },
	// --- Esperia ---
	{ name: 'Blade', manufacturer: 'Esperia', kind: 'ship' },
	{ name: 'Glaive', manufacturer: 'Esperia', kind: 'ship' },
	{ name: 'Prowler', manufacturer: 'Esperia', kind: 'ship' },
	{ name: 'Prowler Utility', manufacturer: 'Esperia', kind: 'ship' },
	{ name: 'Stinger', manufacturer: 'Esperia', kind: 'ship' },
	{ name: 'Talon', manufacturer: 'Esperia', kind: 'ship' },
	{ name: 'Talon Shrike', manufacturer: 'Esperia', kind: 'ship' },
	{ name: 'Vanduul Scythe', manufacturer: 'Esperia', kind: 'ship' },
	// --- Gatac ---
	{ name: 'Railen', manufacturer: 'Gatac', kind: 'ship' },
	{ name: 'Syulen', manufacturer: 'Gatac', kind: 'ship' },
	{ name: 'Tyilui', manufacturer: 'Gatac', kind: 'ship' },
	// --- Grey's ---
	{ name: 'Basher', manufacturer: "Grey's Market", kind: 'ship' },
	{ name: 'Shiv', manufacturer: "Grey's Market", kind: 'ship' },
	// --- Greycat ---
	{ name: 'MDC', manufacturer: 'Greycat', kind: 'vehicle' },
	{ name: 'MTC', manufacturer: 'Greycat', kind: 'vehicle' },
	{ name: 'PTV', manufacturer: 'Greycat', kind: 'vehicle' },
	{ name: 'ROC', manufacturer: 'Greycat', kind: 'vehicle' },
	{ name: 'ROC-DS', manufacturer: 'Greycat', kind: 'vehicle' },
	{ name: 'STV', manufacturer: 'Greycat', kind: 'vehicle' },
	{ name: 'UTV', manufacturer: 'Greycat', kind: 'vehicle' },
	// --- Kruger ---
	{ name: 'L-21 Wolf', manufacturer: 'Kruger', kind: 'ship' },
	{ name: 'L-22 Alpha Wolf', manufacturer: 'Kruger', kind: 'ship' },
	{ name: 'P-52 Merlin', manufacturer: 'Kruger', kind: 'ship', snub: true },
	{ name: 'P-72 Archimedes', manufacturer: 'Kruger', kind: 'ship', snub: true },
	{ name: 'P-72 Archimedes Emerald', manufacturer: 'Kruger', kind: 'ship', snub: true },
	{ name: 'S-65 Stingray', manufacturer: 'Kruger', kind: 'ship' },
	// --- Mirai ---
	{ name: 'Fury', manufacturer: 'Mirai', kind: 'ship', snub: true },
	{ name: 'Fury LX', manufacturer: 'Mirai', kind: 'ship', snub: true },
	{ name: 'Fury MX', manufacturer: 'Mirai', kind: 'ship', snub: true },
	{ name: 'Guardian', manufacturer: 'Mirai', kind: 'ship' },
	{ name: 'Guardian MX', manufacturer: 'Mirai', kind: 'ship' },
	{ name: 'Guardian QI', manufacturer: 'Mirai', kind: 'ship' },
	{ name: 'Pulse', manufacturer: 'Mirai', kind: 'vehicle' },
	{ name: 'Pulse LX', manufacturer: 'Mirai', kind: 'vehicle' },
	{ name: 'Razor', manufacturer: 'Mirai', kind: 'vehicle' },
	{ name: 'Razor EX', manufacturer: 'Mirai', kind: 'vehicle' },
	{ name: 'Razor LX', manufacturer: 'Mirai', kind: 'vehicle' },
	// --- MISC ---
	{ name: 'Fortune', manufacturer: 'MISC', kind: 'ship' },
	{ name: 'Freelancer', manufacturer: 'MISC', kind: 'ship' },
	{ name: 'Freelancer DUR', manufacturer: 'MISC', kind: 'ship' },
	{ name: 'Freelancer MAX', manufacturer: 'MISC', kind: 'ship' },
	{ name: 'Freelancer MIS', manufacturer: 'MISC', kind: 'ship' },
	{ name: 'Hull A', manufacturer: 'MISC', kind: 'ship' },
	{ name: 'Hull B', manufacturer: 'MISC', kind: 'ship' },
	{ name: 'Hull C', manufacturer: 'MISC', kind: 'ship' },
	{ name: 'Prospector', manufacturer: 'MISC', kind: 'ship' },
	{ name: 'Prospector Alliance', manufacturer: 'MISC', kind: 'ship' },
	{ name: 'Reliant Kore', manufacturer: 'MISC', kind: 'ship' },
	{ name: 'Reliant Mako', manufacturer: 'MISC', kind: 'ship' },
	{ name: 'Reliant Sen', manufacturer: 'MISC', kind: 'ship' },
	{ name: 'Reliant Tana', manufacturer: 'MISC', kind: 'ship' },
	{ name: 'Starfarer', manufacturer: 'MISC', kind: 'ship' },
	{ name: 'Starfarer Gemini', manufacturer: 'MISC', kind: 'ship' },
	{ name: 'Starlancer MAX', manufacturer: 'MISC', kind: 'ship' },
	{ name: 'Starlancer TAC', manufacturer: 'MISC', kind: 'ship' },
	{ name: 'Starlite', manufacturer: 'MISC', kind: 'ship' },
	// --- Origin ---
	{ name: '100i', manufacturer: 'Origin', kind: 'ship' },
	{ name: '125a', manufacturer: 'Origin', kind: 'ship' },
	{ name: '135c', manufacturer: 'Origin', kind: 'ship' },
	{ name: '300i', manufacturer: 'Origin', kind: 'ship' },
	{ name: '315p', manufacturer: 'Origin', kind: 'ship' },
	{ name: '325a', manufacturer: 'Origin', kind: 'ship' },
	{ name: '350r', manufacturer: 'Origin', kind: 'ship' },
	{ name: '400i', manufacturer: 'Origin', kind: 'ship' },
	{ name: '600i', manufacturer: 'Origin', kind: 'ship' },
	{ name: '600i 2951 BIS', manufacturer: 'Origin', kind: 'ship' },
	{ name: '600i Executive Edition', manufacturer: 'Origin', kind: 'ship' },
	{ name: '600i Touring', manufacturer: 'Origin', kind: 'ship' },
	{ name: '85X Limited', manufacturer: 'Origin', kind: 'ship' },
	{ name: '890 Jump', manufacturer: 'Origin', kind: 'ship' },
	{ name: 'M50 Interceptor', manufacturer: 'Origin', kind: 'ship' },
	{ name: 'M80', manufacturer: 'Origin', kind: 'ship' },
	{ name: 'X1', manufacturer: 'Origin', kind: 'vehicle' },
	{ name: 'X1 Force', manufacturer: 'Origin', kind: 'vehicle' },
	{ name: 'X1 Velocity', manufacturer: 'Origin', kind: 'vehicle' },
	// --- RSI ---
	{ name: 'Apollo Medivac', manufacturer: 'RSI', kind: 'ship' },
	{ name: 'Apollo Triage', manufacturer: 'RSI', kind: 'ship' },
	{ name: 'Aurora Mk I CL', manufacturer: 'RSI', kind: 'ship' },
	{ name: 'Aurora Mk I ES', manufacturer: 'RSI', kind: 'ship' },
	{ name: 'Aurora Mk I LN', manufacturer: 'RSI', kind: 'ship' },
	{ name: 'Aurora Mk I LX', manufacturer: 'RSI', kind: 'ship' },
	{ name: 'Aurora Mk I MR', manufacturer: 'RSI', kind: 'ship' },
	{ name: 'Aurora Mk I SE', manufacturer: 'RSI', kind: 'ship' },
	{ name: 'Aurora Mk II', manufacturer: 'RSI', kind: 'ship' },
	{ name: 'Constellation Mk IV Andromeda', manufacturer: 'RSI', kind: 'ship' },
	{ name: 'Constellation Mk IV Aquila', manufacturer: 'RSI', kind: 'ship' },
	{ name: 'Constellation Mk IV Phoenix', manufacturer: 'RSI', kind: 'ship' },
	{ name: 'Constellation Mk IV Phoenix Emerald', manufacturer: 'RSI', kind: 'ship' },
	{ name: 'Constellation Mk IV Taurus', manufacturer: 'RSI', kind: 'ship' },
	{ name: 'Constellation Mk V Centaurus', manufacturer: 'RSI', kind: 'ship' },
	{ name: 'Hermes', manufacturer: 'RSI', kind: 'ship' },
	{ name: 'Lynx', manufacturer: 'RSI', kind: 'ship' },
	{ name: 'Mantis', manufacturer: 'RSI', kind: 'ship' },
	{ name: 'Meteor', manufacturer: 'RSI', kind: 'ship' },
	{ name: 'Perseus', manufacturer: 'RSI', kind: 'ship' },
	{ name: 'Polaris', manufacturer: 'RSI', kind: 'ship' },
	{ name: 'Salvation', manufacturer: 'RSI', kind: 'ship' },
	{ name: 'Scorpius', manufacturer: 'RSI', kind: 'ship' },
	{ name: 'Scorpius Antares', manufacturer: 'RSI', kind: 'ship' },
	{ name: 'Skylark', manufacturer: 'RSI', kind: 'ship', snub: true },
	{ name: 'Ursa', manufacturer: 'RSI', kind: 'vehicle' },
	{ name: 'Ursa Fortuna', manufacturer: 'RSI', kind: 'vehicle' },
	{ name: 'Ursa Medivac', manufacturer: 'RSI', kind: 'vehicle' },
	{ name: 'Zeus Mk II CL', manufacturer: 'RSI', kind: 'ship' },
	{ name: 'Zeus Mk II ES', manufacturer: 'RSI', kind: 'ship' },
	// --- Tumbril ---
	{ name: 'Cyclone', manufacturer: 'Tumbril', kind: 'vehicle' },
	{ name: 'Cyclone AA', manufacturer: 'Tumbril', kind: 'vehicle' },
	{ name: 'Cyclone MT', manufacturer: 'Tumbril', kind: 'vehicle' },
	{ name: 'Cyclone RC', manufacturer: 'Tumbril', kind: 'vehicle' },
	{ name: 'Cyclone RN', manufacturer: 'Tumbril', kind: 'vehicle' },
	{ name: 'Cyclone TR', manufacturer: 'Tumbril', kind: 'vehicle' },
	{ name: 'Nova', manufacturer: 'Tumbril', kind: 'vehicle' },
	{ name: 'Storm', manufacturer: 'Tumbril', kind: 'vehicle' },
	{ name: 'Storm AA', manufacturer: 'Tumbril', kind: 'vehicle' },
	// --- Vanduul ---
	{ name: 'Mauler Destroyer', manufacturer: 'Vanduul', kind: 'ship' },
	// UH (t_424660aa): Konzept-Schiffe — SC-Wiki Shipmatrix
	// (api.star-citizen.wiki/api/shipmatrix/vehicles, production_status
	// 'in-concept', live 2026-10-31: 265 vehicles / 31 in-concept; 'Javelin'
	// existierte bereits im Kuratierungs-Paket und bleibt unveraendert —
	// Kuratierung geht vor). concept=true -> dezenter 'Konzept'-Chip im
	// Hangar-Autocomplete. Modul-Logik unangetastet (keine Zuordnung).
	// --- Konzept (Shipmatrix in-concept) ---
	{ name: 'Arrastra', manufacturer: 'RSI', kind: 'ship', concept: true },
	{ name: 'Crucible', manufacturer: 'Anvil', kind: 'ship', concept: true },
	{ name: 'E1 Spirit', manufacturer: 'Crusader', kind: 'ship', concept: true },
	{ name: 'Endeavor', manufacturer: 'MISC', kind: 'ship', concept: true },
	{ name: 'Expanse', manufacturer: 'MISC', kind: 'ship', concept: true },
	{ name: 'G12', manufacturer: 'Origin', kind: 'vehicle', concept: true },
	{ name: 'G12a', manufacturer: 'Origin', kind: 'vehicle', concept: true },
	{ name: 'G12r', manufacturer: 'Origin', kind: 'ship', concept: true },
	{ name: 'Galaxy', manufacturer: 'RSI', kind: 'ship', concept: true },
	{ name: 'Genesis', manufacturer: 'Crusader', kind: 'ship', concept: true },
	{ name: 'Geotack Planetary Beacon', manufacturer: 'Greycat', kind: 'vehicle', concept: true },
	{ name: 'Geotack-X Planetary Beacon', manufacturer: 'Greycat', kind: 'vehicle', concept: true },
	{ name: 'Hull D', manufacturer: 'MISC', kind: 'ship', concept: true },
	{ name: 'Hull E', manufacturer: 'MISC', kind: 'ship', concept: true },
	{ name: 'Kraken', manufacturer: 'Drake', kind: 'ship', concept: true },
	{ name: 'Kraken Privateer', manufacturer: 'Drake', kind: 'ship', concept: true },
	{ name: 'Legionnaire', manufacturer: 'Anvil', kind: 'ship', concept: true },
	{ name: 'Liberator', manufacturer: 'Anvil', kind: 'ship', concept: true },
	{ name: 'Merchantman', manufacturer: 'Banu', kind: 'ship', concept: true },
	{ name: 'Nautilus', manufacturer: 'Aegis', kind: 'ship', concept: true },
	{ name: 'Nautilus Solstice Edition', manufacturer: 'Aegis', kind: 'ship', concept: true },
	{ name: 'Odin', manufacturer: 'Anvil', kind: 'ship', concept: true },
	{ name: 'Odyssey', manufacturer: 'MISC', kind: 'ship', concept: true },
	{ name: 'Orion', manufacturer: 'RSI', kind: 'ship', concept: true },
	{ name: 'Pioneer', manufacturer: 'Consolidated Outland', kind: 'ship', concept: true },
	{ name: 'Ranger CV', manufacturer: 'Tumbril', kind: 'vehicle', concept: true },
	{ name: 'Ranger RC', manufacturer: 'Tumbril', kind: 'vehicle', concept: true },
	{ name: 'Ranger TR', manufacturer: 'Tumbril', kind: 'vehicle', concept: true },
	{ name: 'Vulcan', manufacturer: 'Aegis', kind: 'ship', concept: true },
	{ name: 'Zeus Mk II MR', manufacturer: 'RSI', kind: 'ship', concept: true },
];

// Snub-Test: fixed list (see header). Case-insensitive, trims.
export function isSnubShip(name: string): boolean {
	const n = name.trim().toLowerCase();
	return SNUB_SHIP_NAMES.some((s) => s.toLowerCase() === n);
}

// Autocomplete-Candidates for the Name field: prefix first, then contains;
// matches name AND manufacturer. Static package only — the caller merges
// already-registered Hangar names on top (dedupe by name).
export function suggestShips(q: string, limit = 8): ShipDatum[] {
	const needle = q.trim().toLowerCase();
	if (!needle) return [];
	const hits = SHIP_CATALOG.filter(
		(s) => s.name.toLowerCase().includes(needle) || s.manufacturer.toLowerCase().includes(needle)
	);
	hits.sort((a, b) => {
		const pa = a.name.toLowerCase().startsWith(needle) ? 0 : 1;
		const pb = b.name.toLowerCase().startsWith(needle) ? 0 : 1;
	 return pa !== pb ? pa - pb : a.name.localeCompare(b.name);
	});
	return hits.slice(0, limit);
}
