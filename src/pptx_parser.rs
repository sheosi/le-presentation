//! PPTX Parser - Extracts animation and transition info from PowerPoint files

use roxmltree::Document;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use zip::read::ZipArchive;

/// Parsed information about a PPTX presentation
#[derive(Debug)]
pub struct PptxInfo {
    pub slide_count: u32,
    pub slides: Vec<SlideInfo>,
}

/// Information about a single slide
#[derive(Debug, Clone)]
pub struct SlideInfo {
    pub slide_number: u32,
    pub animations: Vec<Animation>,
    pub transition: Option<Transition>,
    pub embedded_media: Vec<EmbeddedMedia>,
}

/// Embedded media (audio/video) in a slide
#[derive(Debug, Clone)]
pub struct EmbeddedMedia {
    pub media_type: MediaType,
    pub filename: String,
    pub content_type: String,
}

/// Type of embedded media
#[derive(Debug, Clone)]
pub enum MediaType {
    Audio,
    Video,
}

/// Animation information
#[derive(Debug, Clone)]
pub struct Animation {
    pub animation_type: AnimationType,
    pub subtype: Option<String>,
    pub duration_ms: u32,
    pub trigger: AnimationTrigger,
    pub target_element: Option<String>,
    pub delay_ms: u32,
}

/// Types of animations in PowerPoint
#[derive(Debug, Clone)]
pub enum AnimationType {
    Appear,
    Fade,
    Fly,
    Float,
    Split,
    Wipe,
    Shape,
    Wheel,
    RandomBars,
    GrowAndShrink,
    Zoom,
    Swivel,
    Bounce,
    Credits,
    None,
    Other(String),
}

impl From<&str> for AnimationType {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "appear" => AnimationType::Appear,
            "fade" => AnimationType::Fade,
            "fly" => AnimationType::Fly,
            "float" => AnimationType::Float,
            "split" => AnimationType::Split,
            "wipe" => AnimationType::Wipe,
            "shape" => AnimationType::Shape,
            "wheel" => AnimationType::Wheel,
            "randombars" => AnimationType::RandomBars,
            "growandshrink" => AnimationType::GrowAndShrink,
            "zoom" => AnimationType::Zoom,
            "swivel" => AnimationType::Swivel,
            "bounce" => AnimationType::Bounce,
            "credits" => AnimationType::Credits,
            "" | "none" => AnimationType::None,
            other => AnimationType::Other(other.to_string()),
        }
    }
}

/// Animation trigger types
#[derive(Debug, Clone)]
pub enum AnimationTrigger {
    OnClick,
    WithPrevious,
    AfterPrevious,
    AfterDuration(u32),
}

impl Default for AnimationTrigger {
    fn default() -> Self {
        AnimationTrigger::OnClick
    }
}

/// Direction for transitions that support it
#[derive(Debug, Clone, PartialEq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Center,
}

impl From<&str> for Direction {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "l" | "left" => Direction::Left,
            "r" | "right" => Direction::Right,
            "u" | "up" => Direction::Up,
            "d" | "down" => Direction::Down,
            "tl" | "topleft" | "top_left" => Direction::TopLeft,
            "tr" | "topright" | "top_right" => Direction::TopRight,
            "bl" | "bottomleft" | "bottom_left" => Direction::BottomLeft,
            "br" | "bottomright" | "bottom_right" => Direction::BottomRight,
            "c" | "center" => Direction::Center,
            _ => Direction::Right,
        }
    }
}

/// Slide transition information
#[derive(Debug, Clone)]
pub struct Transition {
    pub transition_type: TransitionType,
    pub duration_ms: u32,
}

/// Slide transition types in PowerPoint
/// Direction is embedded only for transitions that support it
#[derive(Debug, Clone, PartialEq)]
pub enum TransitionType {
    // No direction needed
    None,
    Fade,
    Cut,
    RandomBars,
    Newsflash,
    Vortex,
    Shred,
    Switch,
    Flip,
    Gallery,
    Ripple,
    Honeycomb,
    Cube,
    Box,
    Accordion,
    Frame,
    Glitter,
    Airplane,
    FerrisWheel,
    ConveyorBelt,
    Clock,
    Wheel,
    Comb,
    Morph,
    ZoomCenter,
    Rotate,

