#!/usr/bin/env python3
"""Recolor-Script: erzeugt alle Colorways aus dem Drake-Master per Hex-Swap."""
import pathlib, subprocess, shutil

MASTER = pathlib.Path(__file__).parent / "drake" / "architects-overlay-drake.svg"
OUT = pathlib.Path(__file__).parent

# (Name, STRUCT/RING/Gelb, ACCENT/HAZARD/Rot, CUBE_SIDE dunkel)
COLORWAYS = [
    ("drake",       "#f5c518", "#e01e2c", "#a51222"),
    ("amber-mono",  "#e8a33d", "#8f5f1e", "#6b4a17"),
    ("blueprint",   "#ffffff", "#7fd4ff", "#4f9ed4"),
    ("steel",       "#4da3ff", "#e8f0f8", "#2f6fbf"),
    ("cargo-green", "#3ddc84", "#8a9a94", "#2aa55e"),
    ("crimson",     "#c41e3a", "#f5efe6", "#8f1428"),
    ("violet",      "#9b5cff", "#ff5ca8", "#6d3fd1"),
]

def swap(svg: str, new_struct, new_accent, new_side):
    s = svg.replace("#f5c518", new_struct).replace("#e01e2c", new_accent).replace("#a51222", new_side)
    s = s.replace("#c41a2a", new_side)  # rechte Würfelfläche = dunkler Ton
    return s

def render_png(svg_path: pathlib.Path, out_png: pathlib.Path, size: int, bg: str = "00000000"):
    subprocess.run([
        "chromium", "--headless", "--disable-gpu", "--no-sandbox",
        f"--screenshot={out_png}", f"--window-size={size},{size}",
        f"--default-background-color={bg}", f"--force-device-scale-factor=1",
        f"file://{svg_path}",
    ], check=True, capture_output=True)

def main():
    master = MASTER.read_text()
    for name, struct, accent, side in COLORWAYS:
        d = OUT / name
        d.mkdir(exist_ok=True)
        (d / "architects-overlay.svg").write_text(swap(master, struct, accent, side))
        render_png(d / "architects-overlay.svg", d / "emblem-512.png", 512)
        render_png(d / "architects-overlay.svg", d / "icon-256-dark.png", 256, bg="#14161aff")
        # Favicon-Set nur für Drake (Master)
        if name == "drake":
            for s in (16, 32, 48):
                render_png(MASTER, OUT / "drake" / f"favicon-{s}.png", s)
        print(f"{name}: svg + pngs ok")

if __name__ == "__main__":
    main()
