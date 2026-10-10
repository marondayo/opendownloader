//! Link classification for the supported sites.
//!
//! Only YouTube, Niconico, Bilibili and SoundCloud are supported for now; any other
//! host is rejected up front instead of being handed to yt-dlp.

use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Site {
    YouTube,
    Niconico,
    Bilibili,
    SoundCloud,
}

impl Site {
    pub const ALL: [Site; 4] = [Site::YouTube, Site::Niconico, Site::Bilibili, Site::SoundCloud];

    pub fn name(self) -> &'static str {
        match self {
            Site::YouTube => "YouTube",
            Site::Niconico => "Niconico",
            Site::Bilibili => "Bilibili",
            Site::SoundCloud => "SoundCloud",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Site::YouTube => "youtube",
            Site::Niconico => "niconico",
            Site::Bilibili => "bilibili",
            Site::SoundCloud => "soundcloud",
        }
    }

    /// The `machine` name yt-dlp looks up in a `.netrc` file for this site.
    pub fn netrc_machine(self) -> &'static str {
        self.key()
    }

    /// SoundCloud is audio-only; everything else offers video.
    pub fn has_video(self) -> bool {
        !matches!(self, Site::SoundCloud)
    }

    pub fn from_host(host: &str) -> Option<Site> {
        let host = host.to_ascii_lowercase();
        let matches = |domain: &str| host == domain || host.ends_with(&format!(".{domain}"));
        if ["youtube.com", "youtu.be", "youtube-nocookie.com"].iter().any(|d| matches(d)) {
            Some(Site::YouTube)
        } else if ["nicovideo.jp", "nico.ms"].iter().any(|d| matches(d)) {
            Some(Site::Niconico)
        } else if ["bilibili.com", "b23.tv", "bilibili.tv"].iter().any(|d| matches(d)) {
            Some(Site::Bilibili)
        } else if ["soundcloud.com", "snd.sc"].iter().any(|d| matches(d)) {
            Some(Site::SoundCloud)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkKind {
    /// A single video or track (may still turn out to be multi-part after probing).
    Video,
    /// A playlist, mylist, channel, set, series, …
    Collection,
    /// A YouTube `watch?v=…&list=…` link: one video that is part of a playlist.
    VideoInPlaylist { video_id: String, playlist_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub url: String,
    pub site: Site,
    pub kind: LinkKind,
}

impl Link {
    /// URL that downloads only the single video, ignoring the playlist.
    pub fn video_only_url(&self) -> String {
        match &self.kind {
            LinkKind::VideoInPlaylist { video_id, .. } => {
                format!("https://www.youtube.com/watch?v={video_id}")
            }
            _ => self.url.clone(),
        }
    }

    /// URL of the whole playlist.
    pub fn playlist_url(&self) -> String {
        match &self.kind {
            LinkKind::VideoInPlaylist { playlist_id, .. } => {
                format!("https://www.youtube.com/playlist?list={playlist_id}")
            }
            _ => self.url.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LinkError {
    #[error("That doesn't look like a link.")]
    NotAUrl,
    #[error("{0} isn't supported yet. OpenDownloader currently supports YouTube, Niconico, Bilibili and SoundCloud.")]
    Unsupported(String),
}

/// Parses user input (pasted text) into a supported [`Link`].
pub fn parse_link(input: &str) -> Result<Link, LinkError> {
    let token = input
        .split_whitespace()
        .find(|t| t.contains('.'))
        .ok_or(LinkError::NotAUrl)?;
    let candidate = if token.starts_with("http://") || token.starts_with("https://") {
        token.to_string()
    } else if token.contains("://") {
        return Err(LinkError::NotAUrl);
    } else {
        format!("https://{token}")
    };
    let mut url = Url::parse(&candidate).map_err(|_| LinkError::NotAUrl)?;
    let host = url.host_str().ok_or(LinkError::NotAUrl)?.to_ascii_lowercase();
    if !host.contains('.') {
        return Err(LinkError::NotAUrl);
    }
    let site = Site::from_host(&host).ok_or_else(|| LinkError::Unsupported(host.clone()))?;
    if url.scheme() == "http" {
        let _ = url.set_scheme("https");
    }
    let kind = match site {
        Site::YouTube => classify_youtube(&mut url, &host),
        Site::Niconico => classify_niconico(&url, &host),
        Site::Bilibili => classify_bilibili(&url, &host),
        Site::SoundCloud => classify_soundcloud(&url, &host),
    };
    Ok(Link { url: url.to_string(), site, kind })
}

fn query_param(url: &Url, key: &str) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
        .filter(|v| !v.is_empty())
}

fn segments(url: &Url) -> Vec<String> {
    url.path_segments()
        .map(|s| s.filter(|p| !p.is_empty()).map(str::to_string).collect())
        .unwrap_or_default()
}

fn classify_youtube(url: &mut Url, host: &str) -> LinkKind {
    let segs = segments(url);
    let list = query_param(url, "list");
    let in_playlist = |video_id: String| match &list {
        Some(playlist_id) => LinkKind::VideoInPlaylist { video_id, playlist_id: playlist_id.clone() },
        None => LinkKind::Video,
    };

    if host == "youtu.be" || host.ends_with(".youtu.be") {
        return match segs.first() {
            Some(id) => in_playlist(id.clone()),
            None => LinkKind::Video,
        };
    }
    match segs.first().map(String::as_str) {
        Some("watch") => match query_param(url, "v") {
            Some(v) => in_playlist(v),
            None if list.is_some() => LinkKind::Collection,
            None => LinkKind::Video,
        },
        Some("playlist") => LinkKind::Collection,
        Some("shorts" | "live" | "embed" | "v") => LinkKind::Video,
        Some(first) if first.starts_with('@') || matches!(first, "channel" | "c" | "user") => {
            // A bare channel URL yields a playlist of tabs; point it at the uploads tab.
            let base_len = if first.starts_with('@') { 1 } else { 2 };
            if segs.len() == base_len {
                let path = format!("/{}/videos", segs.join("/"));
                url.set_path(&path);
            }
            LinkKind::Collection
        }
        _ => LinkKind::Video,
    }
}

fn classify_niconico(url: &Url, host: &str) -> LinkKind {
    let segs = segments(url);
    if host == "nico.ms" {
        return LinkKind::Video;
    }
    match segs.first().map(String::as_str) {
        Some("watch") => LinkKind::Video,
        _ => LinkKind::Collection,
    }
}

fn classify_bilibili(url: &Url, host: &str) -> LinkKind {
    let segs = segments(url);
    if host == "b23.tv" || host.starts_with("space.") {
        return if host == "b23.tv" { LinkKind::Video } else { LinkKind::Collection };
    }
    match (segs.first().map(String::as_str), segs.get(1).map(String::as_str), segs.get(2)) {
        (Some("bangumi"), Some("play"), Some(id)) if id.starts_with("ss") => LinkKind::Collection,
        (Some("bangumi"), Some("media"), _) => LinkKind::Collection,
        (Some("medialist" | "list" | "festival" | "favlist"), _, _) => LinkKind::Collection,
        (Some("audio"), Some(id), _) if id.starts_with("am") => LinkKind::Collection,
        _ => LinkKind::Video,
    }
}

fn classify_soundcloud(url: &Url, host: &str) -> LinkKind {
    let segs = segments(url);
    if host == "on.soundcloud.com" || host == "snd.sc" {
        return LinkKind::Video;
    }
    match segs.as_slice() {
        [] => LinkKind::Collection,
        [first, ..] if first == "discover" => LinkKind::Collection,
        [_user] => LinkKind::Collection,
        [_user, second, ..]
            if matches!(
                second.as_str(),
                "sets" | "tracks" | "likes" | "reposts" | "albums" | "popular-tracks" | "toptracks"
            ) =>
        {
            LinkKind::Collection
        }
        _ => LinkKind::Video,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn youtube_video_in_playlist() {
        let l = parse_link("https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=PL123&index=3").unwrap();
        assert_eq!(l.site, Site::YouTube);
        assert_eq!(
            l.kind,
            LinkKind::VideoInPlaylist { video_id: "dQw4w9WgXcQ".into(), playlist_id: "PL123".into() }
        );
        assert_eq!(l.video_only_url(), "https://www.youtube.com/watch?v=dQw4w9WgXcQ");
        assert_eq!(l.playlist_url(), "https://www.youtube.com/playlist?list=PL123");
    }

    #[test]
    fn youtube_short_link_and_plain_video() {
        assert_eq!(parse_link("youtu.be/abc123").unwrap().kind, LinkKind::Video);
        assert!(matches!(
            parse_link("https://youtu.be/abc123?list=PLx").unwrap().kind,
            LinkKind::VideoInPlaylist { .. }
        ));
        assert_eq!(parse_link("https://m.youtube.com/watch?v=abc").unwrap().kind, LinkKind::Video);
        assert_eq!(parse_link("https://www.youtube.com/shorts/abc").unwrap().kind, LinkKind::Video);
    }

    #[test]
    fn youtube_collections() {
        assert_eq!(
            parse_link("https://www.youtube.com/playlist?list=PL1").unwrap().kind,
            LinkKind::Collection
        );
        let ch = parse_link("https://www.youtube.com/@SomeChannel").unwrap();
        assert_eq!(ch.kind, LinkKind::Collection);
        assert_eq!(ch.url, "https://www.youtube.com/@SomeChannel/videos");
        let ch2 = parse_link("https://www.youtube.com/@SomeChannel/shorts").unwrap();
        assert_eq!(ch2.url, "https://www.youtube.com/@SomeChannel/shorts");
        let ch3 = parse_link("https://www.youtube.com/channel/UC123").unwrap();
        assert_eq!(ch3.url, "https://www.youtube.com/channel/UC123/videos");
    }

    #[test]
    fn niconico() {
        assert_eq!(parse_link("https://www.nicovideo.jp/watch/sm9").unwrap().kind, LinkKind::Video);
        assert_eq!(parse_link("https://nico.ms/sm9").unwrap().kind, LinkKind::Video);
        assert_eq!(
            parse_link("https://www.nicovideo.jp/user/123/mylist/456").unwrap().kind,
            LinkKind::Collection
        );
        assert_eq!(
            parse_link("https://www.nicovideo.jp/series/789").unwrap().kind,
            LinkKind::Collection
        );
    }

    #[test]
    fn bilibili() {
        let l = parse_link("https://www.bilibili.com/video/BV1xx411c7mD?p=2").unwrap();
        assert_eq!((l.site, l.kind), (Site::Bilibili, LinkKind::Video));
        assert_eq!(parse_link("https://b23.tv/abcd").unwrap().kind, LinkKind::Video);
        assert_eq!(
            parse_link("https://space.bilibili.com/123/favlist?fid=1").unwrap().kind,
            LinkKind::Collection
        );
        assert_eq!(
            parse_link("https://www.bilibili.com/bangumi/play/ss1234").unwrap().kind,
            LinkKind::Collection
        );
        assert_eq!(
            parse_link("https://www.bilibili.com/bangumi/play/ep1234").unwrap().kind,
            LinkKind::Video
        );
    }

    #[test]
    fn soundcloud() {
        assert_eq!(
            parse_link("https://soundcloud.com/artist/track-name").unwrap().kind,
            LinkKind::Video
        );
        assert_eq!(
            parse_link("https://soundcloud.com/artist/sets/album").unwrap().kind,
            LinkKind::Collection
        );
        assert_eq!(parse_link("https://soundcloud.com/artist").unwrap().kind, LinkKind::Collection);
        assert_eq!(
            parse_link("https://soundcloud.com/artist/likes").unwrap().kind,
            LinkKind::Collection
        );
    }

    #[test]
    fn rejects_unsupported_and_garbage() {
        assert!(matches!(parse_link("https://vimeo.com/123"), Err(LinkError::Unsupported(_))));
        assert!(matches!(parse_link("https://www.tiktok.com/@x/video/1"), Err(LinkError::Unsupported(_))));
        assert_eq!(parse_link("hello world"), Err(LinkError::NotAUrl));
        assert_eq!(parse_link(""), Err(LinkError::NotAUrl));
        assert_eq!(parse_link("ftp://youtube.com/x"), Err(LinkError::NotAUrl));
        // Lookalike domains must not match.
        assert!(matches!(parse_link("https://notyoutube.com/watch?v=1"), Err(LinkError::Unsupported(_))));
    }

    #[test]
    fn extracts_link_from_surrounding_text() {
        let l = parse_link("check this out https://www.nicovideo.jp/watch/sm9 !!").unwrap();
        assert_eq!(l.site, Site::Niconico);
    }
}