    // With direction
    Push(Direction),
    Cover(Direction),
    Uncover(Direction),
    PeelOff(Direction),
    PageCurl(Direction),
    Wipe(Direction),
    Split(Direction),
    Reveal(Direction),
    Doors(Direction),
    Window(Direction),
    Pan(Direction),
    Zoom(Direction),

    // Fallthrough for unknown/custom transitions
    Other(String),
}

impl TransitionType {
    /// Parse transition type and direction from string attributes
    pub fn parse(type_attr: Option<&str>, dir_attr: Option<&str>) -> Self {
        let type_str = type_attr.unwrap_or("none").to_lowercase();
        let dir = dir_attr.map(Direction::from);

        match type_str.as_str() {
            // No direction needed
            "" | "none" => TransitionType::None,
            "fade" => TransitionType::Fade,
            "cut" | "cutthroughblack" => TransitionType::Cut,
            "randombars" | "random_bars" => TransitionType::RandomBars,
            "newsflash" => TransitionType::Newsflash,
            "vortex" => TransitionType::Vortex,
            "shred" => TransitionType::Shred,
            "switch" => TransitionType::Switch,
            "flip" => TransitionType::Flip,
            "gallery" => TransitionType::Gallery,
            "ripple" => TransitionType::Ripple,
            "honeycomb" => TransitionType::Honeycomb,
            "cube" => TransitionType::Cube,
            "box" => TransitionType::Box,
            "accordion" => TransitionType::Accordion,
            "frame" => TransitionType::Frame,
            "glitter" => TransitionType::Glitter,
            "airplane" => TransitionType::Airplane,
            "ferriswheel" | "ferris_wheel" => TransitionType::FerrisWheel,
            "conveyorbelt" | "conveyor_belt" => TransitionType::ConveyorBelt,
            "clock" => TransitionType::Clock,
            "wheel" => TransitionType::Wheel,
            "comb" => TransitionType::Comb,
            "morph" => TransitionType::Morph,
            "rotate" => TransitionType::Rotate,

            // With direction - use provided direction or default
            "push" | "pushthroughblack" => TransitionType::Push(dir.unwrap_or(Direction::Right)),
            "cover" => TransitionType::Cover(dir.unwrap_or(Direction::Right)),
            "uncover" | "pull" => TransitionType::Uncover(dir.unwrap_or(Direction::Right)),
            "peeloff" | "peel_off" => TransitionType::PeelOff(dir.unwrap_or(Direction::Right)),
            "pagecurl" | "page_curl" => TransitionType::PageCurl(dir.unwrap_or(Direction::Right)),
            "wipe" => TransitionType::Wipe(dir.unwrap_or(Direction::Right)),
            "split" => TransitionType::Split(dir.unwrap_or(Direction::Right)),
            "reveal" => TransitionType::Reveal(dir.unwrap_or(Direction::Right)),
            "doors" => TransitionType::Doors(dir.unwrap_or(Direction::Right)),
            "window" => TransitionType::Window(dir.unwrap_or(Direction::Right)),
            "pan" => TransitionType::Pan(dir.unwrap_or(Direction::Right)),
            "zoom" => {
                if let Some(Direction::Center) = dir {
                    TransitionType::ZoomCenter
                } else {
                    TransitionType::Zoom(dir.unwrap_or(Direction::Center))
                }
            }

            // Unknown
            other => TransitionType::Other(other.to_string()),
        }
    }

    /// Returns true if this transition supports direction
    pub fn has_direction(&self) -> bool {
        matches!(
            self,
            TransitionType::Push(_)
                | TransitionType::Cover(_)
                | TransitionType::Uncover(_)
                | TransitionType::PeelOff(_)
                | TransitionType::PageCurl(_)
                | TransitionType::Wipe(_)
                | TransitionType::Split(_)
                | TransitionType::Reveal(_)
                | TransitionType::Doors(_)
                | TransitionType::Window(_)
                | TransitionType::Pan(_)
                | TransitionType::Zoom(_)
        )
    }

