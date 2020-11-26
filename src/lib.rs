use std::fmt;

use indexmap::IndexMap;
use nom::lib::std::fmt::Formatter;
use nom::Finish;

use crate::error::M3U8ParserError;

pub mod error;

#[derive(Debug)]
pub struct Playlist<'a> {
    pub ext_infos: Vec<PlaylistExtInfo<'a>>,
}

impl<'a> Playlist<'a> {
    #[allow(unused)]
    pub fn playlist_names(&self) -> Vec<&str> {
        self.ext_infos
            .iter()
            .filter(|e| e.ext_type == PlaylistExtType::Media)
            .map(|m| *m.attributes.get("NAME").unwrap_or(&"Unknown"))
            .collect()
    }

    pub fn first_playlist_link(&self) -> Option<&str> {
        self.ext_infos
            .iter()
            .filter(|e| e.ext_type == PlaylistExtType::StreamInf)
            .collect::<Vec<_>>()
            .get(0)
            .and_then(|p| p.attributes.get("URI").copied())
    }

    #[allow(unused)]
    pub fn playlist_link(&self, name: &str) -> Option<&str> {
        let playlist_group_id = self
            .ext_infos
            .iter()
            .filter(|e| e.ext_type == PlaylistExtType::Media)
            .find(|e| {
                if let Some(n) = e.attributes.get("NAME") {
                    *n == name
                } else {
                    false
                }
            })
            .and_then(|e| e.attributes.get("GROUP-ID"))?;

        self.ext_infos
            .iter()
            .filter(|e| e.ext_type == PlaylistExtType::StreamInf)
            .find(|e| {
                if let Some(v) = e.attributes.get("VIDEO") {
                    v == playlist_group_id
                } else {
                    false
                }
            })
            .and_then(|e| e.attributes.get("URI").copied())
    }
}

#[derive(Debug)]
pub struct PlaylistExtInfo<'a> {
    pub ext_type: PlaylistExtType,
    pub attributes: IndexMap<&'a str, &'a str>,
}

#[derive(Debug, PartialEq)]
pub enum PlaylistExtType {
    Media,
    StreamInf,
    Unknown(String),
}

impl<T: AsRef<str>> From<T> for PlaylistExtType {
    fn from(s: T) -> Self {
        let s = s.as_ref().trim_start_matches("-X-").to_owned();

        match s.as_str() {
            "MEDIA" => Self::Media,
            "STREAM-INF" => Self::StreamInf,
            _ => Self::Unknown(s),
        }
    }
}

fn not_newline(i: &str) -> nom::IResult<&str, &str> {
    nom::bytes::complete::is_not("\n")(i)
}

fn ext_identifier(i: &str) -> nom::IResult<&str, &str, M3U8ParserError<&str>> {
    nom::bytes::complete::tag("#EXTM3U\n")(i)
}

fn ext_type<'a, T>(i: &'a str) -> nom::IResult<&str, T>
where
    T: From<&'a str>,
{
    let (i, ext_type_str) = nom::sequence::preceded(
        nom::bytes::complete::tag("#EXT"),
        nom::branch::alt((
            nom::sequence::terminated(
                nom::bytes::complete::is_not(":"),
                nom::character::complete::char(':'),
            ),
            not_newline,
        )),
    )(i)?;

    Ok((i, T::from(ext_type_str)))
}

fn comma_sep_pair(i: &str) -> nom::IResult<&str, (&str, &str)> {
    nom::sequence::separated_pair(
        nom::bytes::complete::is_not(","),
        nom::bytes::complete::tag(","),
        not_newline,
    )(i)
}

fn read_quoted_attribute(i: &str) -> nom::IResult<&str, &str> {
    nom::combinator::recognize(nom::sequence::delimited(
        nom::character::complete::char('\"'),
        nom::bytes::complete::is_not("\""),
        nom::character::complete::char('\"'),
    ))(i)
}

fn attribute_key_val(i: &str) -> nom::IResult<&str, (&str, &str)> {
    nom::sequence::separated_pair(
        nom::bytes::complete::is_not("="),
        nom::bytes::complete::tag("="),
        nom::branch::alt((read_quoted_attribute, nom::bytes::complete::is_not(","))),
    )(i)
}

fn attributes(i: &str) -> nom::IResult<&str, IndexMap<&str, &str>> {
    let (i, attributes_vec) =
        nom::multi::separated_list0(nom::character::complete::char(','), attribute_key_val)(i)?;

    let attributes_map = attributes_vec
        .into_iter()
        .fold(IndexMap::new(), |mut curr, next| {
            curr.insert(next.0, next.1);
            curr
        });

    Ok((i, attributes_map))
}

