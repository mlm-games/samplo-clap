# Samplo

A minimal Rust CLAP sampler with SFZ support, running headless on Android (yadaw) and desktop hosts.

## Usage

Place your sfz (or json) in the Documents dir's Samplo/instruments directory, and use the instrument setting to control which instrument is loaded (always sorted alphabetically, enable logging (info) to find which instrument was selected)

## Features

- **Audio formats**: WAV, FLAC, OGG, Ogg Vorbis (via Symphonia)
- **Instrument formats**: JSON, SFZ
- **Multi-sample mapping**: Note and velocity layers
- **Round robin**: Automatic sample cycling for realistic playback
- **Interpolation**: 4-point Hermite for quality pitch shifting
- **Loops**: Sustain loop support
- **ADSR envelope**: Per-voice amplitude shaping
- **Filter**: Zero-delay SVF (LP/HP/BP)
- **Polyphony**: Up to 64 voices with oldest-voice stealing
- **Headless**: No GUI required

## SFZ Support

Supported opcodes:

| Category | Opcodes |
|----------|---------|
| Sample | `sample`, `offset`, `end` |
| Mapping | `key`, `lokey`, `hikey`, `pitch_keycenter` |
| Velocity | `lovel`, `hivel` |
| Loop | `loop_mode`, `loop_start`, `loop_end` |
| Tuning | `tune`, `volume`, `pan` |
| Round Robin | `seq_length`, `seq_position`, `group` |
| Control | `default_path` |

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

### Example Json

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
      "lo_vel": 0,
      "hi_vel": 80,
      "rr_group": 1,
      "rr_seq": 1
    }
  ]
}
```

### Build 

```sh
# Linux
cargo build --release
cp target/release/libsamplo.so Samplo.clap

# Windows
cargo build --release
copy target\release\samplo.dll Samplo.clap

# Android (Termux)
cargo ndk -t arm64-v8a --platform 26 build --release
cp target/aarch64-linux-android/release/libsamplo.so Samplo.clap
```

[License](LICENSE)