    /// Get direction if this transition supports it
    pub fn direction(&self) -> Option<&Direction> {
        match self {
            TransitionType::Push(d)
            | TransitionType::Cover(d)
            | TransitionType::Uncover(d)
            | TransitionType::PeelOff(d)
            | TransitionType::PageCurl(d)
            | TransitionType::Wipe(d)
            | TransitionType::Split(d)
            | TransitionType::Reveal(d)
            | TransitionType::Doors(d)
            | TransitionType::Window(d)
            | TransitionType::Pan(d)
            | TransitionType::Zoom(d) => Some(d),
            _ => None,
        }
    }
}

/// Error type for PPTX parsing
#[derive(Debug)]
pub enum PptxParseError {
    Io(std::io::Error),
    Zip(zip::result::ZipError),
    XmlParse(String),
    MissingFile(String),
}

impl std::fmt::Display for PptxParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PptxParseError::Io(e) => write!(f, "IO error: {}", e),
            PptxParseError::Zip(e) => write!(f, "ZIP error: {}", e),
            PptxParseError::XmlParse(e) => write!(f, "XML parse error: {}", e),
            PptxParseError::MissingFile(e) => write!(f, "Missing file: {}", e),
        }
    }
}

impl std::error::Error for PptxParseError {}

impl From<std::io::Error> for PptxParseError {
    fn from(e: std::io::Error) -> Self {
        PptxParseError::Io(e)
    }
}

impl From<zip::result::ZipError> for PptxParseError {
    fn from(e: zip::result::ZipError) -> Self {
        PptxParseError::Zip(e)
    }
}

/// Main PPTX parser
pub struct PptxParser;

impl PptxParser {
    /// Parse a PPTX file and extract all information
    pub fn parse(path: &Path) -> Result<PptxInfo, PptxParseError> {
        let file = File::open(path)?;
        let mut archive = ZipArchive::new(file)?;

        let slide_count = Self::get_slide_count(&mut archive)?;
        let mut slides = Vec::new();

        for slide_num in 1..=slide_count {
            let slide_info = Self::parse_slide(&mut archive, slide_num)?;
            slides.push(slide_info);
        }

        Ok(PptxInfo {
            slide_count,
            slides,
        })
    }

    fn get_slide_count(archive: &mut ZipArchive<File>) -> Result<u32, PptxParseError> {
        let mut presentation_xml = String::new();
        let mut file = archive
            .by_name("ppt/presentation.xml")
            .map_err(|_| PptxParseError::MissingFile("ppt/presentation.xml".to_string()))?;
        file.read_to_string(&mut presentation_xml)?;

        let doc = Document::parse(&presentation_xml)
            .map_err(|e| PptxParseError::XmlParse(e.to_string()))?;

        let mut count = 0u32;
        for node in doc.descendants() {
            if node.tag_name().name() == "sldId" {
                count += 1;
            }
        }
        Ok(count)
    }

    fn parse_slide(
        archive: &mut ZipArchive<File>,
        slide_num: u32,
    ) -> Result<SlideInfo, PptxParseError> {
        let slide_xml_path = format!("ppt/slides/slide{}.xml", slide_num);

        let mut slide_info = SlideInfo {
            slide_number: slide_num,
            animations: Vec::new(),
            transition: None,
            embedded_media: Vec::new(),
        };

        // Parse slide relationships first
        let slide_rels = Self::parse_slide_rels(archive, slide_num)?;

        let mut slide_xml = String::new();
        if let Ok(mut slide_file) = archive.by_name(&slide_xml_path) {
            slide_file.read_to_string(&mut slide_xml)?;
            slide_info.animations = Self::parse_animations(&slide_xml)?;
            slide_info.transition = Self::parse_transitions(&slide_xml)?;
            slide_info.embedded_media = Self::parse_embedded_media(&slide_xml, &slide_rels)?;
        }

        Ok(slide_info)
    }