pub fn read_playlist(data: &str) -> Result<Playlist, M3U8ParserError<&str>> {
    let (i, _) = ext_identifier(&data).finish()?;

    let mut remaining_lines = i.lines();

    let mut ext_infos = Vec::new();

    while let Some(line) = remaining_lines.next() {
        let (i, ext_type) = ext_type::<PlaylistExtType>(line).finish()?;
        let (_, mut attributes) = attributes(i).finish()?;

        if ext_type == PlaylistExtType::StreamInf {
            if let Some(stream_inf_location) = remaining_lines.next() {
                attributes.insert("URI", stream_inf_location);
            }
        }

        ext_infos.push(PlaylistExtInfo {
            ext_type,
            attributes,
        })
    }

    Ok(Playlist { ext_infos })
}

fn rejoin_attributes(attributes: &IndexMap<&str, &str>) -> String {
    attributes
        .iter()
        .map(|(k, v)| {
            if *k == "UNKNOWN" {
                (*v).to_owned()
            } else {
                format!("{}={}", k, v)
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

#[derive(Debug)]
pub struct MediaList<'a> {
    pub version: u8,
    pub target_duration: u8,
    pub media_sequence: u32,
    pub media_segments: Vec<MediaSegment>,
    pub ext_infos: Vec<MediaExtInfo<'a>>,
}

impl<'a> MediaList<'a> {
    pub fn save<T: std::io::Write>(&self, output: &mut T) -> Result<(), M3U8ParserError<()>> {
        let ext_tag = "#EXT";

        writeln!(output, "#EXTM3U")?;
        writeln!(output, "{}-X-VERSION:{}", ext_tag, self.version)?;
        writeln!(
            output,
            "{}-X-TARGETDURATION:{}",
            ext_tag, self.target_duration
        )?;
        writeln!(
            output,
            "{}-X-MEDIA-SEQUENCE:{}",
            ext_tag, self.media_sequence
        )?;

        for ext_info in &self.ext_infos {
            match &ext_info.ext_type {
                MediaExtType::Inf | MediaExtType::ProgramDateTime => (),
                MediaExtType::Discontinuity => {
                    writeln!(output, "{}-X-{}", ext_tag, ext_info.ext_type)?;
                }
                _ => {
                    writeln!(
                        output,
                        "{}-X-{}:{}",
                        ext_tag,
                        ext_info.ext_type,
                        rejoin_attributes(&ext_info.attributes)
                    )?;
                }
            }
        }

        for segment in &self.media_segments {
            if let Some(ref program_date_time) = segment.program_date_time {
                writeln!(
                    output,
                    "{}-X-{}:{}",
                    ext_tag,
                    MediaExtType::ProgramDateTime,
                    program_date_time
                )?;
            }

            writeln!(
                output,
                "{}{}:{:.3},{}\n{}",
                ext_tag,
                MediaExtType::Inf,
                segment.duration,
                segment.title.as_ref().unwrap_or(&"".to_owned()),
                segment.uri,
            )?;
        }

        Ok(())
    }
}

#[derive(Debug)]
pub struct MediaExtInfo<'a> {
    pub ext_type: MediaExtType,
    pub attributes: IndexMap<&'a str, &'a str>,
}

#[derive(Debug, PartialEq)]
pub enum MediaExtType {
    Version,
    TargetDuration,
    MediaSequence,
    DateRange,
    Discontinuity,
    Inf,
    ProgramDateTime,
    Unknown(String),
}

#[derive(Debug, PartialEq)]
pub struct MediaSegment {
    pub duration: f64,
    pub title: Option<String>,
    pub uri: String,
    pub program_date_time: Option<String>,
}

impl fmt::Display for MediaExtType {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            MediaExtType::Version => write!(f, "VERSION"),
            MediaExtType::TargetDuration => write!(f, "TARGETDURATION"),
            MediaExtType::MediaSequence => write!(f, "MEDIA-SEQUENCE"),
            MediaExtType::DateRange => write!(f, "DATERANGE"),
            MediaExtType::Discontinuity => write!(f, "DISCONTINUITY"),
            MediaExtType::Inf => write!(f, "INF"),
            MediaExtType::ProgramDateTime => write!(f, "PROGRAM-DATE-TIME"),
            MediaExtType::Unknown(ext_type) => write!(f, "{}", ext_type),
        }
    }
}

