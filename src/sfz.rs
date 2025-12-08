//! Basic SFZ parser - supports common opcodes needed for most instruments
//!
//! Supported opcodes:
//! - sample, key, lokey, hikey, pitch_keycenter
//! - lovel, hivel
//! - loop_mode, loop_start, loop_end
//! - tune, volume, pan
//! - seq_length, seq_position (round robin)
//! - group, off_by (voice groups - basic)
//! - offset, end (sample start/end)
//! - default_path

use crate::loader::load_audio;
use crate::sample::{Instrument, Region};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// SFZ section types
#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    None,
    Control,
    Global,
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

/// Parse an SFZ file and load the instrument
pub fn load_sfz(sfz_path: &Path) -> Result<Instrument, String> {
    let content = std::fs::read_to_string(sfz_path)
        .map_err(|e| format!("Failed to read {}: {}", sfz_path.display(), e))?;

    let base_dir = sfz_path.parent().unwrap_or(Path::new("."));
    let name = sfz_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown")
        .to_string();

    let mut default_path = String::new();
    let mut global_opcodes = OpcodeSet::default();
    let mut group_opcodes = OpcodeSet::default();
    let mut current_section = Section::None;
    let mut regions: Vec<Region> = Vec::new();

    // Pending region being built
    let mut pending_region: Option<OpcodeSet> = None;

    for line in content.lines() {
        let line = strip_comments(line).trim();
        if line.is_empty() {
            continue;
        }

        // Check for section headers
        if line.starts_with('<') && line.ends_with('>') {
            // Finalize pending region
            if let Some(region_ops) = pending_region.take() {
                if let Some(region) = build_region(&region_ops, base_dir, &default_path) {
                    regions.push(region);
                }
            }

            let section_name = &line[1..line.len() - 1].to_lowercase();
            current_section = match section_name.as_str() {
                "control" => Section::Control,
                "global" => Section::Global,
                "group" => {
                    group_opcodes = OpcodeSet::default();
                    Section::Group
                }
                "region" => {
                    let mut ops = OpcodeSet::default();
                    ops.merge(&global_opcodes);
                    ops.merge(&group_opcodes);
                    pending_region = Some(ops);
                    Section::Region
                }
                _ => Section::None,
            };
            continue;
        }

        // Parse opcodes
        let opcodes = parse_opcodes(line);

        match current_section {
            Section::Control => {
                if let Some(path) = opcodes.get("default_path") {
                    default_path = path.clone();
                }
            }
            Section::Global => {
                apply_opcodes(&mut global_opcodes, &opcodes);
            }
            Section::Group => {
                apply_opcodes(&mut group_opcodes, &opcodes);
            }
            Section::Region => {
                if let Some(ref mut ops) = pending_region {
                    apply_opcodes(ops, &opcodes);
                }
            }
            Section::None => {}
        }
    }

    // Finalize last pending region
    if let Some(region_ops) = pending_region {
        if let Some(region) = build_region(&region_ops, base_dir, &default_path) {
            regions.push(region);
        }
    }

    if regions.is_empty() {
        return Err(format!("No valid regions found in {}", sfz_path.display()));
    }

    Ok(Instrument::new(name, regions))
}

fn strip_comments(line: &str) -> &str {
    // SFZ comments start with //
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

    while let Some((i, c)) = chars.next() {
        if c.is_ascii_alphabetic() || c == '_' {
            // Could be start of an opcode, look for '='
            let mut j = i;
            while let Some(&(idx, ch)) = chars.peek() {
                if ch.is_ascii_alphanumeric() || ch == '_' {
                    j = idx;
                    chars.next();
                } else if ch == '=' {
                    return Some(i);
                } else {
                    break;
                }
            }
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

            _ => {} // Ignore unsupported opcodes
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

    // Build sample path
    let sample_path = if default_path.is_empty() {
        base_dir.join(sample_name)
    } else {
        base_dir.join(default_path).join(sample_name)
    };

    // Normalize path separators (SFZ uses backslashes on Windows)
    let sample_path_str = sample_path.to_string_lossy().replace('\\', "/");
    let sample_path = Path::new(&sample_path_str);

    // Load audio
    let audio = match load_audio(sample_path) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Warning: {}", e);
            return None;
        }
    };

    // Determine loop settings
    let loop_enabled = ops
        .loop_mode
        .as_ref()
        .map(|m| m == "loop_continuous" || m == "loop_sustain")
        .unwrap_or(false);

    // Round robin
    let rr_group = ops.group.unwrap_or(0);
    let rr_seq = ops.seq_position.unwrap_or(1).saturating_sub(1); // SFZ is 1-based

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

        rr_group,
        rr_seq,

        tune_cents: ops.tune.unwrap_or(0.0),
        volume_db: ops.volume.unwrap_or(0.0),
        pan: ops.pan.map(|p| p / 100.0).unwrap_or(0.0), // SFZ pan is -100..100

        sample_path: sample_path.to_string_lossy().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_note() {
        assert_eq!(parse_note("60"), Some(60));
        assert_eq!(parse_note("c4"), Some(60));
        assert_eq!(parse_note("C4"), Some(60));
        assert_eq!(parse_note("c#4"), Some(61));
        assert_eq!(parse_note("db4"), Some(61));
        assert_eq!(parse_note("a4"), Some(69));
        assert_eq!(parse_note("c-1"), Some(0));
    }

    #[test]
    fn test_parse_opcodes() {
        let ops = parse_opcodes("sample=piano_c4.wav lokey=48 hikey=72 pitch_keycenter=60");
        assert_eq!(ops.get("sample"), Some(&"piano_c4.wav".to_string()));
        assert_eq!(ops.get("lokey"), Some(&"48".to_string()));
        assert_eq!(ops.get("hikey"), Some(&"72".to_string()));
        assert_eq!(ops.get("pitch_keycenter"), Some(&"60".to_string()));
    }
}
