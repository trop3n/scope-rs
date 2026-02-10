# 15. MIDI Control

This milestone added MIDI CC (Control Change) input so external hardware controllers can manipulate oscilloscope parameters in real time.

## MIDI Basics

MIDI (Musical Instrument Digital Interface) is a protocol for sending musical control data between devices. For our purposes, the relevant message type is **Control Change (CC)**:

```
Status byte       CC number      Value
[0xB0 | channel]  [0-127]        [0-127]
```

- **Status byte**: `0xB0` = CC on channel 0, `0xB1` = CC on channel 1, etc.
- **CC number**: Identifies which knob/slider (0-127)
- **Value**: The position (0-127, mapped to whatever range we need)

Most MIDI controllers send CC messages when you turn a knob or move a fader. Each physical control has a fixed CC number (configurable on most controllers).

## The `midir` Crate

`midir` provides cross-platform MIDI I/O for Rust:

```rust
use midir::MidiInput;

// Create a MIDI input instance
let midi_in = MidiInput::new("scope-rs")?;

// List available ports
for port in midi_in.ports().iter() {
    println!("{}", midi_in.port_name(port)?);
}

// Connect with a callback
let connection = midi_in.connect(
    &port,
    "scope-rs-input",
    |timestamp, message, _data| {
        // Called on the MIDI thread for each message
        println!("{}: {:?}", timestamp, message);
    },
    (),
)?;

// Connection stays alive as long as `connection` exists
// Drop it to disconnect
```

### Platform Backends

| Platform | Backend |
|----------|---------|
| Linux | ALSA |
| macOS | CoreMIDI |
| Windows | WinMM |

`midir` abstracts over all three -- no platform-specific code needed.

### Key Design Point

`MidiInput::connect()` **consumes** the `MidiInput` instance and returns a `MidiInputConnection`. This is Rust's ownership system ensuring you can't enumerate ports while a connection is active. To scan ports again, you need to create a new `MidiInput`.

## Lock-Free Communication

The MIDI callback runs on its own thread. We need to pass CC values to the UI thread without blocking either side. The same principle from Milestone 12 (Lock-Free Audio) applies here.

### Shared Atomic Array

We use an array of 128 atomic bytes -- one per possible CC number:

```rust
struct SharedCcValues {
    values: Arc<[AtomicU8; 128]>,
    changed: Arc<[AtomicU8; 128]>,
}
```

The MIDI callback writes:
```rust
fn set(&self, cc: u8, value: u8) {
    self.values[cc as usize].store(value, Ordering::Relaxed);
    self.changed[cc as usize].store(1, Ordering::Relaxed);
}
```

The UI thread reads:
```rust
fn poll(&self, cc: u8) -> Option<u8> {
    if self.changed[cc as usize].swap(0, Ordering::Relaxed) != 0 {
        Some(self.values[cc as usize].load(Ordering::Relaxed))
    } else {
        None
    }
}
```

This is simpler than a ring buffer because we only care about the *latest* value for each CC, not a history. The `changed` flags ensure we only process values that actually arrived since the last frame.

### Why `Ordering::Relaxed`?

- We don't need ordering guarantees between different CC numbers
- We don't need the UI to see values in the exact order they arrived
- We just need each individual atomic operation to be atomic (no torn reads)
- `Relaxed` is the cheapest ordering -- perfect for this use case

## Parameter Mapping

### The `MidiParam` Enum

Each mappable parameter has a defined range:

```rust
pub enum MidiParam {
    Gain,        // 0.1 - 10.0
    Volume,      // 0.0 - 2.0
    Speed,       // 0.25 - 2.0
    LineWidth,   // 0.5 - 5.0
    Intensity,   // 0.1 - 1.0
    Persistence, // 0.0 - 0.99
    Zoom,        // 0.1 - 2.0
    DcOffsetX,   // -1.0 - 1.0
    DcOffsetY,   // -1.0 - 1.0
}
```

### Value Mapping

MIDI CC values (0-127) are linearly mapped to each parameter's range:

```rust
pub fn map_value(&self, cc_value: u8) -> f32 {
    let t = cc_value as f32 / 127.0; // Normalize to 0.0..1.0
    let (min, max) = self.range();
    min + t * (max - min)             // Scale to parameter range
}
```

For example, a CC value of 64 (midpoint) maps to:
- Gain: `0.1 + 0.504 * 9.9 = 5.09`
- Zoom: `0.1 + 0.504 * 1.9 = 1.06`
- DC Offset X: `-1.0 + 0.504 * 2.0 = 0.008`