    fn parse_animations(xml: &str) -> Result<Vec<Animation>, PptxParseError> {
        let doc = Document::parse(xml).map_err(|e| PptxParseError::XmlParse(e.to_string()))?;
        let mut animations = Vec::new();

        for node in doc.descendants() {
            let tag_name = node.tag_name().name();
            match tag_name {
                "anim" | "animMotion" | "animScale" | "animRot" | "animClr" | "set" => {
                    if let Some(anim) = Self::extract_animation(&node, tag_name) {
                        animations.push(anim);
                    }
                }
                _ => {}
            }
        }
        Self::find_animation_nodes(&doc.root_element(), &mut animations);
        Ok(animations)
    }

    fn find_animation_nodes(node: &roxmltree::Node<'_, '_>, animations: &mut Vec<Animation>) {
        for child in node.children() {
            let tag_name = child.tag_name().name();
            match tag_name {
                "anim" | "animMotion" | "animScale" | "animRot" | "animClr" | "set" => {
                    if let Some(anim) = Self::extract_animation(&child, tag_name) {
                        animations.push(anim);
                    }
                }
                _ => Self::find_animation_nodes(&child, animations),
            }
        }
    }

    fn extract_animation(node: &roxmltree::Node<'_, '_>, tag_name: &str) -> Option<Animation> {
        let anim_type = Self::get_animation_type(node, tag_name);
        let duration_ms = node
            .attribute("dur")
            .and_then(|dur| Self::parse_time_value(dur))
            .unwrap_or(500);
        let delay_ms = node
            .attribute("delay")
            .and_then(|d| Self::parse_time_value(d))
            .unwrap_or(0);
        let trigger = Self::get_trigger_type(node);
        let target_element = node
            .attribute("spid")
            .or_else(|| node.attribute("tgtEl"))
            .map(|s| s.to_string());
        let subtype = node.attribute("presetClass").map(|s| s.to_string());

        Some(Animation {
            animation_type: anim_type,
            subtype,
            duration_ms,
            trigger,
            target_element,
            delay_ms,
        })
    }

    fn get_animation_type(node: &roxmltree::Node<'_, '_>, tag_name: &str) -> AnimationType {
        if let Some(preset) = node.attribute("presetID") {
            return AnimationType::from(preset);
        }
        match tag_name {
            "animMotion" => AnimationType::Fly,
            "animScale" => AnimationType::GrowAndShrink,
            "animRot" => AnimationType::Swivel,
            "animClr" => AnimationType::Fade,
            "set" => AnimationType::Appear,
            _ => AnimationType::None,
        }
    }

    fn get_trigger_type(node: &roxmltree::Node<'_, '_>) -> AnimationTrigger {
        if let Some(cond) = node.attribute("cond") {
            match cond {
                "click" => return AnimationTrigger::OnClick,
                "withPrevious" | "withPrev" => return AnimationTrigger::WithPrevious,
                "afterPrevious" | "afterPrev" => return AnimationTrigger::AfterPrevious,
                _ => {}
            }
        }
        for child in node.children() {
            if child.tag_name().name() == "stCond" {
                if let Some("onClick") = child.attribute("evt") {
                    return AnimationTrigger::OnClick;
                }
            }
        }
        AnimationTrigger::OnClick
    }

    fn parse_time_value(value: &str) -> Option<u32> {
        if value == "indefinite" {
            return Some(0);
        }
        let cleaned = value.trim_end_matches("ms");
        if let Ok(secs) = cleaned.parse::<f32>() {
            if secs < 10.0 {
                return Some((secs * 1000.0) as u32);
            }
            return Some(secs as u32);
        }
        cleaned.parse::<u32>().ok()
    }

