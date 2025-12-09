//! Basic SFZ parser - supports common opcodes needed for most instruments
//!

use crate::loader::load_audio;
use crate::sample::{Instrument, Region};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// SFZ section types
#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    None,
    Control,
    Global,
    Master,
    Group,
    Region,
}

/// Accumulated opcode values (inherited from control -> global -> group -> region)
#[derive(Clone, Default)]
struct OpcodeSet {
    // Sample
    sample: Option<String>,
    offset: Option<usize>,
    end: Option<usize>,

    // Key mapping
    lokey: Option<u8>,
    hikey: Option<u8>,
    pitch_keycenter: Option<u8>,
    key: Option<u8>, // Sets all three above

    // Velocity mapping
    lovel: Option<u8>,
    hivel: Option<u8>,

    // Loop
    loop_mode: Option<String>,
    loop_start: Option<usize>,
    loop_end: Option<usize>,

    // Tuning/volume
    tune: Option<f32>,
    volume: Option<f32>,
    pan: Option<f32>,

    // Round robin
    seq_length: Option<u32>,
    seq_position: Option<u32>,

    // Voice groups (basic)
    group: Option<u32>,
}

impl OpcodeSet {
    fn merge(&mut self, other: &OpcodeSet) {
        macro_rules! merge_field {
            ($field:ident) => {
                if other.$field.is_some() {
                    self.$field = other.$field.clone();
                }
            };
        }
        merge_field!(sample);
        merge_field!(offset);
        merge_field!(end);
        merge_field!(lokey);
        merge_field!(hikey);
        merge_field!(pitch_keycenter);
        merge_field!(key);
        merge_field!(lovel);
        merge_field!(hivel);
        merge_field!(loop_mode);
        merge_field!(loop_start);
        merge_field!(loop_end);
        merge_field!(tune);
        merge_field!(volume);
        merge_field!(pan);
        merge_field!(seq_length);
        merge_field!(seq_position);
        merge_field!(group);
    }
}

struct SfzParser {
    base_dir: PathBuf,
    defines: HashMap<String, String>,
    default_path: String,
    global_opcodes: OpcodeSet,
    master_opcodes: OpcodeSet,
    group_opcodes: OpcodeSet,
    current_section: Section,
    regions: Vec<Region>,
    pending_region: Option<OpcodeSet>,
    include_depth: usize,
    failed_samples: Vec<String>,
}

impl SfzParser {
    fn new(base_dir: PathBuf) -> Self {
        Self {
            base_dir,
            defines: HashMap::new(),
            default_path: String::new(),
            global_opcodes: OpcodeSet::default(),
            master_opcodes: OpcodeSet::default(),
            group_opcodes: OpcodeSet::default(),
            current_section: Section::None,
            regions: Vec::new(),
            pending_region: None,
            include_depth: 0,
            failed_samples: Vec::new(),
        }
    }

    fn expand_defines(&self, line: &str) -> String {
        let mut result = line.to_string();
        for (key, value) in &self.defines {
            result = result.replace(key, value);
        }
        result
    }

    fn parse_file(&mut self, path: &Path) -> Result<(), String> {
        if self.include_depth > 10 {
            return Err("Include depth exceeded (possible circular include)".to_string());
        }

        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;

        self.include_depth += 1;

        for line in content.lines() {
            self.parse_line(line)?;
        }

        self.include_depth -= 1;
        Ok(())
    }

