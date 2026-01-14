# Samplo

A minimal Rust CLAP sampler with SFZ support, running headless on Android (yadaw) and desktop hosts.

## Quick Start

1. Create the instruments directory:
   - **Linux/macOS**: `~/Documents/Samplo/instruments/`
   - **Windows**: `Documents\Samplo\instruments\`
   - **Android**: `/storage/emulated/0/Samplo/instruments/`

2. Place your `.sfz` or `.json` instrument files in that directory

3. Load the plugin in your DAW and select an instrument using the "Instrument" parameter

## Instrument Locations

Samplo searches for instruments in these directories (in order):

| Platform | Path |
|----------|------|
| All | `./instruments/` (relative to working directory) |
| Android | `/storage/emulated/0/Samplo/instruments/` |
| Linux/macOS | `~/Documents/Samplo/instruments/` |
| Windows | `%USERPROFILE%\Documents\Samplo\instruments\` |

### Directory Structure

Samplo scans up to 2 levels deep, making it compatible with instrument collections like [sfzinstruments](https://github.com/sfzinstruments):

```
~/Documents/Samplo/instruments/
├── Piano.sfz                    # ✓ Found
├── MyPiano/
│   ├── piano.sfz                # ✓ Found
│   └── samples/                 # Skipped (samples directory)
│       └── c4.wav
├── sfzinstruments-piano/        # Git repo structure
│   └── GM Piano/
│       ├── piano.sfz            # ✓ Found (2 levels deep)
│       └── samples/
└── .hidden-folder/              # Skipped (hidden)
```

**Note**: Directories named `samples`, `waves`, or `audio` are skipped during scanning (they typically contain audio files, not instruments).

## Parameters

| Parameter | Range | Default | Description |
|-----------|-------|---------|-------------|
| **Instrument** | 0-2047 | 0 | Select instrument (sorted alphabetically) |
| **Attack** | 0-2000 ms | 5 ms | Amplitude envelope attack time |
| **Decay** | 1-4000 ms | 100 ms | Amplitude envelope decay time |
| **Sustain** | 0-1 | 1.0 | Amplitude envelope sustain level |
| **Release** | 1-8000 ms | 200 ms | Amplitude envelope release time |
| **Filter** | Off/LP/HP/BP | Off | Filter type (Low/High/Band pass) |
| **Cutoff** | 20-20000 Hz | 8000 Hz | Filter cutoff frequency |
| **Resonance** | 0.1-4.0 | 0.5 | Filter resonance (Q) |
| **Gain** | 0-2× | 0.8× | Output gain |
| **Pan** | -1 to +1 | 0 | Stereo panning |
| **Tune** | -100 to +100 cents | 0 | Fine pitch adjustment |
| **Voices** | 1-64 | 32 | Maximum polyphony |
| **Vel Sens** | 0-1 | 0.7 | Velocity sensitivity |

## Features

- **Audio formats**: WAV, FLAC, OGG Vorbis
- **Instrument formats**: SFZ, JSON
- **Multi-sample mapping**: Note and velocity layers
- **Round robin**: Automatic sample cycling for realistic playback
- **Interpolation**: 4-point Hermite for quality pitch shifting
- **Loops**: Sustain loop support
- **ADSR envelope**: Per-voice amplitude shaping
- **Filter**: Zero-delay feedback SVF (LP/HP/BP)
- **Polyphony**: Up to 64 voices with oldest-voice stealing
- **Headless**: No GUI required

## SFZ Support

### Supported Opcodes

| Category | Opcodes |
|----------|---------|
| Sample | `sample`, `offset`, `end` |
| Mapping | `key`, `lokey`, `hikey`, `pitch_keycenter` |
| Velocity | `lovel`, `hivel` |
| Loop | `loop_mode`, `loop_start`, `loop_end` |
| Tuning | `tune`, `volume`, `pan` |
| Round Robin | `seq_length`, `seq_position`, `group` |
| Control | `default_path`, `#include`, `#define` |