    fn parse_transitions(xml: &str) -> Result<Option<Transition>, PptxParseError> {
        let doc = Document::parse(xml).map_err(|e| PptxParseError::XmlParse(e.to_string()))?;

        for node in doc.descendants() {
            if node.tag_name().name() == "transition" {
                let type_attr = node.attribute("type").or_else(|| node.attribute("prst"));
                let dir_attr = node.attribute("dir");
                let transition_type = TransitionType::parse(type_attr, dir_attr);

                let duration_ms = node
                    .attribute("dur")
                    .and_then(|d| Self::parse_time_value(d))
                    .unwrap_or(1000);

                return Ok(Some(Transition {
                    transition_type,
                    duration_ms,
                }));
            }
        }
        Ok(None)
    }

    /// Parse slide relationships to build a map of rId -> (Target, Type)
    fn parse_slide_rels(
        archive: &mut ZipArchive<File>,
        slide_num: u32,
    ) -> Result<std::collections::HashMap<String, (String, String)>, PptxParseError> {
        let rels_path = format!("ppt/slides/_rels/slide{}.xml.rels", slide_num);
        let mut rels_map = std::collections::HashMap::new();

        let mut rels_xml = String::new();
        if let Ok(mut rels_file) = archive.by_name(&rels_path) {
            rels_file.read_to_string(&mut rels_xml)?;

            let doc =
                Document::parse(&rels_xml).map_err(|e| PptxParseError::XmlParse(e.to_string()))?;

            for node in doc.descendants() {
                if node.tag_name().name() == "Relationship" {
                    if let (Some(id), Some(target), Some(rel_type)) = (
                        node.attribute("Id"),
                        node.attribute("Target"),
                        node.attribute("Type"),
                    ) {
                        rels_map.insert(id.to_string(), (target.to_string(), rel_type.to_string()));
                    }
                }
            }
        }

        Ok(rels_map)
    }

    fn parse_embedded_media(
        xml: &str,
        rels: &std::collections::HashMap<String, (String, String)>,
    ) -> Result<Vec<EmbeddedMedia>, PptxParseError> {
        let doc = Document::parse(xml).map_err(|e| PptxParseError::XmlParse(e.to_string()))?;
        let mut media = Vec::new();
        let mut processed_rids = std::collections::HashSet::new();

        // Namespace URI for relationship attributes (r:)
        const NS_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

        for node in doc.descendants() {
            let tag_name = node.tag_name().name();

            // Check for videoFile references (a:videoFile)
            if tag_name == "videoFile" {
                // The attribute is r:link - need namespace-aware lookup
                if let Some(rid) = node.attribute((NS_REL, "link")) {
                    if let Some((target, _rel_type)) = rels.get(rid) {
                        if processed_rids.insert(rid.to_string()) {
                            media.push(EmbeddedMedia {
                                media_type: MediaType::Video,
                                filename: target.to_string(),
                                content_type: Self::guess_video_type(target),
                            });
                        }
                    }
                }
            }

            // Check for media references (p14:media) - used for both video and audio
            if tag_name == "media" {
                // The attribute is r:link - need namespace-aware lookup
                if let Some(rid) = node.attribute((NS_REL, "link")) {
                    if let Some((target, rel_type)) = rels.get(rid) {
                        if processed_rids.insert(rid.to_string()) {
                            // Determine if video or audio based on relationship type
                            let media_type = if rel_type.contains("/video") {
                                MediaType::Video
                            } else if rel_type.contains("/audio") {
                                MediaType::Audio
                            } else {
                                // Check file extension
                                if Self::guess_video_type(target) != "application/octet-stream" {
                                    MediaType::Video
                                } else {
                                    MediaType::Audio
                                }
                            };

                            let content_type = match media_type {
                                MediaType::Video => Self::guess_video_type(target),
                                MediaType::Audio => Self::guess_audio_type(target),
                            };

                            media.push(EmbeddedMedia {
                                media_type,
                                filename: target.to_string(),
                                content_type,
                            });
                        }
                    }
                }
            }

            // Legacy support for old-style audio/video elements
            if tag_name == "audio" {
                if let Some(name) = node.attribute("name") {
                    media.push(EmbeddedMedia {
                        media_type: MediaType::Audio,
                        filename: name.to_string(),
                        content_type: Self::guess_audio_type(name),
                    });
                }
            }
            if tag_name == "video" {
                if let Some(name) = node.attribute("name") {
                    media.push(EmbeddedMedia {
                        media_type: MediaType::Video,
                        filename: name.to_string(),
                        content_type: Self::guess_video_type(name),
                    });
                }
            }
        }
        Ok(media)
    }