## MIDI Learn

Instead of requiring users to know their controller's CC numbers, we support "MIDI learn":

1. User clicks **Learn** next to a mapping
2. App enters learn mode for that mapping
3. User moves the desired knob/fader on their controller
4. The first CC received is assigned to that mapping
5. Learn mode exits automatically

```rust
pub fn poll(&mut self) -> Vec<(MidiParam, f32)> {
    if let Some(mapping_idx) = self.learning {
        // In learn mode: scan all CCs for any activity
        for cc in 0..128u8 {
            if self.cc_values.poll(cc).is_some() {
                if let Some(mapping) = self.mappings.get_mut(mapping_idx) {
                    mapping.cc = cc;
                }
                self.learning = None;
                return vec![];
            }
        }
        return vec![];
    }

    // Normal mode: apply mapped values
    // ...
}
```

## Borrow Checker Challenge

The settings UI needs to iterate over `self.midi.mappings` while also calling methods on `self.midi` (like `start_learn()` and `cancel_learn()`). Rust's borrow checker prevents this:

```rust
// WON'T COMPILE: can't borrow self.midi mutably while iterating
for (i, mapping) in self.midi.mappings.iter().enumerate() {
    if ui.button("Learn").clicked() {
        self.midi.start_learn(i); // ERROR: mutable borrow while immutable borrow active
    }
}
```

The solution: snapshot the data first, collect UI actions, then apply them:

```rust
// 1. Snapshot (immutable borrow ends here)
let mapping_info: Vec<_> = self.midi.mappings.iter().enumerate()
    .map(|(i, m)| (i, m.cc, m.param.name()))
    .collect();

// 2. Render UI using snapshot (no borrow on self.midi)
let mut learn_idx = None;
for (i, cc, name) in &mapping_info {
    if ui.button("Learn").clicked() {
        learn_idx = Some(*i);
    }
}

// 3. Apply deferred actions (mutable borrow is fine now)
if let Some(idx) = learn_idx {
    self.midi.start_learn(idx);
}
```

This "snapshot then defer" pattern is common in immediate-mode GUIs with Rust's ownership model.

## Persistence

MIDI mappings are saved as part of `AppSettings` (Milestone 14):

```json
{
  "midi_mappings": [
    {"cc": 1, "param": "Gain"},
    {"cc": 7, "param": "Volume"},
    {"cc": 74, "param": "Zoom"}
  ]
}
```

Both `MidiParam` and `MidiMapping` derive `Serialize`/`Deserialize`, so they serialize cleanly as JSON. On startup, saved mappings are restored so users don't have to reconfigure their controller each session.

Note: we don't persist the MIDI device name because, like audio devices, MIDI port names can change between sessions.

## Architecture

```
┌──────────────┐              ┌──────────────────────┐
│ MIDI Device  │              │      UI Thread        │
│  (hardware)  │              │                       │
└──────┬───────┘              │  MidiController       │
       │                      │  ├── ports[]          │
       │ MIDI messages        │  ├── mappings[]       │
       v                      │  └── cc_values ◄──────┤
┌──────────────┐    atomic    │       (shared)        │
│ MIDI Thread  ├─── writes ───►                       │
│  (callback)  │              │  poll() each frame:   │
└──────────────┘              │  CC value → param     │
                              │  → update app state   │
                              └──────────────────────┘
```

## Key Takeaways

1. **MIDI CC is simple** -- just three bytes: status, CC number, value. Perfect for mapping to sliders and knobs.

2. **Atomics for simple shared state** -- when you only need the latest value (not a queue), an atomic array is simpler and faster than a ring buffer.

3. **`MidiInput::connect()` consumes self** -- Rust's ownership system prevents misuse at compile time. This is a great example of "making invalid states unrepresentable."

4. **MIDI learn is a UX essential** -- users shouldn't need to know their controller's CC mapping tables.

5. **Snapshot-then-defer for borrow checker** -- in immediate-mode GUIs, collect actions during rendering and apply them afterward to avoid mutable+immutable borrow conflicts.

6. **Persist mappings, not devices** -- MIDI port names change; CC assignments don't.

## Links

- [midir crate](https://docs.rs/midir/)
- [MIDI CC Messages](https://www.midi.org/specifications-old/item/table-3-control-change-messages-data-bytes-2)
- [MIDI Specification](https://www.midi.org/specifications)
