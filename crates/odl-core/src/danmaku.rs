//! Danmaku (Bilibili) and comments (Niconico): fetching, parsing and conversion to ASS
//! subtitles that players such as mpv and VLC load next to the video.

use crate::auth::{parse_netscape, BROWSER_UA};
use anyhow::{anyhow, bail, Context};
use serde_json::Value;
use std::fmt::Write as _;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    Scroll,
    Top,
    Bottom,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Comment {
    /// Seconds from the start of the video.
    pub time: f64,
    pub text: String,
    pub placement: Placement,
    /// 0xRRGGBB
    pub color: u32,
    /// Relative to the normal size (1.0).
    pub size: f32,
}

// ---------------------------------------------------------------------------
// Bilibili: protobuf danmaku segments (`x/v2/dm/web/seg.so`)

/// Raw Bilibili danmaku element (subset of `DanmakuElem`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BiliElem {
    pub id: u64,
    pub progress_ms: u64,
    pub mode: u64,
    pub fontsize: u64,
    pub color: u64,
    pub mid_hash: String,
    pub content: String,
    pub ctime: u64,
    pub pool: u64,
}

fn read_varint(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *buf.get(*pos)?;
        *pos += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

/// Iterates `(field, wire value)` pairs of a protobuf message.
fn proto_fields(buf: &[u8]) -> Vec<(u64, ProtoValue<'_>)> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < buf.len() {
        let Some(key) = read_varint(buf, &mut pos) else { break };
        let (field, wire) = (key >> 3, key & 7);
        let value = match wire {
            0 => match read_varint(buf, &mut pos) {
                Some(v) => ProtoValue::Int(v),
                None => break,
            },
            2 => {
                let Some(len) = read_varint(buf, &mut pos) else { break };
                let end = pos.saturating_add(len as usize);
                if end > buf.len() {
                    break;
                }
                let bytes = &buf[pos..end];
                pos = end;
                ProtoValue::Bytes(bytes)
            }
            1 => {
                pos += 8;
                continue;
            }
            5 => {
                pos += 4;
                continue;
            }
            _ => break,
        };
        out.push((field, value));
    }
    out
}

enum ProtoValue<'a> {
    Int(u64),
    Bytes(&'a [u8]),
}

/// Parses a `DmSegMobileReply` message.
pub fn parse_bilibili_segment(data: &[u8]) -> anyhow::Result<Vec<BiliElem>> {
    if data.first() == Some(&b'{') {
        // An API error comes back as JSON instead of protobuf.
        bail!("Bilibili: {}", String::from_utf8_lossy(data));
    }
    let mut elems = Vec::new();
    for (field, value) in proto_fields(data) {
        let (1, ProtoValue::Bytes(msg)) = (field, value) else { continue };
        let mut e = BiliElem::default();
        for (f, v) in proto_fields(msg) {
            match (f, v) {
                (1, ProtoValue::Int(x)) => e.id = x,
                (2, ProtoValue::Int(x)) => e.progress_ms = x,
                (3, ProtoValue::Int(x)) => e.mode = x,
                (4, ProtoValue::Int(x)) => e.fontsize = x,
                (5, ProtoValue::Int(x)) => e.color = x,
                (6, ProtoValue::Bytes(b)) => e.mid_hash = String::from_utf8_lossy(b).into_owned(),
                (7, ProtoValue::Bytes(b)) => e.content = String::from_utf8_lossy(b).into_owned(),
                (8, ProtoValue::Int(x)) => e.ctime = x,
                (11, ProtoValue::Int(x)) => e.pool = x,
                _ => {}
            }
        }
        elems.push(e);
    }
    Ok(elems)
}

pub fn bilibili_to_comments(elems: &[BiliElem]) -> Vec<Comment> {
    let mut out: Vec<Comment> = elems
        .iter()
        .filter_map(|e| {
            // 7 = positioned/advanced, 8 = code, 9 = BAS: not representable as plain text.
            let placement = match e.mode {
                1..=3 | 6 => Placement::Scroll,
                4 => Placement::Bottom,
                5 => Placement::Top,
                _ => return None,
            };
            if e.content.trim().is_empty() || e.pool == 2 {
                return None;
            }
            Some(Comment {
                time: e.progress_ms as f64 / 1000.0,
                text: e.content.clone(),
                placement,
                color: (e.color & 0xff_ffff) as u32,
                size: if e.fontsize == 0 { 1.0 } else { (e.fontsize as f32 / 25.0).clamp(0.5, 2.0) },
            })
        })
        .collect();
    out.sort_by(|a, b| a.time.total_cmp(&b.time));
    out
}