    fn guess_audio_type(filename: &str) -> String {
        let ext = filename.split('.').last().unwrap_or("mp3").to_lowercase();
        match ext.as_str() {
            "mp3" => "audio/mpeg",
            "wav" => "audio/wav",
            "wma" => "audio/x-ms-wma",
            "m4a" => "audio/mp4",
            "aac" => "audio/aac",
            "ogg" => "audio/ogg",
            _ => "audio/mpeg",
        }
        .to_string()
    }

    fn guess_video_type(filename: &str) -> String {
        let ext = filename.split('.').last().unwrap_or("mp4").to_lowercase();
        match ext.as_str() {
            "mp4" | "m4v" => "video/mp4",
            "avi" => "video/x-msvideo",
            "wmv" => "video/x-ms-wmv",
            "mov" | "qt" => "video/quicktime",
            "mkv" => "video/x-matroska",
            "flv" => "video/x-flv",
            "webm" => "video/webm",
            _ => "video/mp4",
        }
        .to_string()
    }

    pub fn get_summary(info: &PptxInfo) -> String {
        let mut summary = format!("Presentation with {} slides\n\n", info.slide_count);

        for slide in &info.slides {
            summary.push_str(&format!("Slide {}: ", slide.slide_number));

            if !slide.animations.is_empty() {
                summary.push_str(&format!("{} animations [", slide.animations.len()));
                for (i, anim) in slide.animations.iter().enumerate() {
                    if i > 0 {
                        summary.push_str(", ");
                    }
                    summary.push_str(&format!("{:?}", anim.animation_type));
                }
                summary.push_str("], ");
            }

            if !slide.embedded_media.is_empty() {
                let audio_count = slide
                    .embedded_media
                    .iter()
                    .filter(|m| matches!(m.media_type, MediaType::Audio))
                    .count();
                let video_count = slide
                    .embedded_media
                    .iter()
                    .filter(|m| matches!(m.media_type, MediaType::Video))
                    .count();
                if audio_count > 0 {
                    summary.push_str(&format!("{} audio, ", audio_count));
                }
                if video_count > 0 {
                    summary.push_str(&format!("{} video, ", video_count));
                }
            }

            if let Some(ref trans) = slide.transition {
                let type_str = format!("{:?}", trans.transition_type);
                summary.push_str(&format!("{} ({}ms)", type_str, trans.duration_ms));
            } else {
                summary.push_str("no transition");
            }

            summary.push('\n');
        }

        summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_time_value() {
        assert_eq!(PptxParser::parse_time_value("500"), Some(500));
        assert_eq!(PptxParser::parse_time_value("1.5"), Some(1500));
        assert_eq!(PptxParser::parse_time_value("indefinite"), Some(0));
    }

    #[test]
    fn test_animation_type_from_str() {
        assert!(matches!(AnimationType::from("fade"), AnimationType::Fade));
        assert!(matches!(AnimationType::from("fly"), AnimationType::Fly));
    }

    #[test]
    fn test_direction_from_str() {
        assert_eq!(Direction::from("left"), Direction::Left);
        assert_eq!(Direction::from("right"), Direction::Right);
        assert_eq!(Direction::from("up"), Direction::Up);
        assert_eq!(Direction::from("tl"), Direction::TopLeft);
    }

    #[test]
    fn test_transition_type_parse() {
        // No direction
        assert_eq!(
            TransitionType::parse(Some("fade"), None),
            TransitionType::Fade
        );
        assert_eq!(
            TransitionType::parse(Some("morph"), None),
            TransitionType::Morph
        );

        // With direction
        assert_eq!(
            TransitionType::parse(Some("push"), Some("left")),
            TransitionType::Push(Direction::Left)
        );
        assert_eq!(
            TransitionType::parse(Some("wipe"), Some("right")),
            TransitionType::Wipe(Direction::Right)
        );

        // Zoom center
        assert_eq!(
            TransitionType::parse(Some("zoom"), Some("center")),
            TransitionType::ZoomCenter
        );
    }

    #[test]
    fn test_transition_has_direction() {
        assert!(TransitionType::Push(Direction::Left).has_direction());
        assert!(TransitionType::Wipe(Direction::Right).has_direction());
        assert!(!TransitionType::Fade.has_direction());
        assert!(!TransitionType::Morph.has_direction());
    }

    #[test]
    fn test_transition_direction() {
        assert_eq!(
            TransitionType::Push(Direction::Left).direction(),
            Some(&Direction::Left)
        );
        assert_eq!(TransitionType::Fade.direction(), None);
    }

    #[test]
    fn test_guess_audio_type() {
        assert_eq!(PptxParser::guess_audio_type("sound.mp3"), "audio/mpeg");
        assert_eq!(PptxParser::guess_audio_type("sound.wav"), "audio/wav");
    }

    #[test]
    fn test_guess_video_type() {
        assert_eq!(PptxParser::guess_video_type("video.mp4"), "video/mp4");
        assert_eq!(PptxParser::guess_video_type("video.mov"), "video/quicktime");
    }

    #[test]
    fn trys() {
        let a = PptxParser::parse(Path::new("Extlst-test.pptx"));
        println!("{:?}", a);
    }

    #[test]
    fn test_video_extraction() {
        let slide_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p14="http://schemas.microsoft.com/office/powerpoint/2010/main">
  <p:cSld>
    <p:spTree>
      <p:pic>
        <p:nvPicPr>
          <p:cNvPr id="65" name="">
            <a:hlinkClick r:id="" action="ppaction://media"/>
          </p:cNvPr>
          <p:cNvPicPr/>
          <p:nvPr>
            <a:videoFile r:link="rId6"/>
            <p:extLst>
              <p:ext uri="{DAA4B4D4-6D71-4841-9C94-3DE7FCFB9230}">
                <p14:media r:link="rId7"/>
              </p:ext>
            </p:extLst>
          </p:nvPr>
        </p:nvPicPr>
        <p:blipFill>
          <a:blip r:embed="rId8"></a:blip>
          <a:stretch><a:fillRect/></a:stretch>
        </p:blipFill>
        <p:spPr>
          <a:xfrm>
            <a:off x="1551960" y="1732320"/>
            <a:ext cx="6095520" cy="3428640"/>
          </a:xfrm>
          <a:prstGeom prst="rect"><a:avLst/></a:prstGeom>
          <a:ln w="0"><a:noFill/></a:ln>
        </p:spPr>
      </p:pic>
    </p:spTree>
  </p:cSld>
</p:sld>"#;

        let mut rels = std::collections::HashMap::new();
        rels.insert(
            "rId6".to_string(),
            (
                "file:///var/home/sergio/Descargas/video.mp4".to_string(),
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/video"
                    .to_string(),
            ),
        );
        rels.insert(
            "rId7".to_string(),
            (
                "file:///var/home/sergio/Descargas/video.mp4".to_string(),
                "http://schemas.microsoft.com/office/2007/relationships/media".to_string(),
            ),
        );

        let media = PptxParser::parse_embedded_media(slide_xml, &rels).unwrap();
        // Should find 2 media references (rId6 from videoFile and rId7 from p14:media)
        assert_eq!(
            media.len(),
            2,
            "Should find videoFile and p14:media references"
        );
        assert!(matches!(media[0].media_type, MediaType::Video));
        assert!(media[0].filename.contains("video.mp4"));
        assert!(matches!(media[1].media_type, MediaType::Video));
        assert!(media[1].filename.contains("video.mp4"));
    }
}