impl<T: AsRef<str>> From<T> for MediaExtType {
    fn from(s: T) -> Self {
        let s = s.as_ref().trim_start_matches("-X-").to_owned();

        match s.as_str() {
            "VERSION" => Self::Version,
            "TARGETDURATION" => Self::TargetDuration,
            "MEDIA-SEQUENCE" => Self::MediaSequence,
            "DATERANGE" => Self::DateRange,
            "DISCONTINUITY" => Self::Discontinuity,
            "INF" => Self::Inf,
            "PROGRAM-DATE-TIME" => Self::ProgramDateTime,
            _ => Self::Unknown(s),
        }
    }
}

pub fn read_media_list(data: &str) -> Result<MediaList, M3U8ParserError<&str>> {
    let (i, _) = ext_identifier(&data).finish()?;

    let mut remaining_lines = i.lines();

    let mut media_segments = Vec::new();
    let mut ext_infos = Vec::new();

    let mut version = 0;
    let mut target_duration = 0;
    let mut media_sequence = 0;

    let mut current_program_date_time = None;

    while let Some(line) = remaining_lines.next() {
        let (i, ext_type) = ext_type::<MediaExtType>(line).finish()?;

        match ext_type {
            MediaExtType::DateRange => {
                let (_, attributes) = attributes(i).finish()?;

                ext_infos.push(MediaExtInfo {
                    ext_type,
                    attributes,
                })
            }
            MediaExtType::Unknown(_) => {
                let (_, unknown_str) = not_newline(i).finish()?;

                let mut attributes = IndexMap::new();

                attributes.insert("UNKNOWN", unknown_str);

                ext_infos.push(MediaExtInfo {
                    ext_type,
                    attributes,
                })
            }
            MediaExtType::ProgramDateTime => {
                let (_, program_date_time) = not_newline(i).finish()?;

                current_program_date_time = Some(program_date_time.to_owned());
            }
            MediaExtType::Inf => {
                let (_, (duration, tit)) = comma_sep_pair(i).finish()?;

                if let Some(stream_inf_location) = remaining_lines.next() {
                    let duration = duration.parse::<f64>()?;
                    let mut title = None;

                    if tit != "" {
                        title = Some(tit.to_owned());
                    }

                    let uri = stream_inf_location.to_owned();

                    media_segments.push(MediaSegment {
                        duration,
                        title,
                        uri,
                        program_date_time: current_program_date_time.take(),
                    })
                }
            }
            MediaExtType::Version => {
                let (_, ver) = not_newline(i).finish()?;
                version = ver.parse::<u8>()?;
            }
            MediaExtType::TargetDuration => {
                let (_, dur) = not_newline(i).finish()?;
                target_duration = dur.parse::<u8>()?;
            }
            MediaExtType::MediaSequence => {
                let (_, media_seq) = not_newline(i).finish()?;
                media_sequence = media_seq.parse::<u32>()?;
            }
            MediaExtType::Discontinuity => {
                let attributes = IndexMap::new();

                ext_infos.push(MediaExtInfo {
                    ext_type,
                    attributes,
                })
            }
        }
    }

    Ok(MediaList {
        version,
        target_duration,
        media_sequence,
        media_segments,
        ext_infos,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::fs::OpenOptions;

    use super::*;

    #[test]
    fn test_read_playlist() {
        let test_file = fs::read_to_string("./test_m3u8_files/playlist.m3u8").unwrap();

        let playlist = read_playlist(&test_file).unwrap();
        let first_ext_info = playlist.ext_infos.get(0).unwrap();

        assert_eq!(
            first_ext_info.ext_type,
            PlaylistExtType::Unknown("TWITCH-INFO".to_owned())
        );
        assert_eq!(
            first_ext_info.attributes.get("MANIFEST-NODE-TYPE"),
            Some(&"\"weaver_cluster\"")
        );
        assert_eq!(
            first_ext_info.attributes.get("BROADCAST-ID"),
            Some(&"\"40032678348\"")
        );
        assert_eq!(
            first_ext_info.attributes.get("USER-COUNTRY"),
            Some(&"\"GB\"")
        );

        let media_1080p = playlist.ext_infos.get(1).unwrap();
        assert_eq!(media_1080p.ext_type, PlaylistExtType::Media);
        assert_eq!(media_1080p.attributes.get("TYPE"), Some(&"VIDEO"));
        assert_eq!(
            media_1080p.attributes.get("NAME"),
            Some(&"\"1080p60 (source)\"")
        );
        assert_eq!(media_1080p.attributes.get("GROUP-ID"), Some(&"\"chunked\""));

        let stream_inf_1080p = playlist.ext_infos.get(2).unwrap();

        assert_eq!(stream_inf_1080p.ext_type, PlaylistExtType::StreamInf);
        assert_eq!(
            stream_inf_1080p.attributes.get("RESOLUTION"),
            Some(&"1920x1080")
        );
        assert_eq!(
            stream_inf_1080p.attributes.get("VIDEO"),
            Some(&"\"chunked\"")
        );
    }

    #[test]
    fn test_read_media_list() {
        let test_file = fs::read_to_string("./test_m3u8_files/media_list.m3u8").unwrap();

        let media_list = read_media_list(&test_file).unwrap();

        assert_eq!(media_list.version, 3);
        assert_eq!(media_list.target_duration, 6);
        assert_eq!(media_list.media_sequence, 4508);

        let first_ext_info = media_list.ext_infos.get(0).unwrap();

        assert_eq!(
            first_ext_info.ext_type,
            MediaExtType::Unknown("TWITCH-ELAPSED-SECS".to_owned())
        );
        assert_eq!(first_ext_info.attributes.get("UNKNOWN"), Some(&"9016.000"));

        let third_ext_info = media_list.ext_infos.get(2).unwrap();
        assert_eq!(third_ext_info.ext_type, MediaExtType::DateRange,);
        assert_eq!(
            third_ext_info.attributes.get("CLASS"),
            Some(&"\"twitch-stream-source\"")
        );
        assert_eq!(
            third_ext_info.attributes.get("START-DATE"),
            Some(&"\"2020-11-18T14:12:40.956Z\"")
        );
        assert_eq!(
            third_ext_info.attributes.get("ID"),
            Some(&"\"source-1605708760\"")
        );
        assert_eq!(third_ext_info.attributes.get("END-ON-NEXT"), Some(&"YES"));
        assert_eq!(
            third_ext_info.attributes.get("X-TV-TWITCH-STREAM-SOURCE"),
            Some(&"\"live\"")
        );

        let segment_info = media_list.media_segments.get(0).unwrap();

        assert_eq!(segment_info.duration, 2.000);
        assert_eq!(segment_info.title, Some("live".to_owned()));
        assert_eq!(
            segment_info.uri, "https://video-edge-c6d608.lhr04.abs.hls.ttvnw.net/v1/segment/CrsEzRu838bWUuW9RMJzmO0XYcHHcASBvuHgb5RLq5NVkkDzcl8Fuk-cSTNUnlimYBLSWEbLD-wh6NRTc5dT8O4n-gVAHGHWNDmeFB7fU6uLudKXTvZ6TRUGWGtUg6nEhhyeQpbrxOD7gaK06BzPgGi4tt4N0MCRLxPRFu3a_XnkjkDZ6_Z9F848pp1IVLbogJuKMLeKt-hf4O_zRlI1XEgKM4XzlEhspGfkhoYZ-_L0px96CRUp7rKsYMSLZ6i_KmdXzT7NDm9x76UBKiIUenIf-N6AAgb8BZEDXJ5Vgwi8YxXsFcIaL5W35XFmP4iS9dIwEbneVU-Rn5bIrYxAuCLxq5xRdB-nXUsKr1Vhki11FrOg4Tsk-LYdTsS2X3-Z8NbJytbs8nPzRo3MpQlyWQIw4WshxZqZwQgb6g-jk85eAG6wP4tSCgcbgo4CeADFQIn33ynjYU1VkF7VzQkGNT1sj1HOMtCrET4aDTU8AF9CN_keWBUX7uOjMEpWJDs44GhfjHHmQqQ1kqmmEzPBxYRchWUrhLYNcDQHmJyFDpahCBXl5gKgHK-z7hBaUW8X8STjf7RhtDt9lFgy2b3SL_AVYLXbyMNs6jFAXqOEDe0o5MC0FAXBVgXNSBHEcqkVp0kiu5kL5jGBm37xeYpXxukrcCJ3BUA6w8SQQ9vjMHVgPzRbIL72NGca42z7e0GYGnXGaxqVj08aJtsnI1_U3chdp5k22pSCMpGpuU-lsLRodOomIdrmgkumvN0xXRIQpQ7h-p7kP9_pganXO5M5jhoMer127dU6GU0FarSp.ts"
        );
    }

    #[test]
    fn test_save_media_list() {
        let curr_stream =
            fs::read_to_string("./test_m3u8_files/twitch_ad_media_list.m3u8").unwrap();

        let mut media_list = read_media_list(&curr_stream).unwrap();

        media_list
            .media_segments
            .retain(|m| !m.title.as_deref().unwrap_or("").starts_with("Amazon"));

        media_list.ext_infos.retain(|e| {
            e.attributes.get("CLASS").unwrap_or(&"") != &"\"twitch-ad-quartile\""
                && e.attributes.get("CLASS").unwrap_or(&"") != &"\"twitch-stitched-ad\""
                && !e
                    .attributes
                    .get("X-TV-TWITCH-STREAM-SOURCE")
                    .unwrap_or(&"")
                    .starts_with(&"\"Amazon")
                && e.ext_type != MediaExtType::Discontinuity
                && e.ext_type != MediaExtType::Unknown("START".to_owned())
        });

        let mut outfile = OpenOptions::new()
            .truncate(true)
            .write(true)
            .open("./test_m3u8_files/save_twitch_ad_media_list.m3u8")
            .unwrap();

        media_list.save(&mut outfile).unwrap();
    }

    #[test]
    fn test_ext_identifier() {
        assert_eq!(ext_identifier("#EXTM3U\n"), Ok(("", "#EXTM3U\n")));
        assert_eq!(
            ext_identifier("EXTM3U"),
            Err(nom::Err::Error(M3U8ParserError::NomError(
                "EXTM3U",
                nom::error::ErrorKind::Tag,
            )))
        );
    }

    #[test]
    fn test_attribute_key_val() {
        assert_eq!(
            attribute_key_val("TYPE=VIDEO"),
            (Ok(("", ("TYPE", "VIDEO"))))
        );
        assert_eq!(
            attribute_key_val(r#"GROUP-ID="720p60""#),
            (Ok(("", ("GROUP-ID", "\"720p60\""))))
        );
        assert_eq!(
            attribute_key_val("CODECS=\"avc1.4D401F,mp4a.40.2\""),
            (Ok(("", ("CODECS", "\"avc1.4D401F,mp4a.40.2\""))))
        );
    }

    #[test]
    fn test_attributes() {
        let mut attributes_map = IndexMap::new();

        attributes_map.insert("TYPE", "VIDEO");
        attributes_map.insert("GROUP-ID", "\"720p60\"");
        attributes_map.insert("NAME", "\"720p60\"");
        attributes_map.insert("AUTOSELECT", "YES");
        attributes_map.insert("DEFAULT", "YES");

        assert_eq!(
            attributes(r#"TYPE=VIDEO,GROUP-ID="720p60",NAME="720p60",AUTOSELECT=YES,DEFAULT=YES"#),
            Ok(("", attributes_map))
        );
    }

    #[test]
    fn test_rejoin_attributes() {
        let mut attributes_map = IndexMap::new();

        attributes_map.insert("TYPE", "VIDEO");
        attributes_map.insert("GROUP-ID", "\"720p60\"");
        attributes_map.insert("NAME", "\"720p60\"");
        attributes_map.insert("AUTOSELECT", "YES");
        attributes_map.insert("DEFAULT", "YES");

        assert_eq!(
            rejoin_attributes(&attributes_map),
            r#"TYPE=VIDEO,GROUP-ID="720p60",NAME="720p60",AUTOSELECT=YES,DEFAULT=YES"#.to_owned()
        );

        let mut attributes_map_unknown = IndexMap::new();

        attributes_map_unknown.insert("UNKNOWN", "33064.367");

        assert_eq!(
            rejoin_attributes(&attributes_map_unknown),
            "33064.367".to_owned()
        );
    }

    #[test]
    fn test_ext_type() {
        assert_eq!(
            ext_type("#EXT-X-TWITCH-INFO:NODE="),
            Ok(("NODE=", PlaylistExtType::Unknown("TWITCH-INFO".to_owned())))
        );
        assert_eq!(
            ext_type("#EXT-X-MEDIA:TYPE=VIDEO"),
            Ok(("TYPE=VIDEO", PlaylistExtType::Media))
        );
        assert_eq!(
            ext_type("#EXT-X-STREAM-INF:BANDWIDTH=1430857,RESOLUTION=852x480\nhttps://video-weaver.lhr04.hls.ttvnw.net/v1/playlist/abc123.m3u8"),
            Ok(("BANDWIDTH=1430857,RESOLUTION=852x480\nhttps://video-weaver.lhr04.hls.ttvnw.net/v1/playlist/abc123.m3u8", PlaylistExtType::StreamInf))
        );
        assert_eq!(
            ext_type::<PlaylistExtType>("#EXT-XBANDWIDTH=630000"),
            Ok((
                "",
                PlaylistExtType::Unknown("-XBANDWIDTH=630000".to_owned())
            ))
        );
    }
}