### Loop Modes

- `no_loop` - One-shot playback (default)
- `loop_continuous` - Loop forever
- `loop_sustain` - Loop while note held, then play to end

### Example SFZ

```sfz
<control>
default_path=samples/

<global>
loop_mode=no_loop

<group>
lokey=60 hikey=60

<region>
sample=piano_c4_rr1.wav
seq_position=1
seq_length=3

<region>
sample=piano_c4_rr2.wav
seq_position=2
seq_length=3

<region>
sample=piano_c4_rr3.wav
seq_position=3
seq_length=3
```

## JSON Format

Alternative to SFZ for simple instruments:

```json
{
  "name": "Ukulele",
  "regions": [
    {
      "sample": "uke_c4_v1.wav",
      "root": 60,
      "lo_note": 48,
      "hi_note": 71,
      "lo_vel": 0,
      "hi_vel": 80,
      "rr_group": 1,
      "rr_seq": 0
    },
    {
      "sample": "uke_c4_v2.wav",
      "root": 60,
      "lo_note": 48,
      "hi_note": 71,
      "lo_vel": 81,
      "hi_vel": 127,
      "rr_group": 1,
      "rr_seq": 1
    }
  ]
}
```

### JSON Region Properties

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `sample` | string | required | Path to audio file (relative to JSON) |
| `root` | int | 60 | Root note (MIDI number) |
| `lo_note` | int | 0 | Lowest mapped note |
| `hi_note` | int | 127 | Highest mapped note |
| `lo_vel` | int | 0 | Lowest velocity |
| `hi_vel` | int | 127 | Highest velocity |
| `loop_start` | int | null | Loop start frame |
| `loop_end` | int | null | Loop end frame |
| `loop_enabled` | bool | false | Enable looping |
| `rr_group` | int | 0 | Round-robin group |
| `rr_seq` | int | 0 | Round-robin sequence number |
| `tune_cents` | float | 0 | Fine tuning in cents |
| `volume_db` | float | 0 | Volume adjustment in dB |
| `pan` | float | 0 | Pan (-1 to +1) |

## Troubleshooting

### No instruments found

1. Check that the instruments directory exists
2. Verify file extensions are `.sfz` or `.json` (lowercase)
3. Enable logging to see scan results:
   ```
   RUST_LOG=info your_daw
   ```

### Samples not loading

1. Ensure `default_path` in SFZ points to correct directory
2. Check sample paths use forward slashes (`/`) not backslashes
3. Verify audio files are WAV, FLAC, or OGG format
4. Check file permissions

### Test tone plays instead of instrument

If you hear a sine wave at A4 (440Hz), the instrument failed to load. Check logs for details.

### Clicking or artifacts

- Increase attack time (try 5-10ms minimum)
- Check if samples have proper zero-crossings at start/end
- Reduce polyphony if CPU is overloaded

## Build

```sh
# Linux
cargo build --release
cp target/release/libsamplo.so ~/.clap/Samplo.clap

# macOS
cargo build --release
cp target/release/libsamplo.dylib ~/.clap/Samplo.clap

# Windows
cargo build --release
copy target\release\samplo.dll "%LOCALAPPDATA%\Programs\Common\CLAP\Samplo.clap"

# Android (Termux with cargo-ndk)
cargo ndk -t arm64-v8a --platform 26 build --release
cp target/aarch64-linux-android/release/libsamplo.so Samplo.clap
```

## CLAP Installation Paths

| Platform | Path |
|----------|------|
| Linux | `~/.clap/` or `/usr/lib/clap/` |
| macOS | `~/Library/Audio/Plug-Ins/CLAP/` |
| Windows | `%LOCALAPPDATA%\Programs\Common\CLAP\` |
| Android | App-specific (varies by host) |

Is also available on Arch User Repository (as samplo-bin), and flathub

## License

[GPLv3](LICENSE)