/// The classic `comment.bilibili.com` XML format (for danmaku players that want it).
pub fn bilibili_to_xml(cid: u64, elems: &[BiliElem]) -> String {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
    let mut out = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<i><chatserver>chat.bilibili.com</chatserver><chatid>{cid}</chatid>\n");
    for e in elems {
        let _ = writeln!(
            out,
            "<d p=\"{:.5},{},{},{},{},{},{},{}\">{}</d>",
            e.progress_ms as f64 / 1000.0,
            e.mode,
            e.fontsize,
            e.color,
            e.ctime,
            e.pool,
            esc(&e.mid_hash),
            e.id,
            esc(&e.content)
        );
    }
    out.push_str("</i>\n");
    out
}

#[derive(Debug, Clone, PartialEq)]
enum BiliRef {
    Bv(String, u32),
    Av(u64, u32),
    Ep(u64),
}

fn bili_ref(url: &url::Url) -> Option<BiliRef> {
    let page = url
        .query_pairs()
        .find(|(k, _)| k == "p")
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(1u32)
        .max(1);
    let path = url.path();
    if let Some(i) = path.find("BV") {
        let id: String = path[i..].chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
        if id.len() == 12 {
            return Some(BiliRef::Bv(id, page));
        }
    }
    let number_after = |marker: &str| -> Option<u64> {
        let i = path.find(marker)? + marker.len();
        path[i..].chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok()
    };
    if let Some(av) = number_after("/video/av") {
        return Some(BiliRef::Av(av, page));
    }
    number_after("/play/ep").map(BiliRef::Ep)
}

fn cookie_header(cookie_file: Option<&Path>) -> Option<String> {
    let text = std::fs::read_to_string(cookie_file?).ok()?;
    let header: Vec<String> = parse_netscape(&text)
        .into_iter()
        .filter(|c| c.domain.trim_start_matches('.').ends_with("bilibili.com"))
        .map(|c| format!("{}={}", c.name, c.value))
        .collect();
    (!header.is_empty()).then(|| header.join("; "))
}

/// Finds the `cid` (danmaku pool id) and duration in seconds of a Bilibili video page.
async fn bili_cid(client: &reqwest::Client, url: &str, cookies: Option<&str>) -> anyhow::Result<(u64, f64)> {
    let mut parsed = url::Url::parse(url)?;
    if parsed.host_str() == Some("b23.tv") {
        // Short link: follow the redirect.
        let resp = client.get(url).send().await?;
        parsed = resp.url().clone();
    }
    let reference = bili_ref(&parsed).ok_or_else(|| anyhow!("unrecognized Bilibili link"))?;
    let get = |api: String| {
        let mut req = client.get(api).header("Referer", "https://www.bilibili.com/");
        if let Some(c) = cookies {
            req = req.header(reqwest::header::COOKIE, c);
        }
        req
    };
    match reference {
        BiliRef::Bv(..) | BiliRef::Av(..) => {
            let (query, page) = match &reference {
                BiliRef::Bv(id, p) => (format!("bvid={id}"), *p),
                BiliRef::Av(id, p) => (format!("aid={id}"), *p),
                BiliRef::Ep(_) => unreachable!(),
            };
            let v: Value = get(format!("https://api.bilibili.com/x/web-interface/view?{query}")).send().await?.json().await?;
            if v["code"].as_i64() != Some(0) {
                bail!("Bilibili: {}", v["message"].as_str().unwrap_or("video info unavailable"));
            }
            let pages = v["data"]["pages"].as_array().cloned().unwrap_or_default();
            let entry = pages
                .iter()
                .find(|p| p["page"].as_u64() == Some(page as u64))
                .or(pages.first())
                .ok_or_else(|| anyhow!("Bilibili: video has no pages"))?;
            let cid = entry["cid"].as_u64().ok_or_else(|| anyhow!("Bilibili: no cid"))?;
            Ok((cid, entry["duration"].as_f64().unwrap_or(0.0)))
        }
        BiliRef::Ep(ep) => {
            let v: Value =
                get(format!("https://api.bilibili.com/pgc/view/web/season?ep_id={ep}")).send().await?.json().await?;
            let episodes = v["result"]["episodes"].as_array().cloned().unwrap_or_default();
            let entry = episodes
                .iter()
                .find(|e| e["ep_id"].as_u64() == Some(ep) || e["id"].as_u64() == Some(ep))
                .ok_or_else(|| anyhow!("Bilibili: episode not found"))?;
            let cid = entry["cid"].as_u64().ok_or_else(|| anyhow!("Bilibili: no cid"))?;
            // Episode durations are in milliseconds.
            Ok((cid, entry["duration"].as_f64().unwrap_or(0.0) / 1000.0))
        }
    }
}