    fn parse_line(&mut self, line: &str) -> Result<(), String> {
        let line = strip_comments(line).trim();
        if line.is_empty() {
            return Ok(());
        }

        // Handle #define
        if line.starts_with("#define") {
            let parts: Vec<&str> = line.splitn(3, ' ').collect();
            if parts.len() >= 3 {
                let key = parts[1].to_string();
                let value = parts[2].to_string();
                self.defines.insert(key, value);
            }
            return Ok(());
        }

        // Handle #include
        if line.starts_with("#include") {
            let include_path = line
                .trim_start_matches("#include")
                .trim()
                .trim_matches('"')
                .trim_matches('\'');

            let full_path = self.base_dir.join(include_path);

            if full_path.exists() {
                self.parse_file(&full_path)?;
            } else {
                nih_plug::nih_log!("Include not found: {}", full_path.display());
            }
            return Ok(());
        }

        // Expand defines in the line
        let line = self.expand_defines(line);
        let line = line.trim();

        // Handle section headers
        if line.contains('<') {
            self.finalize_pending_region();

            if let Some(start) = line.find('<') {
                if let Some(end) = line.find('>') {
                    let section_name = line[start + 1..end].to_lowercase();
                    let rest_of_line = line[end + 1..].trim();

                    self.current_section = match section_name.as_str() {
                        "control" => Section::Control,
                        "global" => Section::Global,
                        "master" => {
                            self.master_opcodes = OpcodeSet::default();
                            self.master_opcodes.merge(&self.global_opcodes);
                            Section::Master
                        }
                        "group" => {
                            self.group_opcodes = OpcodeSet::default();
                            self.group_opcodes.merge(&self.global_opcodes);
                            self.group_opcodes.merge(&self.master_opcodes);
                            Section::Group
                        }
                        "region" => {
                            let mut ops = OpcodeSet::default();
                            ops.merge(&self.global_opcodes);
                            ops.merge(&self.master_opcodes);
                            ops.merge(&self.group_opcodes);
                            self.pending_region = Some(ops);
                            Section::Region
                        }
                        "curve" => Section::None, // Skip curve definitions
                        _ => Section::None,
                    };

                    if !rest_of_line.is_empty() {
                        self.apply_opcodes_to_section(rest_of_line);
                    }
                    return Ok(());
                }
            }
        }

        // Parse opcodes
        self.apply_opcodes_to_section(&line);
        Ok(())
    }

    fn apply_opcodes_to_section(&mut self, line: &str) {
        let opcodes = parse_opcodes(line);

        match self.current_section {
            Section::Control => {
                if let Some(path) = opcodes.get("default_path") {
                    self.default_path = path.clone();
                }
            }
            Section::Global => apply_opcodes(&mut self.global_opcodes, &opcodes),
            Section::Master => apply_opcodes(&mut self.master_opcodes, &opcodes),
            Section::Group => apply_opcodes(&mut self.group_opcodes, &opcodes),
            Section::Region => {
                if let Some(ref mut ops) = self.pending_region {
                    apply_opcodes(ops, &opcodes);
                }
            }
            Section::None => {}
        }
    }

    fn finalize_pending_region(&mut self) {
        if let Some(region_ops) = self.pending_region.take() {
            match build_region(&region_ops, &self.base_dir, &self.default_path) {
                Some(region) => self.regions.push(region),
                None => {
                    if let Some(s) = &region_ops.sample {
                        self.failed_samples.push(s.clone());
                    }
                }
            }
        }
    }
}

pub fn load_sfz(sfz_path: &Path) -> Result<Instrument, String> {
    let base_dir = sfz_path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let name = sfz_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown")
        .to_string();

    nih_plug::nih_log!("Parsing SFZ: {}", sfz_path.display());

    let mut parser = SfzParser::new(base_dir);
    parser.parse_file(sfz_path)?;
    parser.finalize_pending_region();

    nih_plug::nih_log!(
        "SFZ complete: {} regions loaded, {} failed",
        parser.regions.len(),
        parser.failed_samples.len()
    );

    if !parser.failed_samples.is_empty() && parser.failed_samples.len() <= 10 {
        for s in &parser.failed_samples {
            nih_plug::nih_log!("  Failed: {}", s);
        }
    }

    if parser.regions.is_empty() {
        return Err(format!(
            "No valid regions in {} (check if samples exist)",
            sfz_path.display()
        ));
    }

    Ok(Instrument::new(name, parser.regions))
}

fn strip_comments(line: &str) -> &str {
    line.split("//").next().unwrap_or(line)
}

fn parse_opcodes(line: &str) -> HashMap<String, String> {
    let mut opcodes = HashMap::new();
    let mut remaining = line;

    while !remaining.is_empty() {
        remaining = remaining.trim_start();

        // Find the next '=' to get opcode name
        let Some(eq_pos) = remaining.find('=') else {
            break;
        };

        let key = remaining[..eq_pos].trim().to_lowercase();
        remaining = &remaining[eq_pos + 1..];

        // Value continues until next opcode or end of line
        // Opcodes are word characters followed by '='
        let value_end = find_next_opcode(remaining).unwrap_or(remaining.len());
        let value = remaining[..value_end].trim().to_string();

        opcodes.insert(key, value);
        remaining = &remaining[value_end..];
    }

    opcodes
}

fn find_next_opcode(s: &str) -> Option<usize> {
    let mut chars = s.char_indices().peekable();
    let mut in_word = false;
    let mut word_start = 0;

    while let Some((i, c)) = chars.next() {
        if c.is_ascii_alphabetic() || c == '_' {
            if !in_word {
                word_start = i;
                in_word = true;
            }
        } else if c == '=' && in_word {
            return Some(word_start);
        } else if c.is_whitespace() {
            in_word = false;
        } else if c.is_ascii_alphanumeric() {
            // Continue
        } else {
            in_word = false;
        }
    }
    None
}

