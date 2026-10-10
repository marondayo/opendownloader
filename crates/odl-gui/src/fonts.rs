//! Loads system fonts so titles in Japanese, Chinese and Korean render properly.

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use std::sync::Arc;

/// Preferred UI fonts (first found wins); egui's bundled fonts stay as fallbacks for icons.
const UI_FAMILIES: &[&str] = &["Inter", "Noto Sans", "Cantarell", "Ubuntu", "DejaVu Sans", "Segoe UI"];

/// CJK fallbacks. Noto Sans CJK JP also covers Simplified/Traditional Chinese and Hangul.
const CJK_FAMILIES: &[&str] = &[
    "Noto Sans CJK JP",
    "Noto Sans JP",
    "Source Han Sans JP",
    "Source Han Sans",
    "Noto Sans CJK SC",
    "Noto Sans SC",
    "WenQuanYi Micro Hei",
    "WenQuanYi Zen Hei",
    "Droid Sans Fallback",
    "IPAGothic",
    "IPAexGothic",
    "TakaoGothic",
    "VL Gothic",
    "Yu Gothic UI",
    "Meiryo UI",
    "Meiryo",
    "Microsoft YaHei UI",
    "Microsoft YaHei",
    "MS Gothic",
    "Malgun Gothic",
];

/// Extra fallbacks for scripts the above may miss.
const EXTRA_FAMILIES: &[&str] = &["Noto Sans Thai", "Noto Sans Arabic", "Noto Sans Devanagari", "Noto Sans Symbols 2"];

fn load(db: &fontdb::Database, family: &str) -> Option<FontData> {
    let query = fontdb::Query {
        families: &[fontdb::Family::Name(family)],
        weight: fontdb::Weight::NORMAL,
        stretch: fontdb::Stretch::Normal,
        style: fontdb::Style::Normal,
    };
    let id = db.query(&query)?;
    db.with_face_data(id, |data, index| {
        let mut font = FontData::from_owned(data.to_vec());
        font.index = index;
        font
    })
}

pub fn install(ctx: &egui::Context) {
    let mut defs = FontDefinitions::default();
    let mut db = fontdb::Database::new();
    db.load_system_fonts();

    if let Some((name, data)) = UI_FAMILIES.iter().find_map(|f| load(&db, f).map(|d| (format!("ui:{f}"), d))) {
        tracing::info!("UI font: {name}");
        defs.font_data.insert(name.clone(), Arc::new(data));
        if let Some(list) = defs.families.get_mut(&FontFamily::Proportional) {
            list.insert(0, name);
        }
    }

    let mut fallbacks = Vec::new();
    let mut cjk = CJK_FAMILIES.iter().find_map(|f| load(&db, f).map(|d| (format!("cjk:{f}"), d)));
    if cjk.is_none() {
        // Under WSL the Windows fonts are usually reachable; load just the CJK ones.
        for file in ["YuGothM.ttc", "meiryo.ttc", "msgothic.ttc", "msyh.ttc", "malgun.ttf"] {
            let path = std::path::Path::new("/mnt/c/Windows/Fonts").join(file);
            if path.exists() && db.load_font_file(&path).is_ok() {
                break;
            }
        }
        cjk = CJK_FAMILIES.iter().find_map(|f| load(&db, f).map(|d| (format!("cjk:{f}"), d)));
    }
    if let Some(found) = cjk {
        tracing::info!("CJK font: {}", found.0);
        fallbacks.push(found);
    } else {
        tracing::warn!("no CJK font found; install fonts-noto-cjk to render Japanese/Chinese/Korean titles");
    }
    for f in EXTRA_FAMILIES {
        if let Some(d) = load(&db, f) {
            fallbacks.push((format!("extra:{f}"), d));
        }
    }
    for (name, data) in fallbacks {
        defs.font_data.insert(name.clone(), Arc::new(data));
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            if let Some(list) = defs.families.get_mut(&family) {
                list.push(name.clone());
            }
        }
    }
    ctx.set_fonts(defs);
}