/// Downloads all danmaku of a Bilibili video (6-minute protobuf segments).
pub async fn fetch_bilibili(url: &str, cookie_file: Option<&Path>) -> anyhow::Result<(u64, Vec<BiliElem>)> {
    let client = reqwest::Client::builder().user_agent(BROWSER_UA).timeout(Duration::from_secs(20)).build()?;
    let cookies = cookie_header(cookie_file);
    let (cid, duration) = bili_cid(&client, url, cookies.as_deref()).await.context("finding the danmaku pool")?;
    let segments = ((duration / 360.0).ceil() as u32).clamp(1, 100);
    let mut elems = Vec::new();
    for index in 1..=segments {
        let mut req = client
            .get("https://api.bilibili.com/x/v2/dm/web/seg.so")
            .query(&[("type", "1".to_string()), ("oid", cid.to_string()), ("segment_index", index.to_string())])
            .header("Referer", "https://www.bilibili.com/");
        if let Some(c) = &cookies {
            req = req.header(reqwest::header::COOKIE, c);
        }
        let bytes = req.send().await?.error_for_status()?.bytes().await?;
        elems.extend(parse_bilibili_segment(&bytes)?);
        // Be gentle with the API.
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    elems.sort_by_key(|e| e.progress_ms);
    elems.dedup_by_key(|e| e.id);
    Ok((cid, elems))
}

// ---------------------------------------------------------------------------
// Niconico comments (yt-dlp's `comments` subtitle JSON)

fn nico_color(name: &str) -> Option<u32> {
    if let Some(hex) = name.strip_prefix('#') {
        return u32::from_str_radix(hex, 16).ok().filter(|_| hex.len() == 6);
    }
    Some(match name {
        "white" => 0xFFFFFF,
        "red" => 0xFF0000,
        "pink" => 0xFF8080,
        "orange" => 0xFFC000,
        "yellow" => 0xFFFF00,
        "green" => 0x00FF00,
        "cyan" => 0x00FFFF,
        "blue" => 0x0000FF,
        "purple" => 0xC000FF,
        "black" => 0x000000,
        "white2" | "niconicowhite" => 0xCCCC99,
        "red2" | "truered" => 0xCC0033,
        "pink2" => 0xFF33CC,
        "orange2" | "passionorange" => 0xFF6600,
        "yellow2" | "madyellow" => 0x999900,
        "green2" | "elementalgreen" => 0x00CC66,
        "cyan2" => 0x00CCCC,
        "blue2" | "marineblue" => 0x3399FF,
        "purple2" | "nobleviolet" => 0x6633CC,
        "black2" => 0x666666,
        _ => return None,
    })
}

/// Parses Niconico comments: a flat list of comments or a list of threads.
pub fn parse_niconico(json: &Value) -> Vec<Comment> {
    let mut raw: Vec<&Value> = Vec::new();
    let list = json
        .as_array()
        .or_else(|| json.pointer("/data/threads").and_then(Value::as_array))
        .cloned()
        .unwrap_or_default();
    let list: &[Value] = &list;
    for item in list {
        match item.get("comments").and_then(Value::as_array) {
            Some(comments) => raw.extend(comments.iter()),
            None => raw.push(item),
        }
    }
    let mut out: Vec<Comment> = raw
        .into_iter()
        .filter_map(|c| {
            let body = c.get("body")?.as_str()?.trim();
            if body.is_empty() || body.starts_with('/') {
                return None; // owner scripts like /nicoscript
            }
            let ms = c.get("vposMs").and_then(Value::as_f64).or_else(|| c.get("vpos").and_then(Value::as_f64).map(|v| v * 10.0))?;
            let commands: Vec<&str> = c
                .get("commands")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            if commands.contains(&"invisible") {
                return None;
            }
            let mut comment = Comment {
                time: ms / 1000.0,
                text: body.to_string(),
                placement: Placement::Scroll,
                color: 0xFFFFFF,
                size: 1.0,
            };
            for cmd in commands {
                match cmd {
                    "ue" => comment.placement = Placement::Top,
                    "shita" => comment.placement = Placement::Bottom,
                    "big" => comment.size = 1.45,
                    "small" => comment.size = 0.7,
                    other => {
                        if let Some(color) = nico_color(other) {
                            comment.color = color;
                        }
                    }
                }
            }
            Some(comment)
        })
        .collect();
    // The API returns comments grouped by thread/fork; the same text can appear twice.
    out.sort_by(|a, b| a.time.total_cmp(&b.time));
    out
}

// ---------------------------------------------------------------------------
// ASS generation

#[derive(Debug, Clone)]
pub struct AssConfig {
    pub width: f32,
    pub height: f32,
    pub font: String,
    pub font_size: f32,
    /// How long a scrolling comment takes to cross the screen.
    pub scroll_secs: f64,
    /// How long top/bottom comments stay.
    pub fixed_secs: f64,
    /// 0 = opaque, 255 = invisible.
    pub alpha: u8,
}

impl AssConfig {
    pub fn bilibili() -> Self {
        Self {
            width: 1920.0,
            height: 1080.0,
            font: "Noto Sans CJK SC".into(),
            font_size: 46.0,
            scroll_secs: 8.0,
            fixed_secs: 4.0,
            alpha: 0x30,
        }
    }

    pub fn niconico() -> Self {
        Self { font: "Noto Sans CJK JP".into(), font_size: 52.0, scroll_secs: 5.0, fixed_secs: 3.0, ..Self::bilibili() }
    }
}

fn text_width(text: &str, size: f32) -> f32 {
    text.lines()
        .map(|line| {
            line.chars()
                .map(|c| match c {
                    c if c.is_ascii() => 0.56,
                    '\u{ff61}'..='\u{ff9f}' => 0.56, // half-width katakana
                    _ => 1.0,
                })
                .sum::<f32>()
        })
        .fold(0.0, f32::max)
        * size
}

fn ass_time(t: f64) -> String {
    let cs = (t.max(0.0) * 100.0).round() as u64;
    format!("{}:{:02}:{:02}.{:02}", cs / 360_000, (cs / 6000) % 60, (cs / 100) % 60, cs % 100)
}

fn ass_escape(text: &str) -> String {
    text.replace('\\', "＼").replace('{', "｛").replace('}', "｝").replace("\r\n", "\n").replace('\n', "\\N")
}

fn ass_color(rgb: u32) -> String {
    format!("&H{:02X}{:02X}{:02X}&", rgb & 0xff, (rgb >> 8) & 0xff, (rgb >> 16) & 0xff)
}

#[derive(Clone, Copy)]
struct ScrollSlot {
    start: f64,
    end: f64,
    width: f32,
    speed: f32,
}

/// Lays comments out without overlap (comments that don't fit are dropped) and renders ASS.
pub fn to_ass(comments: &[Comment], cfg: &AssConfig) -> String {
    let pitch = cfg.font_size * 1.15;
    let rows = ((cfg.height / pitch).floor() as usize).max(1);
    let mut scroll: Vec<Option<ScrollSlot>> = vec![None; rows];
    let mut top: Vec<f64> = vec![f64::MIN; rows];
    let mut bottom: Vec<f64> = vec![f64::MIN; rows];
    let alpha = format!("{:02X}", cfg.alpha);
    let mut out = String::new();
    let _ = write!(
        out,
        "[Script Info]\nTitle: Danmaku generated by OpenDownloader\nScriptType: v4.00+\nPlayResX: {w}\nPlayResY: {h}\n\
         WrapStyle: 2\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\n\
         Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
         Style: Danmaku,{font},{fs},&H{a}FFFFFF,&H{a}FFFFFF,&H{a}000000,&H{a}000000,0,0,0,0,100,100,0,0,1,2,0,7,0,0,0,1\n\n\
         [Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
        w = cfg.width,
        h = cfg.height,
        font = cfg.font,
        fs = cfg.font_size,
        a = alpha
    );

    for c in comments {
        let size = cfg.font_size * c.size;
        let lines = c.text.lines().count().max(1) as f32;
        let height = size * 1.15 * lines;
        let span = ((height / pitch).ceil() as usize).clamp(1, rows);
        let width = text_width(&c.text, size);
        let mut tags = String::new();
        if c.color != 0xFFFFFF {
            tags.push_str(&format!("\\c{}", ass_color(c.color)));
            // Dark text needs a light outline to stay readable.
            let luma = 0.299 * ((c.color >> 16) & 0xff) as f32 + 0.587 * ((c.color >> 8) & 0xff) as f32 + 0.114 * (c.color & 0xff) as f32;
            if luma < 50.0 {
                tags.push_str("\\3c&HFFFFFF&");
            }
        }
        if (c.size - 1.0).abs() > 0.01 {
            tags.push_str(&format!("\\fs{}", size.round()));
        }
        let text = ass_escape(&c.text);
        match c.placement {
            Placement::Scroll => {
                let dur = cfg.scroll_secs;
                let speed = (cfg.width + width) / dur as f32;
                let fits = |slot: &Option<ScrollSlot>| match slot {
                    None => true,
                    Some(prev) => {
                        // The previous comment's tail must be on screen, and we must not catch up with it.
                        let tail_in = c.time >= prev.start + (prev.width / prev.speed) as f64;
                        let no_catch_up = c.time + (cfg.width / speed) as f64 >= prev.end;
                        tail_in && no_catch_up
                    }
                };
                let Some(row) = (0..=rows - span).find(|&r| scroll[r..r + span].iter().all(fits)) else { continue };
                let slot = ScrollSlot { start: c.time, end: c.time + dur, width, speed };
                scroll[row..row + span].iter_mut().for_each(|s| *s = Some(slot));
                let y = row as f32 * pitch;
                let _ = writeln!(
                    out,
                    "Dialogue: 2,{},{},Danmaku,,0,0,0,,{{\\move({:.0},{:.0},{:.0},{:.0}){tags}}}{text}",
                    ass_time(c.time),
                    ass_time(c.time + dur),
                    cfg.width,
                    y,
                    -width,
                    y
                );
            }
            Placement::Top | Placement::Bottom => {
                let lanes = if c.placement == Placement::Top { &mut top } else { &mut bottom };
                let Some(row) = (0..=rows - span).find(|&r| lanes[r..r + span].iter().all(|end| *end <= c.time)) else {
                    continue;
                };
                lanes[row..row + span].iter_mut().for_each(|e| *e = c.time + cfg.fixed_secs);
                let (align, y) = match c.placement {
                    Placement::Top => (8, row as f32 * pitch),
                    _ => (2, cfg.height - row as f32 * pitch),
                };
                let _ = writeln!(
                    out,
                    "Dialogue: 3,{},{},Danmaku,,0,0,0,,{{\\an{align}\\pos({:.0},{:.0}){tags}}}{text}",
                    ass_time(c.time),
                    ass_time(c.time + cfg.fixed_secs),
                    cfg.width / 2.0,
                    y
                );
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn varint(mut v: u64, out: &mut Vec<u8>) {
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(b);
                break;
            }
            out.push(b | 0x80);
        }
    }

    fn elem(progress: u64, mode: u64, color: u64, text: &str) -> Vec<u8> {
        let mut m = Vec::new();
        for (field, v) in [(1u64, 99u64), (2, progress), (3, mode), (4, 25), (5, color)] {
            varint(field << 3, &mut m);
            varint(v, &mut m);
        }
        varint((7 << 3) | 2, &mut m);
        varint(text.len() as u64, &mut m);
        m.extend_from_slice(text.as_bytes());
        let mut wrapped = Vec::new();
        varint((1 << 3) | 2, &mut wrapped);
        varint(m.len() as u64, &mut wrapped);
        wrapped.extend(m);
        wrapped
    }

    #[test]
    fn parses_bilibili_protobuf() {
        let mut data = elem(13_798, 1, 0xFFFFFF, "1播放");
        data.extend(elem(2_000, 5, 0xFF0000, "顶部"));
        data.extend(elem(3_000, 7, 0, "[advanced]"));
        let elems = parse_bilibili_segment(&data).unwrap();
        assert_eq!(elems.len(), 3);
        assert_eq!(elems[0].progress_ms, 13_798);
        assert_eq!(elems[0].content, "1播放");
        let comments = bilibili_to_comments(&elems);
        assert_eq!(comments.len(), 2); // mode 7 dropped
        assert_eq!(comments[0].placement, Placement::Top);
        assert_eq!(comments[0].color, 0xFF0000);
        assert!(parse_bilibili_segment(br#"{"code":-400}"#).is_err());
        assert!(bilibili_to_xml(1, &elems).contains(r#"<d p="13.79800,1,25,16777215,0,0,,99">1播放</d>"#));
    }

    #[test]
    fn parses_niconico_commands() {
        let v: Value = serde_json::from_str(
            r##"[{"vposMs": 170800, "body": "歌詞なんだっけ？", "commands": ["shita", "big", "yellow", "184"]},
                {"vposMs": 1000, "body": "/nicoscript", "commands": []},
                {"fork": "main", "comments": [{"vposMs": 500, "body": "うぽつ", "commands": ["#00ff80"]}]}]"##,
        )
        .unwrap();
        let c = parse_niconico(&v);
        assert_eq!(c.len(), 2);
        assert_eq!((c[0].text.as_str(), c[0].color), ("うぽつ", 0x00ff80));
        assert_eq!(c[1].placement, Placement::Bottom);
        assert_eq!(c[1].color, 0xFFFF00);
        assert!((c[1].size - 1.45).abs() < 1e-6);
    }

    #[test]
    fn ass_layout_avoids_overlap() {
        let mk = |t: f64, p: Placement| Comment { time: t, text: "テストコメント".into(), placement: p, color: 0xFFFFFF, size: 1.0 };
        let comments = vec![mk(1.0, Placement::Scroll), mk(1.0, Placement::Scroll), mk(1.0, Placement::Top), mk(1.0, Placement::Top)];
        let ass = to_ass(&comments, &AssConfig::bilibili());
        let events: Vec<&str> = ass.lines().filter(|l| l.starts_with("Dialogue:")).collect();
        assert_eq!(events.len(), 4);
        // Same start time → different rows.
        assert!(events[0].contains("\\move(1920,0,"));
        assert!(events[1].contains("\\move(1920,53,"));
        assert!(events[2].contains("\\an8\\pos(960,0)"));
        assert!(events[3].contains("\\an8\\pos(960,53)"));
        assert!(events[0].starts_with("Dialogue: 2,0:00:01.00,0:00:09.00"));
        assert_eq!(ass_escape("a{b}\\c\nd"), "a｛b｝＼c\\Nd");
        assert_eq!(ass_color(0x112233), "&H332211&");
    }

    #[test]
    fn bilibili_refs() {
        let u = |s: &str| bili_ref(&url::Url::parse(s).unwrap());
        assert_eq!(u("https://www.bilibili.com/video/BV1XtpM6vEwv/?p=3"), Some(BiliRef::Bv("BV1XtpM6vEwv".into(), 3)));
        assert_eq!(u("https://www.bilibili.com/video/av170001"), Some(BiliRef::Av(170001, 1)));
        assert_eq!(u("https://www.bilibili.com/bangumi/play/ep12345"), Some(BiliRef::Ep(12345)));
        assert_eq!(u("https://www.bilibili.com/"), None);
    }
}
