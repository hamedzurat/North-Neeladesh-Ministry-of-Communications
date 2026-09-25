# Voice playback volume report

## Observed state

The Raspberry Pi output controls are already at full configured volume:

- PipeWire default sink: `1.00`
- MAX98357A ALSA mixer: `100%` (`255/255`)
- Frontend playback: `sounddevice.RawOutputStream`, `int16`, 24 kHz, mono
- Frontend playback path applies no attenuation

Increasing the Pi mixer volume is therefore not the current solution.

## Audio pipeline

```text
PocketTTS waveform
  -> pocket_tts.py encode_pcm()
  -> backend RTP packetization
  -> UDP 7879
  -> frontend RTP decoder
  -> sounddevice RawOutputStream
  -> PipeWire
  -> MAX98357A
```

`pocket_tts.py` currently applies `OUTPUT_GAIN = 1.5` and a soft limiter.
The generated RTP path sends those samples directly. The Rust
`NN_VOICE_PLAYBACK_GAIN` setting belongs to the separate command playback path
and does not amplify generated TTS RTP.

## Likely volume loss

The remaining likely source is the average level of the PocketTTS waveform.
Speech can have a low RMS level even when its peak is close to full scale. A
blind fixed gain increase could clip consonants and degrade quality.

## Recommended fix

Measure each generated response before packetization:

1. Calculate peak absolute sample and RMS level.
2. Apply transparent loudness normalization toward a configured speech target.
3. Cap the gain with a true-peak safety limit.
4. Apply a soft limiter only to the small number of samples that exceed the
   safety limit.
5. Log one summary per response:

```text
[VOICE] tts_level peak_dbfs=-4.2 rms_dbfs=-24.8 gain_db=8.0 clipped_samples=0
```

This increases quiet speech without blindly multiplying already-loud audio.

## Next measurement

Before changing normalization, add the level summary at the backend RTP
boundary and compare it with the frontend's first received RTP packet. This
will distinguish quiet generated PCM from a playback-device problem.