fn apply_opcodes(ops: &mut OpcodeSet, parsed: &HashMap<String, String>) {
    for (key, value) in parsed {
        match key.as_str() {
            "sample" => ops.sample = Some(value.clone()),
            "offset" => ops.offset = value.parse().ok(),
            "end" => ops.end = value.parse().ok(),
            "key" => {
                if let Some(note) = parse_note(value) {
                    ops.key = Some(note);
                    ops.lokey = Some(note);
                    ops.hikey = Some(note);
                    ops.pitch_keycenter = Some(note);
                }
            }
            "lokey" => ops.lokey = parse_note(value),
            "hikey" => ops.hikey = parse_note(value),
            "pitch_keycenter" => ops.pitch_keycenter = parse_note(value),
            "lovel" => ops.lovel = value.parse().ok(),
            "hivel" => ops.hivel = value.parse().ok(),
            "loop_mode" => ops.loop_mode = Some(value.clone()),
            "loop_start" => ops.loop_start = value.parse().ok(),
            "loop_end" => ops.loop_end = value.parse().ok(),
            "tune" => ops.tune = value.parse().ok(),
            "volume" => ops.volume = value.parse().ok(),
            "pan" => ops.pan = value.parse().ok(),
            "seq_length" => ops.seq_length = value.parse().ok(),
            "seq_position" => ops.seq_position = value.parse().ok(),
            "group" => ops.group = value.parse().ok(),
            _ => {}
        }
    }
}

fn parse_note(s: &str) -> Option<u8> {
    // Try numeric first
    if let Ok(n) = s.parse::<u8>() {
        return Some(n.min(127));
    }

    // Try note name (c4, c#4, db4, etc.)
    let s = s.to_lowercase();
    let mut chars = s.chars().peekable();

    let note_base = match chars.next()? {
        'c' => 0,
        'd' => 2,
        'e' => 4,
        'f' => 5,
        'g' => 7,
        'a' => 9,
        'b' => 11,
        _ => return None,
    };

    let modifier = match chars.peek() {
        Some('#') | Some('s') => {
            chars.next();
            1i8
        }
        Some('b') | Some('f') => {
            chars.next();
            -1i8
        }
        _ => 0,
    };

    let octave_str: String = chars.collect();
    let octave: i8 = octave_str.parse().ok()?;

    // MIDI: C4 = 60
    let midi = (octave + 1) * 12 + note_base + modifier;

    if midi >= 0 && midi <= 127 {
        Some(midi as u8)
    } else {
        None
    }
}

fn build_region(ops: &OpcodeSet, base_dir: &Path, default_path: &str) -> Option<Region> {
    let sample_name = ops.sample.as_ref()?;
    let sample_name_normalized = sample_name.replace('\\', "/");

    let sample_path = if default_path.is_empty() {
        base_dir.join(&sample_name_normalized)
    } else {
        let dp = default_path.replace('\\', "/");
        base_dir.join(&dp).join(&sample_name_normalized)
    };

    if !sample_path.exists() {
        return None;
    }

    let audio = load_audio(&sample_path).ok()?;

    let loop_enabled = ops
        .loop_mode
        .as_ref()
        .map(|m| m == "loop_continuous" || m == "loop_sustain")
        .unwrap_or(false);

    Some(Region {
        data: Arc::new(audio.samples),
        channels: audio.channels,
        sample_rate: audio.sample_rate as f32,
        num_frames: audio.num_frames,
        root_note: ops.pitch_keycenter.or(ops.key).unwrap_or(60),
        lo_note: ops.lokey.unwrap_or(0),
        hi_note: ops.hikey.unwrap_or(127),
        lo_vel: ops.lovel.unwrap_or(0),
        hi_vel: ops.hivel.unwrap_or(127),
        loop_start: ops.loop_start,
        loop_end: ops.loop_end,
        loop_enabled,
        rr_group: ops.group.unwrap_or(0),
        rr_seq: ops.seq_position.unwrap_or(1).saturating_sub(1),
        tune_cents: ops.tune.unwrap_or(0.0),
        volume_db: ops.volume.unwrap_or(0.0),
        pan: ops.pan.map(|p| p / 100.0).unwrap_or(0.0),
        sample_path: sample_path.to_string_lossy().to_string(),
    })
}
